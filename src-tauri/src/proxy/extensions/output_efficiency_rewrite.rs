// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/output-efficiency-rewrite.mjs
// 翻译: 2026-05-20
//
// Replaces the "# Output efficiency" section in system prompt text blocks
// with a custom replacement string. Gated by the env var
// CACHE_FIX_OUTPUT_EFFICIENCY_REPLACEMENT or ctx.meta.outputEfficiencyReplacement.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::Value;

const SECTION_HEADER: &str = "# Output efficiency";

pub struct OutputEfficiencyRewrite;

impl OutputEfficiencyRewrite {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for OutputEfficiencyRewrite {
    fn name(&self) -> &str {
        "output-efficiency-rewrite"
    }
    fn order(&self) -> u32 {
        90
    }
    fn default_enabled(&self) -> bool {
        false
    }
}

impl RequestExtension for OutputEfficiencyRewrite {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        // Priority: ctx.meta override > env var.
        let raw = ctx
            .meta
            .get("outputEfficiencyReplacement")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                std::env::var("CACHE_FIX_OUTPUT_EFFICIENCY_REPLACEMENT").ok()
            })
            .unwrap_or_default();

        let replacement = normalize_replacement(&raw);
        if replacement.is_empty() {
            return Ok(None);
        }

        let system = match ctx.body.get("system") {
            Some(v) => v,
            None => return Ok(None),
        };

        if let Some(rewritten) = rewrite_output_efficiency(system, &replacement) {
            ctx.body["system"] = rewritten;
        }

        Ok(None)
    }
}

/// Normalize the replacement text. If non-empty and doesn't start with the section
/// header, prepend it.
fn normalize_replacement(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with(SECTION_HEADER) {
        trimmed.to_string()
    } else {
        format!("{}\n\n{}", SECTION_HEADER, trimmed)
    }
}

/// Find the "# Output efficiency" section in `text`, replace it with `replacement`,
/// and preserve everything that follows the NEXT section heading.
fn replace_section(text: &str, replacement: &str) -> Option<String> {
    let start = text.find(SECTION_HEADER)?;
    let after_header = start + SECTION_HEADER.len();
    let remainder = &text[after_header..];

    // Find the next "# " heading after the section header.
    if let Some(next_heading_match) = remainder.find("\n# ") {
        // next_heading_match points to the "\n". We want to keep the "\n"
        // and everything after it, but NOT the one before the heading.
        // The JS does: text.slice(0, start) + replacement + "\n\n" + text.slice(nextHeadingStart)
        // where nextHeadingStart = afterHeader + nextHeadingMatch.index + 1
        // The +1 skips the leading "\n" before "# ".
        let next_heading_start = after_header + next_heading_match + 1;
        Some(format!(
            "{}{}\n\n{}",
            &text[..start],
            replacement,
            &text[next_heading_start..]
        ))
    } else {
        // No next heading — replace from the section header to end.
        Some(format!("{}{}", &text[..start], replacement))
    }
}

/// Walk system blocks. For each text block containing SECTION_HEADER,
/// apply replace_section. Returns rewritten array if any block changed.
fn rewrite_output_efficiency(system: &Value, replacement: &str) -> Option<Value> {
    let blocks = system.as_array()?;
    let mut changed = false;
    let rewritten: Vec<Value> = blocks
        .iter()
        .map(|block| {
            if block.get("type").and_then(|v| v.as_str()) != Some("text") {
                return block.clone();
            }
            let text = match block.get("text").and_then(|v| v.as_str()) {
                Some(t) => t,
                None => return block.clone(),
            };
            if !text.contains(SECTION_HEADER) {
                return block.clone();
            }
            let next_text = match replace_section(text, replacement) {
                Some(t) => t,
                None => return block.clone(),
            };
            if next_text == text {
                return block.clone();
            }
            changed = true;
            let mut new_block = block.clone();
            new_block["text"] = Value::String(next_text);
            new_block
        })
        .collect();

    if changed {
        Some(Value::Array(rewritten))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_empty_returns_empty() {
        assert_eq!(normalize_replacement(""), "");
        assert_eq!(normalize_replacement("   "), "");
    }

    #[test]
    fn normalize_prepends_header() {
        let result = normalize_replacement("custom text");
        assert!(result.starts_with("# Output efficiency\n\n"));
        assert!(result.contains("custom text"));
    }

    #[test]
    fn normalize_does_not_double_header() {
        let input = "# Output efficiency\n\ncustom";
        assert_eq!(normalize_replacement(input), input);
    }

    #[test]
    fn replace_section_without_next_heading() {
        let text = "prefix\n# Output efficiency\nsome content";
        let replacement = "# Output efficiency\n\nnew content";
        let result = replace_section(text, replacement);
        assert_eq!(result.unwrap(), "prefix\n# Output efficiency\n\nnew content");
    }

    #[test]
    fn replace_section_with_next_heading() {
        let text = "prefix\n# Output efficiency\nsome content\n# Next Section\nmore";
        let replacement = "# Output efficiency\n\nnew content";
        let result = replace_section(text, replacement);
        assert_eq!(
            result.unwrap(),
            "prefix\n# Output efficiency\n\nnew content\n\n# Next Section\nmore"
        );
    }

    #[test]
    fn rewrite_skips_non_text_blocks() {
        let system = serde_json::json!([
            {"type": "image", "source": {}},
            {"type": "text", "text": "# Output efficiency\nold content"}
        ]);
        let replacement = "# Output efficiency\n\nnew content";
        let result = rewrite_output_efficiency(&system, replacement);
        assert!(result.is_some());
        let arr = result.unwrap();
        assert_eq!(arr[1]["text"].as_str().unwrap(), "# Output efficiency\n\nnew content");
    }
}
