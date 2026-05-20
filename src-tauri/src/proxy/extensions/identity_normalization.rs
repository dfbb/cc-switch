// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/identity-normalization.mjs
// 翻译: 2026-05-20
//
// Normalizes volatile identity fields for cache stability:
// - SessionStart:resume -> SessionStart:startup
// - Strips <session-id> tags
// - Removes "Last active:" lines
// - Strips session_knowledge from system blocks
// - Pins system-reminder blocks in body.system

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};

const CONTINUE_TRAILER_TEXT: &str = "Continue from where you left off.";

/// Patterns for bookkeeping reminders (identity_normalization uses a subset of 3).
const BOOKKEEPING_PATTERNS: [&str; 3] = [
    r"^Token usage: \d+/\d+; \d+ remaining\s*$",
    r"^Output tokens \u{2014} turn: [^\n]+ \u{00b7} session: [^\n]+\s*$",
    r"^USD budget: \$[\d.]+\/\$[\d.]+; \$[\d.]+ remaining\s*$",
];

pub struct IdentityNormalization;

impl IdentityNormalization {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for IdentityNormalization {
    fn name(&self) -> &str {
        "identity-normalization"
    }
    fn order(&self) -> u32 {
        300
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for IdentityNormalization {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        // Process body.system: strip session_knowledge and pin system-reminder blocks
        if let Some(system) = ctx.body.get_mut("system") {
            if let Some(blocks) = system.as_array_mut() {
                for block in blocks.iter_mut() {
                    if block.get("type").and_then(|v| v.as_str()) != Some("text") {
                        continue;
                    }
                    let mut text = match block.get("text").and_then(|v| v.as_str()) {
                        Some(t) => t.to_string(),
                        None => continue,
                    };

                    let original = text.clone();

                    if text.contains("session_knowledge") {
                        text = strip_session_knowledge(&text);
                    }
                    if text.contains("<system-reminder>") {
                        text = pin_block_content(&text);
                    }
                    if text != original {
                        block["text"] = Value::String(text);
                    }
                }
            }
        }

        // Process body.messages: normalize SessionStart text
        if let Some(messages) = ctx.body.get_mut("messages") {
            if let Some(msgs) = messages.as_array_mut() {
                for msg in msgs.iter_mut() {
                    let content = match msg.get_mut("content") {
                        Some(Value::Array(arr)) => arr,
                        _ => continue,
                    };

                    for block in content.iter_mut() {
                        if block.get("type").and_then(|v| v.as_str()) != Some("text") {
                            continue;
                        }
                        let block_text = match block.get("text").and_then(|v| v.as_str()) {
                            Some(t) => t.to_string(),
                            None => continue,
                        };

                        if let Some(normalized) = normalize_session_start_text(&block_text) {
                            if normalized != block_text {
                                block["text"] = Value::String(normalized);
                            }
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

// --- Helper functions ---

fn strip_session_knowledge(text: &str) -> String {
    let re = Regex::new(r"\n<session_knowledge[^>]*>[\s\S]*?</session_knowledge>").unwrap();
    re.replace_all(text, "").to_string()
}

/// Normalize trailing whitespace before `</system-reminder>`.
fn normalize_reminder_trailing(text: &str) -> String {
    let re = Regex::new(r"\s+(</system-reminder>)\s*$").unwrap();
    re.replace(text, "\n$1").to_string()
}

fn content_hash(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    hash.iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
        .chars()
        .take(16)
        .collect()
}

fn pin_block_content(text: &str) -> String {
    // Without module-level cache, just normalize trailing whitespace.
    // The pinning cache in the JS is a cross-request optimization.
    normalize_reminder_trailing(text)
}

fn normalize_session_start_text(text: &str) -> Option<String> {
    if !text.contains("SessionStart:") {
        return None;
    }

    let mut out = text.to_string();

    // SessionStart:resume -> SessionStart:startup
    let resume_re = Regex::new(r"SessionStart:resume hook success:").unwrap();
    if resume_re.is_match(&out) {
        out = resume_re
            .replace(&out, "SessionStart:startup hook success:")
            .to_string();
    }

    // Strip <session-id> tags
    let session_id_re = Regex::new(r"\n?<session-id>[^<]*</session-id>").unwrap();
    if session_id_re.is_match(&out) {
        out = session_id_re.replace_all(&out, "").to_string();
    }

    // Remove "Last active:" lines
    let last_active_re = Regex::new(r"\nLast active:[^\n]*").unwrap();
    if last_active_re.is_match(&out) {
        out = last_active_re.replace_all(&out, "").to_string();
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_resume_to_startup() {
        let input = "SessionStart:resume hook success: 5 hooks loaded";
        let result = normalize_session_start_text(input).unwrap();
        assert!(result.contains("SessionStart:startup"));
        assert!(!result.contains("SessionStart:resume"));
    }

    #[test]
    fn test_strip_session_id() {
        let input = "SessionStart:startup hook success:\nsome text\n<session-id>abc123</session-id>\nmore text";
        let result = normalize_session_start_text(input).unwrap();
        assert!(!result.contains("session-id"));
        assert!(result.contains("some text"));
        assert!(result.contains("more text"));
    }

    #[test]
    fn test_strip_last_active() {
        let input = "SessionStart:startup hook success:\nLast active: 2024-01-01\nmore text";
        let result = normalize_session_start_text(input).unwrap();
        assert!(!result.contains("Last active"));
        assert!(result.contains("more text"));
    }

    #[test]
    fn test_no_session_start_returns_none() {
        assert!(normalize_session_start_text("plain text").is_none());
    }

    #[test]
    fn test_pin_block_content_normalizes_trailing() {
        let input = "some content   </system-reminder>  ";
        let result = pin_block_content(input);
        assert!(result.ends_with("</system-reminder>"));
        assert!(!result.contains("   </system-reminder>"));
    }
}
