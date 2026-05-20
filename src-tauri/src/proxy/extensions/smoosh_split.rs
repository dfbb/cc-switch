// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/smoosh-split.mjs
// 翻译: 2026-05-20
//
// Peels smooshed system-reminders from tool_result.content into standalone
// text blocks. When a tool_result's content string ends with one or more
// `<system-reminder>...</system-reminder>` blocks (smooshed onto the output
// by the upstream API), this extension extracts them as separate text blocks
// after the tool_result block.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::Value;

pub struct SmooshSplit;

impl SmooshSplit {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for SmooshSplit {
    fn name(&self) -> &str {
        "smoosh-split"
    }
    fn order(&self) -> u32 {
        320
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for SmooshSplit {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let messages = match ctx.body.get_mut("messages") {
            Some(Value::Array(arr)) => arr,
            _ => return Ok(None),
        };

        let mut total_peeled = 0u64;

        for msg in messages.iter_mut() {
            if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
                continue;
            }
            let content = match msg.get_mut("content") {
                Some(Value::Array(arr)) => arr,
                _ => continue,
            };

            let mut out: Vec<Value> = Vec::new();
            let mut peeled_reminders: Vec<Value> = Vec::new();
            let mut mutated = false;

            for block in content.drain(..) {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                    let block_content = match block.get("content").and_then(|v| v.as_str()) {
                        Some(s) => s.to_string(),
                        None => {
                            out.push(block);
                            continue;
                        }
                    };

                    let (stripped, reminders) = peel_trailing_reminders(&block_content);
                    if !reminders.is_empty() {
                        let mut new_block = block.clone();
                        new_block["content"] = Value::String(stripped);
                        out.push(new_block);
                        for r in &reminders {
                            peeled_reminders.push(serde_json::json!({
                                "type": "text",
                                "text": r
                            }));
                        }
                        total_peeled += reminders.len() as u64;
                        mutated = true;
                        continue;
                    }
                }
                out.push(block);
            }

            if mutated {
                out.append(&mut peeled_reminders);
            }
            *content = out;
        }

        if total_peeled > 0 {
            ctx.meta
                .set("smooshSplitStats", serde_json::json!({"peeled": total_peeled}));
        }

        Ok(None)
    }
}

// --- Helper functions ---

const END_TAG: &str = "</system-reminder>";
const START_TAG: &str = "<system-reminder>";

/// Peel trailing `\n\n<system-reminder>...</system-reminder>` blocks from
/// the end of a tool_result content string. Returns the stripped content
/// and the extracted reminder blocks (in original order, outermost first).
///
/// Uses string matching instead of regex to avoid lookahead (unsupported
/// by Rust's regex crate). The JS equivalent uses:
///   `/\n\n(<system-reminder>\n(?:(?!<\/system-reminder>)[\s\S])*?\n<\/system-reminder>)\s*$/`
fn peel_trailing_reminders(content: &str) -> (String, Vec<String>) {
    let mut s = content.to_string();
    let mut reminders: Vec<String> = Vec::new();

    loop {
        // Find the last </system-reminder> in the string
        let end_pos = match s.rfind(END_TAG) {
            Some(pos) => pos,
            None => break,
        };
        let after_end = end_pos + END_TAG.len();

        // Only trailing whitespace is allowed after </system-reminder>
        if !s[after_end..].trim().is_empty() {
            break;
        }

        // Find matching <system-reminder> — it must be preceded by \n\n
        let before_end = &s[..end_pos];
        // Look backwards for the opening tag
        let start_pos = match before_end.rfind(START_TAG) {
            Some(pos) => pos,
            None => break,
        };

        // Check that \n\n precedes the start tag
        if start_pos < 2 || &s[start_pos - 2..start_pos] != "\n\n" {
            break;
        }

        // Extract the reminder (from the \n\n to end of string, trimmed)
        let reminder = s[start_pos - 2..].trim_end().to_string();
        reminders.insert(0, reminder);

        // Strip from the \n\n onwards
        s = s[..start_pos - 2].to_string();
    }

    (s, reminders)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peel_trailing_single_reminder() {
        let input = "some tool output\n\n<system-reminder>\nbookkeeping info\n</system-reminder>";
        let (stripped, reminders) = peel_trailing_reminders(input);
        assert_eq!(stripped, "some tool output");
        assert_eq!(reminders.len(), 1);
        assert!(reminders[0].contains("bookkeeping info"));
    }

    #[test]
    fn test_no_match_without_double_newline() {
        let input = "some tool output<system-reminder>\nbookkeeping info\n</system-reminder>";
        let (stripped, reminders) = peel_trailing_reminders(input);
        assert_eq!(reminders.len(), 0);
        assert_eq!(stripped, input);
    }

    #[test]
    fn test_no_match_if_not_at_end() {
        let input =
            "some output\n\n<system-reminder>\ninfo\n</system-reminder>\nmore output";
        let (stripped, reminders) = peel_trailing_reminders(input);
        // Should NOT peel because there's "more output" after </system-reminder>
        assert_eq!(reminders.len(), 0);
    }

    #[test]
    fn test_peel_multiple_reminders() {
        let input = "tool output\n\n<system-reminder>\nreminder 1\n</system-reminder>\n\n<system-reminder>\nreminder 2\n</system-reminder>";
        let (stripped, reminders) = peel_trailing_reminders(input);
        assert_eq!(stripped, "tool output");
        assert_eq!(reminders.len(), 2);
        assert!(reminders[0].contains("reminder 1"));
        assert!(reminders[1].contains("reminder 2"));
    }

    #[test]
    fn test_peel_single_reminder_request() {
        let ext = SmooshSplit::new();
        let body = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "content": "tool output\n\n<system-reminder>\nbookkeeping info\n</system-reminder>"
                        }
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
        // Should now have 2 blocks: tool_result + text
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[1]["type"], "text");
        assert!(content[1]["text"]
            .as_str()
            .unwrap()
            .contains("bookkeeping info"));

        // Check stats
        let stats = ctx.meta.get("smooshSplitStats").unwrap();
        assert_eq!(stats["peeled"], 1);
    }

    #[test]
    fn test_no_reminder_no_change() {
        let ext = SmooshSplit::new();
        let body = serde_json::json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "content": "plain tool output without reminders"
                        }
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
        // Content should be unchanged (1 block, tool_result)
        let content = ctx.body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(
            content[0]["content"].as_str().unwrap(),
            "plain tool output without reminders"
        );
    }
}
