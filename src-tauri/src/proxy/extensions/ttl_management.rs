// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/ttl-management.mjs
// 翻译: 2026-05-20
//
// Inject correct TTL on cache_control markers based on detected tier.
// Reads ctx.meta["_ttlTier"] (set by ttl-tier-detect at order 75).
// Distinguishes main thread vs subagent ("Claude Agent SDK" prompt).
// Env vars: CACHE_FIX_TTL_MAIN / CACHE_FIX_TTL_SUBAGENT (default "1h").

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::Value;

const AGENT_SDK_PREFIX: &str = "You are a Claude agent, built on Anthropic's Claude Agent SDK.";

pub struct TtlManagement;

impl TtlManagement {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for TtlManagement {
    fn name(&self) -> &str {
        "ttl-management"
    }
    fn order(&self) -> u32 {
        500
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for TtlManagement {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let body = &mut ctx.body;

        // Short-circuit: no system block → nothing to do.
        if body.get("system").is_none() {
            return Ok(None);
        }

        let request_type = detect_request_type(&body["system"]);
        let ttl_value = match request_type {
            RequestType::Subagent => {
                std::env::var("CACHE_FIX_TTL_SUBAGENT")
                    .ok()
                    .unwrap_or_else(|| "1h".to_string())
                    .to_lowercase()
            }
            RequestType::Main => std::env::var("CACHE_FIX_TTL_MAIN")
                .ok()
                .unwrap_or_else(|| "1h".to_string())
                .to_lowercase(),
        };

        if ttl_value == "none" {
            return Ok(None);
        }

        // Read detected tier from meta (set by ttl-tier-detect).
        let detected_tier = ctx
            .meta
            .get("_ttlTier")
            .and_then(|v| v.as_str())
            .unwrap_or("1h");
        let ttl_param = if ttl_value == "5m" || detected_tier == "5m" {
            "5m"
        } else {
            "1h"
        };

        // Process system blocks.
        if let Some(system) = body.get_mut("system").and_then(|v| v.as_array_mut()) {
            for block in system.iter_mut() {
                *block = inject_ttl(block.clone(), ttl_param);
            }
        }

        // Process message content blocks.
        if let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) {
            for msg in messages.iter_mut() {
                if let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                    for block in content.iter_mut() {
                        *block = inject_ttl(block.clone(), ttl_param);
                    }
                }
            }
        }

        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestType {
    Main,
    Subagent,
}

/// Detect whether this is a main thread or subagent request.
/// Subagent is identified by a system block whose text starts with the Claude Agent SDK prefix.
fn detect_request_type(system: &Value) -> RequestType {
    let blocks = match system.as_array() {
        Some(b) => b,
        None => return RequestType::Main,
    };

    let is_subagent = blocks.iter().any(|b| {
        b.get("type").and_then(|v| v.as_str()) == Some("text")
            && b.get("text")
                .and_then(|v| v.as_str())
                .map_or(false, |t| t.starts_with(AGENT_SDK_PREFIX))
    });

    if is_subagent {
        RequestType::Subagent
    } else {
        RequestType::Main
    }
}

/// Inject TTL on cache_control markers where type="ephemeral" and ttl is missing.
/// Returns a new Value (clone-on-write semantics matching JS behavior).
fn inject_ttl(mut block: Value, ttl_param: &str) -> Value {
    if let Some(cc) = block.get("cache_control") {
        if cc.get("type").and_then(|v| v.as_str()) == Some("ephemeral")
            && cc.get("ttl").is_none()
        {
            let mut new_cc = cc.clone();
            new_cc["ttl"] = Value::String(ttl_param.to_string());
            block["cache_control"] = new_cc;
        }
    }
    block
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use serde_json::json;

    #[test]
    fn detect_main_when_no_agent_sdk_prefix() {
        let system = json!([
            {"type": "text", "text": "You are a helpful assistant"}
        ]);
        assert_eq!(detect_request_type(&system), RequestType::Main);
    }

    #[test]
    fn detect_subagent_when_agent_sdk_prefix_present() {
        let system = json!([
            {"type": "text", "text": "You are a Claude agent, built on Anthropic's Claude Agent SDK. You are running in a subagent."}
        ]);
        assert_eq!(detect_request_type(&system), RequestType::Subagent);
    }

    #[test]
    fn detect_main_when_empty_system() {
        let system = json!([]);
        assert_eq!(detect_request_type(&system), RequestType::Main);
    }

    #[test]
    fn inject_ttl_adds_ttl_when_missing() {
        let block = json!({"type": "text", "text": "hello", "cache_control": {"type": "ephemeral"}});
        let result = inject_ttl(block, "1h");
        assert_eq!(result["cache_control"]["ttl"].as_str().unwrap(), "1h");
    }

    #[test]
    fn inject_ttl_preserves_existing_ttl() {
        let block = json!({"type": "text", "text": "hello", "cache_control": {"type": "ephemeral", "ttl": "5m"}});
        let result = inject_ttl(block, "1h");
        assert_eq!(result["cache_control"]["ttl"].as_str().unwrap(), "5m");
    }

    #[test]
    fn inject_ttl_skips_non_ephemeral() {
        let block = json!({"type": "text", "text": "hello", "cache_control": {"type": "finite"}});
        let expected = block.clone();
        let result = inject_ttl(block, "1h");
        assert_eq!(result, expected);
    }

    #[test]
    fn inject_ttl_skips_no_cache_control() {
        let block = json!({"type": "text", "text": "hello"});
        let expected = block.clone();
        let result = inject_ttl(block, "1h");
        assert_eq!(result, expected);
    }

    #[test]
    fn on_request_injects_ttl_in_system_and_messages() {
        let ext = TtlManagement::new();
        let mut ctx = RequestContext {
            body: json!({
                "system": [
                    {"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}
                ],
                "messages": [
                    {"role": "user", "content": [
                        {"type": "text", "text": "hi", "cache_control": {"type": "ephemeral"}}
                    ]}
                ]
            }),
            headers: HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        // Default tier is "1h", default env TTL_MAIN is "1h"
        let result = ext.on_request(&mut ctx).unwrap();
        assert!(result.is_none());

        // Verify system block got ttl injected.
        let sys_ttl = &ctx.body["system"][0]["cache_control"]["ttl"];
        assert_eq!(sys_ttl.as_str().unwrap(), "1h");

        // Verify message block got ttl injected.
        let msg_ttl = &ctx.body["messages"][0]["content"][0]["cache_control"]["ttl"];
        assert_eq!(msg_ttl.as_str().unwrap(), "1h");
    }

    #[test]
    fn on_request_uses_5m_when_meta_tier_is_5m() {
        let ext = TtlManagement::new();
        let mut ctx = RequestContext {
            body: json!({
                "system": [
                    {"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}
                ],
                "messages": []
            }),
            headers: HeaderMap::new(),
            meta: {
                let mut meta = ExtensionMeta::default();
                meta.set("_ttlTier", Value::String("5m".to_string()));
                meta
            },
        };
        let result = ext.on_request(&mut ctx).unwrap();
        assert!(result.is_none());

        let sys_ttl = &ctx.body["system"][0]["cache_control"]["ttl"];
        assert_eq!(sys_ttl.as_str().unwrap(), "5m");
    }

    #[test]
    fn on_request_skips_when_no_system() {
        let ext = TtlManagement::new();
        let mut ctx = RequestContext {
            body: json!({"messages": []}),
            headers: HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let result = ext.on_request(&mut ctx).unwrap();
        assert!(result.is_none());
    }
}
