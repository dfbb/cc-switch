// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/cache-control-normalize.mjs
// 翻译: 2026-05-20
//
// Strips scattered cache_control markers from user messages and applies
// canonical placement at the last block of the last user message. This
// stabilizes the cache breakpoint by ensuring cache_control is always at
// a predictable position.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::Value;

pub struct CacheControlNormalize;

impl CacheControlNormalize {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for CacheControlNormalize {
    fn name(&self) -> &str {
        "cache-control-normalize"
    }
    fn order(&self) -> u32 {
        400
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for CacheControlNormalize {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let messages = match ctx.body.get("messages").and_then(|v| v.as_array()) {
            Some(m) => m,
            None => return Ok(None),
        };
        if messages.is_empty() {
            return Ok(None);
        }

        // Count cache_control markers in user messages.
        let marker_count = count_user_cache_control_markers(messages);
        if marker_count == 0 {
            return Ok(None);
        }

        let messages = ctx.body["messages"].as_array_mut().unwrap();

        // Strip all cache_control from all user message blocks.
        for msg in messages.iter_mut() {
            if msg.get("role").and_then(|v| v.as_str()) == Some("user") {
                strip_cache_control_markers(msg);
            }
        }

        // Apply canonical cache_control at the last block of the last user msg.
        for i in (0..messages.len()).rev() {
            let msg = &mut messages[i];
            if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
                continue;
            }
            let content = match msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                Some(c) => c,
                None => continue,
            };
            if content.is_empty() {
                continue;
            }
            let last_idx = content.len() - 1;
            let last_block = &mut content[last_idx];
            if last_block.is_object() {
                last_block["cache_control"] =
                    serde_json::json!({"type": "ephemeral"});
            }
            break;
        }

        Ok(None)
    }
}

/// Count total cache_control markers across all user message blocks.
fn count_user_cache_control_markers(messages: &[Value]) -> usize {
    let mut n = 0;
    for msg in messages {
        if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let content = match msg.get("content").and_then(|v| v.as_array()) {
            Some(c) => c,
            None => continue,
        };
        for block in content {
            if block.is_object() && block.get("cache_control").is_some() {
                n += 1;
            }
        }
    }
    n
}

/// Remove cache_control from all blocks in a user message (mutates in place).
fn strip_cache_control_markers(msg: &mut Value) {
    let content = match msg.get_mut("content").and_then(|v| v.as_array_mut()) {
        Some(c) => c,
        None => return,
    };
    for block in content.iter_mut() {
        if let Some(obj) = block.as_object_mut() {
            obj.remove("cache_control");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn count_markers_counts_correctly() {
        let messages = vec![
            json!({"role": "user", "content": [
                {"type": "text", "text": "a", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "b"}
            ]}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "c", "cache_control": {"type": "ephemeral"}}
            ]}),
            json!({"role": "assistant", "content": [{"type": "text", "text": "d"}]}),
        ];
        assert_eq!(count_user_cache_control_markers(&messages), 2);
    }

    #[test]
    fn strip_markers_removes_cache_control() {
        let mut msg = json!({"role": "user", "content": [
            {"type": "text", "text": "a", "cache_control": {"type": "ephemeral"}},
            {"type": "text", "text": "b"}
        ]});
        strip_cache_control_markers(&mut msg);
        let content = msg["content"].as_array().unwrap();
        assert!(content[0].get("cache_control").is_none());
        assert!(content[1].get("cache_control").is_none());
    }

    #[test]
    fn full_normalize_strips_and_applies_canonical() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "a", "cache_control": {"type": "ephemeral"}},
                    {"type": "text", "text": "b"}
                ]},
                {"role": "assistant", "content": [{"type": "text", "text": "reply"}]},
                {"role": "user", "content": [
                    {"type": "text", "text": "c"},
                    {"type": "text", "text": "d", "cache_control": {"type": "ephemeral"}}
                ]}
            ]
        });
        let mut ctx = RequestContext {
            body,
            headers: axum::http::HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let ext = CacheControlNormalize::new();
        ext.on_request(&mut ctx).unwrap();

        // No cache_control should remain on earlier blocks.
        let messages = ctx.body["messages"].as_array().unwrap();
        let msg0 = &messages[0]["content"].as_array().unwrap();
        assert!(msg0[0].get("cache_control").is_none());
        assert!(msg0[1].get("cache_control").is_none());
        let msg2 = &messages[2]["content"].as_array().unwrap();
        assert!(msg2[0].get("cache_control").is_none());

        // Last block of last user message should have canonical cache_control.
        let last_block = &msg2[1];
        assert_eq!(
            last_block["cache_control"]["type"].as_str().unwrap(),
            "ephemeral"
        );
    }

    #[test]
    fn no_markers_returns_early() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hello"}]}
            ]
        });
        let mut ctx = RequestContext {
            body: body.clone(),
            headers: axum::http::HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let ext = CacheControlNormalize::new();
        ext.on_request(&mut ctx).unwrap();
        // Body should be unchanged.
        assert_eq!(ctx.body, body);
    }
}
