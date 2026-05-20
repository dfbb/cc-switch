// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/thinking-display.mjs
// 翻译: 2026-05-20
//
// Injects thinking.display on Opus 4.7 requests when unset, to restore
// thinking summaries lost to CC's non-interactive CLI gate (claude-code#59844).
//
// Config via CACHE_FIX_THINKING_DISPLAY env var:
//   "summarized" (default) — inject display: "summarized"
//   "omitted"    — inject display: "omitted" (force-suppress)
//   "disabled"   — no injection; extension is a no-op

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const MODEL_REGEX: &str = r"^claude-opus-4-7";

fn model_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(MODEL_REGEX).unwrap())
}

const ACTIVE_THINKING_TYPES: &[&str] = &["enabled", "adaptive"];

fn resolve_mode() -> String {
    std::env::var("CACHE_FIX_THINKING_DISPLAY")
        .ok()
        .filter(|v| v == "summarized" || v == "omitted" || v == "disabled")
        .unwrap_or_else(|| "summarized".to_string())
}

fn should_inject(body: &Value) -> bool {
    let model = match body.get("model").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return false,
    };
    if !model_regex().is_match(model) {
        return false;
    }
    let thinking = match body.get("thinking") {
        Some(t) => t,
        None => return false,
    };
    if !thinking.is_object() {
        return false;
    }
    let thinking_type = match thinking.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return false,
    };
    if !ACTIVE_THINKING_TYPES.contains(&thinking_type) {
        return false;
    }
    // Only inject when display is unset. Preserve explicit user choice.
    thinking.get("display").is_none()
}

pub struct ThinkingDisplay;

impl ThinkingDisplay {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for ThinkingDisplay {
    fn name(&self) -> &str {
        "thinking-display"
    }
    fn order(&self) -> u32 {
        360
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for ThinkingDisplay {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let mode = resolve_mode();
        if mode == "disabled" {
            return Ok(None);
        }
        if !should_inject(&ctx.body) {
            return Ok(None);
        }

        ctx.body["thinking"]["display"] = Value::String(mode.clone());
        ctx.meta.set(
            "thinkingDisplayInjected",
            Value::String(mode),
        );

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn should_inject_opus47_no_display() {
        let body = json!({
            "model": "claude-opus-4-7-20250514",
            "thinking": {"type": "enabled"}
        });
        assert!(should_inject(&body));
    }

    #[test]
    fn should_inject_opus47_adaptive() {
        let body = json!({
            "model": "claude-opus-4-7-20250514",
            "thinking": {"type": "adaptive"}
        });
        assert!(should_inject(&body));
    }

    #[test]
    fn should_not_inject_when_display_present() {
        let body = json!({
            "model": "claude-opus-4-7-20250514",
            "thinking": {"type": "enabled", "display": "omitted"}
        });
        assert!(!should_inject(&body));
    }

    #[test]
    fn should_not_inject_non_opus47() {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "thinking": {"type": "enabled"}
        });
        assert!(!should_inject(&body));
    }

    #[test]
    fn should_not_inject_no_thinking() {
        let body = json!({
            "model": "claude-opus-4-7-20250514"
        });
        assert!(!should_inject(&body));
    }

    #[test]
    fn resolve_mode_defaults_to_summarized() {
        // Can't unset env in unit tests easily, so test default behavior.
        // resolve_mode() with no env set returns "summarized".
        std::env::remove_var("CACHE_FIX_THINKING_DISPLAY");
        assert_eq!(resolve_mode(), "summarized");
    }

    #[test]
    fn full_pipeline_injects_display() {
        let body = json!({
            "model": "claude-opus-4-7-20250514",
            "thinking": {"type": "enabled"}
        });
        let mut ctx = RequestContext {
            body,
            headers: axum::http::HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        // Temporarily set env for this test.
        std::env::set_var("CACHE_FIX_THINKING_DISPLAY", "summarized");
        let ext = ThinkingDisplay::new();
        ext.on_request(&mut ctx).unwrap();
        assert_eq!(
            ctx.body["thinking"]["display"].as_str().unwrap(),
            "summarized"
        );
        std::env::remove_var("CACHE_FIX_THINKING_DISPLAY");
    }
}
