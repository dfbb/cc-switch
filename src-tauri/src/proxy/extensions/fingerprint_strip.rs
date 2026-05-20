// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/fingerprint-strip.mjs
// 翻译: 2026-05-20
//
// Stabilizes cc_version fingerprint in system prompt. The 4th dot-segment of
// cc_version is a hash of the first user message text. When the first message
// changes (e.g. different project context), the fingerprint changes even though
// the CC binary version is the same — breaking prompt cache prefix consistency.
//
// This extension recomputes the fingerprint from the real user message text
// and verifies against the old value before replacing.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};

const FINGERPRINT_SALT: &str = "59cf53e54c78";
const FINGERPRINT_INDICES: [usize; 3] = [4, 7, 20];

pub struct FingerprintStrip;

impl FingerprintStrip {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for FingerprintStrip {
    fn name(&self) -> &str {
        "fingerprint-strip"
    }
    fn order(&self) -> u32 {
        100
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for FingerprintStrip {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let body = &ctx.body;
        let system = match body.get("system") {
            Some(s) => s,
            None => return Ok(None),
        };
        let messages = match body.get("messages") {
            Some(m) => m,
            None => return Ok(None),
        };

        if let Some(result) = stabilize_fingerprint(system, messages) {
            // Clone the block and replace its text.
            let mut new_block = system[result.attr_idx].clone();
            new_block["text"] = Value::String(result.new_text);
            ctx.body["system"][result.attr_idx] = new_block;
        }

        Ok(None)
    }
}

struct StabilizeResult {
    attr_idx: usize,
    new_text: String,
}

/// Compute fingerprint: SHA256(salt + chars[4] + chars[7] + chars[20] + version),
/// take first 3 hex chars.
fn compute_fingerprint(message_text: &str, version: &str) -> String {
    let chars: String = FINGERPRINT_INDICES
        .iter()
        .map(|&i| {
            message_text
                .chars()
                .nth(i)
                .unwrap_or('0')
                .to_string()
        })
        .collect();

    let input = format!("{}{}{}", FINGERPRINT_SALT, chars, version);
    let hash = Sha256::digest(input.as_bytes());
    // JS: digest("hex").slice(0, 3) — first 3 hex chars, not 3 bytes.
    let full_hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    full_hex.chars().take(3).collect()
}

/// Extract the first real user message text (skip <system-reminder> messages).
fn extract_real_user_message_text(messages: &Value) -> String {
    let arr = match messages.as_array() {
        Some(a) => a,
        None => return String::new(),
    };
    for msg in arr {
        if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let content = match msg.get("content") {
            Some(c) => c,
            None => continue,
        };
        if let Some(content_arr) = content.as_array() {
            for block in content_arr {
                if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        if !text.starts_with("<system-reminder>") {
                            return text.to_string();
                        }
                    }
                }
            }
        } else if let Some(text) = content.as_str() {
            if !text.starts_with("<system-reminder>") {
                return text.to_string();
            }
        }
    }
    String::new()
}

/// Extract text from the first message (legacy fallback).
fn extract_first_message_text(messages: &Value) -> String {
    let arr = match messages.as_array() {
        Some(a) => a,
        None => return String::new(),
    };
    let first = match arr.first() {
        Some(m) => m,
        None => return String::new(),
    };
    if first.get("role").and_then(|v| v.as_str()) != Some("user") {
        return String::new();
    }
    let content = match first.get("content") {
        Some(c) => c,
        None => return String::new(),
    };
    if let Some(text) = content.as_str() {
        return text.to_string();
    }
    if let Some(arr) = content.as_array() {
        for block in arr {
            if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    return text.to_string();
                }
            }
        }
    }
    String::new()
}

/// Find the system block with x-anthropic-billing-header, parse cc_version,
/// recompute fingerprint from real user message, verify, and stabilize.
fn stabilize_fingerprint(system: &Value, messages: &Value) -> Option<StabilizeResult> {
    let blocks = system.as_array()?;

    // Find the attribution block containing x-anthropic-billing-header and cc_version.
    let attr_idx = blocks.iter().position(|b| {
        b.get("type").and_then(|v| v.as_str()) == Some("text")
            && b.get("text")
                .and_then(|v| v.as_str())
                .map(|t| t.contains("x-anthropic-billing-header:"))
                .unwrap_or(false)
    })?;

    let attr_block = &blocks[attr_idx];
    let attr_text = attr_block.get("text").and_then(|v| v.as_str())?;

    // Extract cc_version=... from the text.
    let re = Regex::new(r"cc_version=([^;]+)").ok()?;
    let caps = re.captures(attr_text)?;
    let full_version = caps.get(1)?.as_str();

    let dot_parts: Vec<&str> = full_version.split('.').collect();
    if dot_parts.len() < 4 {
        return None;
    }

    let base_version = dot_parts[..3].join(".");
    let old_fingerprint = dot_parts[3];

    let real_text = extract_real_user_message_text(messages);
    let real_verification = compute_fingerprint(&real_text, &base_version);
    let legacy_text = extract_first_message_text(messages);
    let legacy_verification = compute_fingerprint(&legacy_text, &base_version);

    // Verify that the old fingerprint matches either the real or legacy computation.
    let verification_passed =
        real_verification == old_fingerprint || legacy_verification == old_fingerprint;

    if !verification_passed {
        return None;
    }

    let stable_fingerprint = compute_fingerprint(&real_text, &base_version);
    if stable_fingerprint == old_fingerprint {
        return None; // Already stable.
    }

    let new_version = format!("{}.{}", base_version, stable_fingerprint);
    let new_text = attr_text.replace(
        &format!("cc_version={}", full_version),
        &format!("cc_version={}", new_version),
    );

    Some(StabilizeResult {
        attr_idx,
        new_text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_fingerprint_is_deterministic() {
        let a = compute_fingerprint("hello world test", "1.2.3");
        let b = compute_fingerprint("hello world test", "1.2.3");
        assert_eq!(a, b);
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn compute_fingerprint_differs_for_different_input() {
        let a = compute_fingerprint("hello", "1.2.3");
        let b = compute_fingerprint("world", "1.2.3");
        assert_ne!(a, b);
    }

    #[test]
    fn extract_real_user_message_skips_reminders() {
        let messages = serde_json::json!([
            {"role": "user", "content": [{"type": "text", "text": "<system-reminder>"}]},
            {"role": "user", "content": [{"type": "text", "text": "real message"}]}
        ]);
        assert_eq!(extract_real_user_message_text(&messages), "real message");
    }

    #[test]
    fn extract_real_user_message_from_string_content() {
        let messages = serde_json::json!([
            {"role": "user", "content": "direct string message"}
        ]);
        assert_eq!(
            extract_real_user_message_text(&messages),
            "direct string message"
        );
    }

    #[test]
    fn stabilize_returns_none_when_no_attribution_block() {
        let system = serde_json::json!([
            {"type": "text", "text": "just a system prompt"}
        ]);
        let messages = serde_json::json!([{"role": "user", "content": "hello"}]);
        assert!(stabilize_fingerprint(&system, &messages).is_none());
    }

    #[test]
    fn stabilize_returns_none_when_verification_fails() {
        // cc_version fingerprint doesn't match any computation since
        // the message text is different from what was used to generate "xxx".
        let system = serde_json::json!([
            {"type": "text", "text": "x-anthropic-billing-header: cc_version=1.0.0.xxx"}
        ]);
        let messages = serde_json::json!([{"role": "user", "content": "hello world testing"}]);
        assert!(stabilize_fingerprint(&system, &messages).is_none());
    }
}
