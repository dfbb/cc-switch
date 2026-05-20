// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/content-strip.mjs
// 翻译: 2026-05-20
//
// Strips "Continue from where you left off" trailers and bookkeeping
// system-reminders from user messages. These volatile UI conveniences
// break prompt cache prefix stability across sessions.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use regex::Regex;
use serde_json::Value;

const CONTINUE_TRAILER_TEXT: &str = "Continue from where you left off.";

const REMINDER_WRAP_REGEX: &str =
    r"^<system-reminder>\n([\s\S]*?)\n</system-reminder>\s*$";

/// Bookkeeping patterns to match inside a system-reminder wrapper.
const BOOKKEEPING_PATTERNS: [&str; 7] = [
    r"^Token usage: \d+/\d+; \d+ remaining\s*$",
    r"^Output tokens \u{2014} turn: [^\n]+ \u{00b7} session: [^\n]+\s*$",
    r"^USD budget: \$[\d.]+\/\$[\d.]+; \$[\d.]+ remaining\s*$",
    r"^The task tools haven't been used recently\.",
    r"^The TodoWrite tool hasn't been used recently\.",
    r"^Remaining conversation turns: ",
    r"^Messages? until auto-compact: ",
];

pub struct ContentStrip;

impl ContentStrip {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for ContentStrip {
    fn name(&self) -> &str {
        "content-strip"
    }
    fn order(&self) -> u32 {
        330
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for ContentStrip {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let messages = match ctx.body.get_mut("messages") {
            Some(Value::Array(arr)) => arr,
            _ => return Ok(None),
        };

        let mut trailer_count = 0u64;
        let mut reminder_count = 0u64;

        let wrap_re = Regex::new(REMINDER_WRAP_REGEX).unwrap();
        let bookkeeping_res: Vec<Regex> = BOOKKEEPING_PATTERNS
            .iter()
            .map(|p| Regex::new(p).unwrap())
            .collect();

        for msg in messages.iter_mut() {
            if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
                continue;
            }
            let content = match msg.get_mut("content") {
                Some(Value::Array(arr)) => arr,
                _ => continue,
            };

            let original_len = content.len();
            let mut msg_trailers = 0u64;
            let mut msg_reminders = 0u64;

            let kept: Vec<Value> = content
                .drain(..)
                .filter(|block| {
                    // Strip "Continue from where you left off." text blocks
                    if is_continue_trailer_block(block) {
                        msg_trailers += 1;
                        return false;
                    }
                    // Strip bookkeeping system-reminders
                    if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                            if is_bookkeeping_reminder(text, &wrap_re, &bookkeeping_res) {
                                msg_reminders += 1;
                                return false;
                            }
                        }
                    }
                    true
                })
                .collect();

            // Skip if nothing was removed or message would become empty
            if kept.is_empty() || kept.len() == original_len {
                // Nothing removed, restore original
                *content = kept;
                continue;
            }

            trailer_count += msg_trailers;
            reminder_count += msg_reminders;
            *content = kept;
        }

        let total = trailer_count + reminder_count;
        if total > 0 {
            ctx.meta.set(
                "contentStripStats",
                serde_json::json!({
                    "trailerCount": trailer_count,
                    "reminderCount": reminder_count
                }),
            );
        }

        Ok(None)
    }
}

// --- Helper functions ---

fn is_continue_trailer_block(block: &Value) -> bool {
    block.get("type").and_then(|v| v.as_str()) == Some("text")
        && block.get("text").and_then(|v| v.as_str()) == Some(CONTINUE_TRAILER_TEXT)
}

fn is_bookkeeping_reminder(text: &str, wrap_re: &Regex, patterns: &[Regex]) -> bool {
    let m = match wrap_re.captures(text) {
        Some(caps) => caps,
        None => return false,
    };
    let inner = match m.get(1) {
        Some(g) => g.as_str(),
        None => return false,
    };
    for rx in patterns {
        if rx.is_match(inner) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_wrap_re() -> Regex {
        Regex::new(REMINDER_WRAP_REGEX).unwrap()
    }

    fn make_patterns() -> Vec<Regex> {
        BOOKKEEPING_PATTERNS
            .iter()
            .map(|p| Regex::new(p).unwrap())
            .collect()
    }

    #[test]
    fn test_is_continue_trailer() {
        let block = serde_json::json!({"type": "text", "text": "Continue from where you left off."});
        assert!(is_continue_trailer_block(&block));

        let not_trailer = serde_json::json!({"type": "text", "text": "hello"});
        assert!(!is_continue_trailer_block(&not_trailer));
    }

    #[test]
    fn test_is_bookkeeping_reminder_token_usage() {
        let wrap_re = make_wrap_re();
        let patterns = make_patterns();
        let text = "<system-reminder>\nToken usage: 100/200; 100 remaining\n</system-reminder>";
        assert!(is_bookkeeping_reminder(text, &wrap_re, &patterns));
    }

    #[test]
    fn test_is_bookkeeping_reminder_budget() {
        let wrap_re = make_wrap_re();
        let patterns = make_patterns();
        let text =
            "<system-reminder>\nUSD budget: $5.00/$10.00; $5.00 remaining\n</system-reminder>";
        assert!(is_bookkeeping_reminder(text, &wrap_re, &patterns));
    }

    #[test]
    fn test_is_bookkeeping_reminder_task_tools() {
        let wrap_re = make_wrap_re();
        let patterns = make_patterns();
        let text =
            "<system-reminder>\nThe task tools haven't been used recently.\n</system-reminder>";
        assert!(is_bookkeeping_reminder(text, &wrap_re, &patterns));
    }

    #[test]
    fn test_not_bookkeeping_reminder() {
        let wrap_re = make_wrap_re();
        let patterns = make_patterns();
        let text = "<system-reminder>\nReal system reminder content\n</system-reminder>";
        assert!(!is_bookkeeping_reminder(text, &wrap_re, &patterns));
    }

    #[test]
    fn test_strips_continue_trailer() {
        let ext = ContentStrip::new();
        let body = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "real message"},
                        {"type": "text", "text": "Continue from where you left off."}
                    ]
                }
            ]
        });
        let mut ctx = RequestContext {
            body,
            headers: axum::http::HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        ext.on_request(&mut ctx).unwrap();

        let content = ctx.body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["text"].as_str().unwrap(), "real message");

        let stats = ctx.meta.get("contentStripStats").unwrap();
        assert_eq!(stats["trailerCount"], 1);
    }
}
