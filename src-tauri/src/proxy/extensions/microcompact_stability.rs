// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/microcompact-stability.mjs
// 翻译: 2026-05-20
//
// Detects and normalizes CC's `time_based_microcompact` sentinel string in
// tool_result content. Two detection modes:
//   - Mode A: Exact match against confirmed patterns → eligible for normalization
//   - Mode B: Prefix-only match → diagnostic-only (tracked in stats)
//
// Normalization is gated by env var CACHE_FIX_NORMALIZE_MICROCOMPACT=1.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const DEFAULT_CANONICAL_TEXT: &str = "[Old tool result content cleared]";
const SENTINEL_PREFIX: &str = "[Old tool result content cleared";

/// Get the compiled default exact-match patterns.
fn default_patterns() -> &'static [(Regex, &'static str)] {
    static PATTERNS: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        vec![
            (
                Regex::new(r"^\[Old tool result content cleared\]\s*$").unwrap(),
                r"^\[Old tool result content cleared\]\s*$",
            ),
            (
                Regex::new(r"^\[Old tool result content cleared at \d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z\]\s*$").unwrap(),
                r"^\[Old tool result content cleared at \d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z\]\s*$",
            ),
        ]
    })
}

fn is_normalize_enabled() -> bool {
    std::env::var("CACHE_FIX_NORMALIZE_MICROCOMPACT")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn get_canonical_text() -> String {
    std::env::var("CACHE_FIX_MICROCOMPACT_NORMALIZED")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_CANONICAL_TEXT.to_string())
}

/// Collect user-supplied regex patterns from CACHE_FIX_MICROCOMPACT_SENTINEL_PATTERN_<N>.
fn get_custom_patterns() -> Vec<(Regex, String)> {
    let mut out = Vec::new();
    for (key, value) in std::env::vars() {
        if !key.starts_with("CACHE_FIX_MICROCOMPACT_SENTINEL_PATTERN_") {
            continue;
        }
        if value.is_empty() {
            continue;
        }
        match Regex::new(&value) {
            Ok(re) => out.push((re, value)),
            Err(_) => {
                eprintln!("[microcompact] invalid regex in {}: {}", key, value);
            }
        }
    }
    out
}

/// Collect user-supplied literal prefixes for Mode B detection.
fn get_custom_prefixes() -> Vec<String> {
    let mut out = Vec::new();
    for (key, value) in std::env::vars() {
        if !key.starts_with("CACHE_FIX_MICROCOMPACT_SENTINEL_PREFIX_") {
            continue;
        }
        if value.is_empty() {
            continue;
        }
        out.push(value);
    }
    out
}

pub struct MicrocompactStability;

impl MicrocompactStability {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for MicrocompactStability {
    fn name(&self) -> &str {
        "microcompact-stability"
    }
    fn order(&self) -> u32 {
        350
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for MicrocompactStability {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let normalize = is_normalize_enabled();
        if !normalize {
            return Ok(None);
        }

        let messages = match ctx.body.get("messages").and_then(|v| v.as_array()) {
            Some(m) => m,
            None => return Ok(None),
        };
        if messages.is_empty() {
            return Ok(None);
        }

        let custom_patterns = get_custom_patterns();
        let custom_prefixes = get_custom_prefixes();
        let canonical_text = get_canonical_text();

        let result = walk_and_normalize(
            ctx.body["messages"].as_array_mut().unwrap(),
            &custom_patterns,
            &custom_prefixes,
            &canonical_text,
        );

        let stats = serde_json::json!({
            "normalization_enabled": true,
            "total_tool_results_scanned": result.total_tool_results,
            "exact_matches_count": result.exact_matches,
            "partial_matches_count": result.partial_matches,
            "sentinels_matched": result.exact_matches + result.partial_matches,
            "sentinels_normalized": result.normalized,
            "bytes_original": result.bytes_original,
            "bytes_normalized": result.bytes_normalized,
            "bytes_saved": result.bytes_original.saturating_sub(result.bytes_normalized),
            "sentinel_pattern_used": result.matched_pattern,
        });

        if result.exact_matches > 0 || result.partial_matches > 0 {
            ctx.meta.set("microcompactStats", stats);
            if result.exact_matches > 0 || result.partial_matches > 0 {
                let pattern_tag = result
                    .matched_pattern
                    .as_deref()
                    .map(|p| {
                        let defaults = default_patterns();
                        if defaults.iter().any(|(_, src)| *src == p) {
                            "default"
                        } else {
                            "custom"
                        }
                    })
                    .unwrap_or("none");
                eprintln!(
                    "[microcompact] matched={} normalized={} bytes={}->{} sentinel_pattern={}",
                    result.exact_matches + result.partial_matches,
                    result.normalized,
                    result.bytes_original,
                    result.bytes_normalized,
                    pattern_tag,
                );
            }
        }

        Ok(None)
    }
}

struct WalkResult {
    total_tool_results: usize,
    exact_matches: usize,
    partial_matches: usize,
    normalized: usize,
    bytes_original: usize,
    bytes_normalized: usize,
    matched_pattern: Option<String>,
}

/// Check if text matches any exact pattern (default + custom).
/// Returns the pattern source string on match, or None.
fn matches_exact(text: &str, custom_patterns: &[(Regex, String)]) -> Option<String> {
    for (re, _source) in default_patterns() {
        if re.is_match(text) {
            return Some(_source.to_string());
        }
    }
    for (re, source) in custom_patterns {
        if re.is_match(text) {
            return Some(source.clone());
        }
    }
    None
}

/// Check if text starts with the sentinel prefix (default or custom).
fn matches_prefix(text: &str, custom_prefixes: &[String]) -> bool {
    if text.starts_with(SENTINEL_PREFIX) {
        return true;
    }
    for p in custom_prefixes {
        if text.starts_with(p) {
            return true;
        }
    }
    false
}

fn walk_and_normalize(
    messages: &mut [Value],
    custom_patterns: &[(Regex, String)],
    custom_prefixes: &[String],
    canonical_text: &str,
) -> WalkResult {
    let mut result = WalkResult {
        total_tool_results: 0,
        exact_matches: 0,
        partial_matches: 0,
        normalized: 0,
        bytes_original: 0,
        bytes_normalized: 0,
        matched_pattern: None,
    };

    let canonical_bytes = canonical_text.len();

    for msg in messages.iter_mut() {
        let content = match msg.get_mut("content").and_then(|v| v.as_array_mut()) {
            Some(c) => c,
            None => continue,
        };
        for block in content.iter_mut() {
            if block.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
                continue;
            }
            result.total_tool_results += 1;

            // Handle string content.
            if let Some(text) = block.get("content").and_then(|v| v.as_str()) {
                if let Some(pattern_src) = matches_exact(text, custom_patterns) {
                    result.exact_matches += 1;
                    if result.matched_pattern.is_none() {
                        result.matched_pattern = Some(pattern_src);
                    }
                    result.bytes_original += text.len();
                    result.bytes_normalized += canonical_bytes;
                    block["content"] = Value::String(canonical_text.to_string());
                    result.normalized += 1;
                } else if matches_prefix(text, custom_prefixes) {
                    result.partial_matches += 1;
                }
                continue;
            }

            // Handle array content.
            if let Some(items) = block.get_mut("content").and_then(|v| v.as_array_mut()) {
                for item in items.iter_mut() {
                    if item.get("type").and_then(|v| v.as_str()) != Some("text") {
                        continue;
                    }
                    let text = match item.get("text").and_then(|v| v.as_str()) {
                        Some(t) => t,
                        None => continue,
                    };

                    if let Some(pattern_src) = matches_exact(text, custom_patterns) {
                        result.exact_matches += 1;
                        if result.matched_pattern.is_none() {
                            result.matched_pattern = Some(pattern_src);
                        }
                        result.bytes_original += text.len();
                        result.bytes_normalized += canonical_bytes;
                        item["text"] = Value::String(canonical_text.to_string());
                        result.normalized += 1;
                    } else if matches_prefix(text, custom_prefixes) {
                        result.partial_matches += 1;
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_exact_default_pattern_plain() {
        assert!(matches_exact("[Old tool result content cleared]", &[]).is_some());
        assert!(matches_exact("[Old tool result content cleared]  ", &[]).is_some());
    }

    #[test]
    fn matches_exact_default_pattern_with_timestamp() {
        assert!(
            matches_exact(
                "[Old tool result content cleared at 2025-06-15T12:34:56Z]",
                &[]
            )
            .is_some()
        );
    }

    #[test]
    fn matches_exact_no_match() {
        assert!(matches_exact("random text", &[]).is_none());
        assert!(matches_exact("[Old tool result content cl", &[]).is_none());
    }

    #[test]
    fn matches_prefix_matches_default() {
        assert!(matches_prefix("[Old tool result content cleared and something else", &[]));
    }

    #[test]
    fn matches_prefix_no_match() {
        assert!(!matches_prefix("other prefix", &[]));
    }

    #[test]
    fn normalize_string_content() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "content": "[Old tool result content cleared]"
            }]
        })];
        let result = walk_and_normalize(
            &mut messages,
            &[],
            &[],
            DEFAULT_CANONICAL_TEXT,
        );
        assert_eq!(result.exact_matches, 1);
        assert_eq!(result.normalized, 1);
        let content = &messages[0]["content"][0]["content"];
        assert_eq!(content.as_str().unwrap(), DEFAULT_CANONICAL_TEXT);
    }

    #[test]
    fn normalize_array_content() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "content": [
                    {"type": "text", "text": "[Old tool result content cleared]"}
                ]
            }]
        })];
        let result = walk_and_normalize(
            &mut messages,
            &[],
            &[],
            DEFAULT_CANONICAL_TEXT,
        );
        assert_eq!(result.exact_matches, 1);
        assert_eq!(result.normalized, 1);
    }

    #[test]
    fn partial_matches_not_normalized() {
        let mut messages = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "content": "[Old tool result content cleared but with extra data]"
            }]
        })];
        let result = walk_and_normalize(
            &mut messages,
            &[],
            &[],
            DEFAULT_CANONICAL_TEXT,
        );
        assert_eq!(result.exact_matches, 0);
        assert_eq!(result.partial_matches, 1);
        assert_eq!(result.normalized, 0);
    }
}
