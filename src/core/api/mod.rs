//! Chat API style adapters under `/v1/` (OpenAI, Claude, Gemini).

mod chat_upstream;
mod claude;
mod gemini;
pub mod handler;
mod openai_ws;
pub mod routes;

pub use claude::messages as claude_messages;
pub use gemini::generate as gemini_generate;
pub use openai_ws::chat_ws;

use serde_json::Value;

/// Pull plain text from Anthropic/OpenAI-ish content that may be a string or block list.
pub fn content_text(content: &Value) -> anyhow::Result<String> {
    match content {
        Value::String(s) => Ok(s.clone()),
        Value::Array(parts) => {
            let mut out = String::new();
            for p in parts {
                if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                    out.push_str(t);
                } else if p.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                        out.push_str(t);
                    }
                } else if p.get("type").is_some() {
                    anyhow::bail!("only text content is supported in this adapter");
                }
            }
            Ok(out)
        }
        _ => anyhow::bail!("content must be a string or an array of text parts"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn content_text_string_and_blocks() {
        assert_eq!(content_text(&json!("hi")).unwrap(), "hi");
        assert_eq!(
            content_text(&json!([{"type":"text","text":"a"},{"type":"text","text":"b"}])).unwrap(),
            "ab"
        );
        assert!(content_text(&json!([{"type":"image","source":{}}])).is_err());
    }
}
