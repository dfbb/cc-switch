// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/ttl-tier-detect.mjs
// 翻译: 2026-05-20
//
// Pure detection extension. Traverses request body cache_control markers
// (system blocks + message content blocks) looking for ttl="5m".
// Sets ctx.meta._ttlTier so downstream extensions (ttl-management at order 500)
// can act on the detected tier even after cache_control normalization.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::Value;

pub struct TtlTierDetect;

impl TtlTierDetect {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for TtlTierDetect {
    fn name(&self) -> &str {
        "ttl-tier-detect"
    }
    fn order(&self) -> u32 {
        75
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for TtlTierDetect {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let tier = detect_existing_tier(&ctx.body);
        ctx.meta.set("_ttlTier", Value::String(tier));
        Ok(None)
    }
}

/// Traverse body.system (array of blocks) and body.messages[*].content (arrays of blocks).
/// Return "5m" if any block has cache_control.ttl === "5m", otherwise "1h".
fn detect_existing_tier(body: &Value) -> String {
    // Check system blocks.
    if let Some(system) = body.get("system").and_then(|v| v.as_array()) {
        for block in system {
            if has_5m_ttl(block) {
                return "5m".to_string();
            }
        }
    }
    // Check message content blocks.
    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
                for block in content {
                    if has_5m_ttl(block) {
                        return "5m".to_string();
                    }
                }
            }
        }
    }
    "1h".to_string()
}

fn has_5m_ttl(block: &Value) -> bool {
    block
        .get("cache_control")
        .and_then(|cc| cc.get("ttl"))
        .and_then(|v| v.as_str())
        == Some("5m")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use serde_json::json;

    #[test]
    fn detects_5m_in_system_block() {
        let body = json!({
            "system": [
                {"type": "text", "text": "hello", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
            ],
            "messages": []
        });
        assert_eq!(detect_existing_tier(&body), "5m");
    }

    #[test]
    fn detects_5m_in_message_content() {
        let body = json!({
            "system": [],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "hi", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
                ]}
            ]
        });
        assert_eq!(detect_existing_tier(&body), "5m");
    }

    #[test]
    fn defaults_to_1h_when_no_cache_control() {
        let body = json!({
            "system": [{"type": "text", "text": "hello"}],
            "messages": []
        });
        assert_eq!(detect_existing_tier(&body), "1h");
    }

    #[test]
    fn defaults_to_1h_when_no_5m() {
        let body = json!({
            "system": [
                {"type": "text", "text": "hello", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": []
        });
        assert_eq!(detect_existing_tier(&body), "1h");
    }

    #[test]
    fn sets_meta_on_request() {
        let ext = TtlTierDetect::new();
        let mut ctx = RequestContext {
            body: json!({
                "system": [
                    {"type": "text", "text": "x", "cache_control": {"type": "ephemeral", "ttl": "5m"}}
                ],
                "messages": []
            }),
            headers: HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let result = ext.on_request(&mut ctx).unwrap();
        assert!(result.is_none());
        assert_eq!(ctx.meta.get("_ttlTier").and_then(|v| v.as_str()), Some("5m"));
    }
}
