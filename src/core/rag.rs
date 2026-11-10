//! Local document search (FTS5) over `rag.docs_dir` for chat prompt injection.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use super::config::RagCfg;
use super::paths;
use crate::core::project::Project;

const EXT: &[&str] = &["txt", "md", "pdf"];

#[derive(Debug, Clone)]
pub struct Hit {
    pub path: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    pub db_path: PathBuf,
    pub files: usize,
    pub chunks: usize,
}

fn db_path(root: &Path) -> PathBuf {
    root.join(".yourai").join("rag.sqlite")
}

fn stamp_path(root: &Path) -> PathBuf {
    root.join(".yourai").join("rag.stamp")
}

/// Fingerprint of docs dir contents + rag settings (for rebuild / stamp).
pub fn fingerprint(root: &Path, cfg: &RagCfg) -> String {
    let mut h = Sha256::new();
    h.update(serde_json::to_vec(cfg).unwrap_or_default());
    let Ok(docs) = paths::inside(root, &cfg.docs_dir) else {
        return hex::encode(h.finalize());
    };
    let mut entries: Vec<(String, u64, u64)> = Vec::new();
    if docs.is_dir() {
        walk_collect(&docs, root, &mut entries);
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (rel, len, mtime) in entries {
        h.update(rel.as_bytes());
        h.update(len.to_le_bytes());
        h.update(mtime.to_le_bytes());
    }
    hex::encode(h.finalize())
}

fn walk_collect(dir: &Path, root: &Path, out: &mut Vec<(String, u64, u64)>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else {
            continue;
        };
        if meta.is_dir() {
            walk_collect(&path, root, out);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|x| x.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if !EXT.contains(&ext.as_str()) {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push((rel, meta.len(), mtime));
    }
}

/// Split text into overlapping character chunks.
pub fn chunk_text(text: &str, chunk_chars: usize) -> Vec<(usize, String)> {
    let chunk_chars = chunk_chars.max(200);
    let overlap = (chunk_chars / 10).max(20);
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let end = (start + chunk_chars).min(chars.len());
        let piece: String = chars[start..end].iter().collect();
        let trimmed = piece.trim();
        if !trimmed.is_empty() {
            out.push((start, trimmed.to_string()));
        }
        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(overlap);
        if start >= end {
            start = end;
        }
    }
    out
}

fn read_doc(path: &Path) -> anyhow::Result<String> {
    let is_pdf = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    if is_pdf {
        Ok(pdf_extract::extract_text(path)?)
    } else {
        Ok(fs::read_to_string(path)?)
    }
}

fn open_db(path: &Path) -> anyhow::Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS documents (
           id INTEGER PRIMARY KEY,
           path TEXT NOT NULL,
           offset_chars INTEGER NOT NULL,
           body TEXT NOT NULL
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
           body,
           path UNINDEXED,
           offset_chars UNINDEXED,
           content='documents',
           content_rowid='id'
         );",
    )?;
    Ok(conn)
}

/// Rebuild the FTS index from `cfg.docs_dir`. Returns file and chunk counts.
pub fn index(p: &Project, cfg: &RagCfg) -> anyhow::Result<(usize, usize)> {
    let docs = paths::inside(&p.root, &cfg.docs_dir)?;
    let db = db_path(&p.root);
    if db.exists() {
        let _ = fs::remove_file(&db);
        let _ = fs::remove_file(PathBuf::from(format!("{}-wal", db.display())));
        let _ = fs::remove_file(PathBuf::from(format!("{}-shm", db.display())));
    }
    let conn = open_db(&db)?;
    let chunk_chars = cfg.chunk_chars as usize;
    let mut files = 0usize;
    let mut chunks = 0usize;

    let mut paths_list: Vec<PathBuf> = Vec::new();
    if docs.is_dir() {
        collect_files(&docs, &mut paths_list);
    }
    paths_list.sort();

    let tx = conn.unchecked_transaction()?;
    for path in &paths_list {
        let text = match read_doc(path) {
            Ok(t) => t,
            Err(e) => {
                crate::ui::warn(format!("skip {}: {e}", path.display()));
                continue;
            }
        };
        let rel = path
            .strip_prefix(&p.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        files += 1;
        for (offset, body) in chunk_text(&text, chunk_chars) {
            tx.execute(
                "INSERT INTO documents (path, offset_chars, body) VALUES (?1, ?2, ?3)",
                params![rel, offset as i64, body],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO documents_fts (rowid, body, path, offset_chars) VALUES (?1, ?2, ?3, ?4)",
                params![id, body, rel, offset as i64],
            )?;
            chunks += 1;
        }
    }
    tx.commit()?;

    let fp = fingerprint(&p.root, cfg);
    fs::write(stamp_path(&p.root), &fp)?;
    Ok((files, chunks))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else {
            continue;
        };
        if meta.is_dir() {
            collect_files(&path, out);
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|x| x.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if EXT.contains(&ext.as_str()) {
            out.push(path);
        }
    }
}

/// Re-index when the stamp is missing or docs/config changed.
pub fn ensure_index(p: &Project, cfg: &RagCfg) -> anyhow::Result<()> {
    if !cfg.enabled {
        return Ok(());
    }
    let fp = fingerprint(&p.root, cfg);
    let stamp = stamp_path(&p.root);
    let current = fs::read_to_string(&stamp).unwrap_or_default();
    if current.trim() == fp && db_path(&p.root).is_file() {
        return Ok(());
    }
    let (files, chunks) = index(p, cfg)?;
    crate::ui::ok(format!("indexed {files} document(s), {chunks} chunk(s)"));
    Ok(())
}

pub fn status(p: &Project) -> anyhow::Result<Status> {
    let db = db_path(&p.root);
    let mut s = Status {
        db_path: db.clone(),
        ..Default::default()
    };
    if !db.is_file() {
        return Ok(s);
    }
    let conn = Connection::open(&db)?;
    s.chunks = conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get::<_, i64>(0).map(|n| n as usize))?;
    s.files =
        conn.query_row("SELECT COUNT(DISTINCT path) FROM documents", [], |r| r.get::<_, i64>(0).map(|n| n as usize))?;
    Ok(s)
}

/// Escape a user query for FTS5 MATCH (quote tokens; drop empties).
fn fts_query(raw: &str) -> String {
    let tokens: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    if tokens.is_empty() {
        String::new()
    } else {
        tokens.join(" ")
    }
}

pub fn retrieve(p: &Project, cfg: &RagCfg, query: &str) -> anyhow::Result<Vec<Hit>> {
    if !cfg.enabled || query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let db = db_path(&p.root);
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let q = fts_query(query);
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let conn = Connection::open(&db)?;
    let mut stmt = conn.prepare(
        "SELECT path, body FROM documents_fts WHERE documents_fts MATCH ?1
         ORDER BY rank LIMIT ?2",
    )?;
    let top = cfg.top_k as i64;
    let rows = stmt.query_map(params![q, top], |r| {
        Ok(Hit {
            path: r.get(0)?,
            excerpt: r.get(1)?,
        })
    })?;
    let mut hits = Vec::new();
    for row in rows {
        hits.push(row?);
    }
    Ok(hits)
}

/// Format hits as a short system/context block for chat.
pub fn context_block(hits: &[Hit], max_chars: usize) -> Option<String> {
    if hits.is_empty() {
        return None;
    }
    let mut out = String::from("Relevant project documents:\n");
    for h in hits {
        let excerpt = if h.excerpt.chars().count() > 800 {
            format!("{}…", h.excerpt.chars().take(800).collect::<String>())
        } else {
            h.excerpt.clone()
        };
        let piece = format!("\n--- {}\n{}\n", h.path, excerpt);
        if out.chars().count() + piece.chars().count() > max_chars {
            break;
        }
        out.push_str(&piece);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::sample;
    use tempfile::tempdir;

    #[test]
    fn chunks_overlap() {
        let text: String = (0..500).map(|i| format!("{i:03} ")).collect();
        let parts = chunk_text(&text, 200);
        assert!(parts.len() >= 3);
        assert_eq!(parts[0].0, 0);
        assert!(parts[1].0 < 200);
    }

    #[test]
    fn index_and_retrieve_unique_term() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("data/docs")).unwrap();
        fs::write(root.join("data/docs/alpha.md"), "alpha has the keyword xylophone once").unwrap();
        fs::write(root.join("data/docs/beta.md"), "beta talks about pianos and drums").unwrap();
        let p = Project {
            root: root.to_path_buf(),
        };
        let mut cfg = sample().rag;
        cfg.enabled = true;
        cfg.docs_dir = "data/docs".into();
        cfg.top_k = 4;
        cfg.chunk_chars = 400;
        let (files, chunks) = index(&p, &cfg).unwrap();
        assert_eq!(files, 2);
        assert!(chunks >= 2);
        let hits = retrieve(&p, &cfg, "xylophone").unwrap();
        assert!(!hits.is_empty());
        assert!(hits[0].path.contains("alpha.md"));
        let st = status(&p).unwrap();
        assert_eq!(st.files, 2);
    }
}
