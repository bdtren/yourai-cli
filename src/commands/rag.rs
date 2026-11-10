use crate::core::project::Project;
use crate::core::rag;
use crate::error::{fail_hint, Exit};
use crate::ui;

pub fn index(p: &Project) -> anyhow::Result<()> {
    p.upgrade_config()?;
    let cfg = p.config()?;
    if !cfg.rag.enabled {
        return fail_hint(
            Exit::InvalidConfig,
            "document search is off",
            "run `yourai config set rag.enabled true`, then `yourai rag index`",
        );
    }
    ui::heading("Document index");
    let (files, chunks) = rag::index(p, &cfg.rag)?;
    ui::ok(format!("indexed {files} file(s), {chunks} chunk(s) into .yourai/rag.sqlite"));
    ui::next("yourai chat");
    Ok(())
}

pub fn status(p: &Project) -> anyhow::Result<()> {
    p.upgrade_config()?;
    let cfg = p.config()?;
    let st = rag::status(p)?;
    ui::heading("Document search");
    println!("  enabled:  {}", cfg.rag.enabled);
    println!("  docs_dir: {}", cfg.rag.docs_dir);
    println!("  top_k:    {}", cfg.rag.top_k);
    println!("  db:       {}", st.db_path.display());
    println!("  files:    {}", st.files);
    println!("  chunks:   {}", st.chunks);
    if cfg.rag.enabled && st.chunks == 0 {
        ui::next("yourai rag index");
    }
    Ok(())
}
