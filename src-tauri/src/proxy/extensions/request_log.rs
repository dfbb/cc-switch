// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/request-log.mjs
// 翻译: 2026-05-20
//
// Optional NDJSON request timing log. Activated by setting
// CACHE_FIX_REQUEST_LOG=<path> and enabling via extensions.json:
//   "request-log": { "enabled": true, "order": 700 }
//
// Records one NDJSON line per message_delta with latency and token counts.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::json;
use std::io::Write;

// --- Extension struct ---

pub struct RequestLog;

impl RequestLog {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for RequestLog {
    fn name(&self) -> &str {
        "request-log"
    }
    fn order(&self) -> u32 {
        700
    }
    fn default_enabled(&self) -> bool {
        false
    }
}

// --- Environment helpers ---

fn log_path() -> Option<String> {
    std::env::var("CACHE_FIX_REQUEST_LOG").ok().filter(|s| !s.is_empty())
}

fn is_enabled() -> bool {
    log_path().is_some()
}

// --- RequestExtension ---

impl RequestExtension for RequestLog {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        ctx.meta.set("_requestStart", json!(now_ms));

        if let Some(model) = ctx.body.get("model").and_then(|v| v.as_str()) {
            ctx.meta.set("_requestModel", json!(model));
        } else {
            ctx.meta.set("_requestModel", json!(null));
        }

        Ok(None)
    }
}

// --- ResponseExtension ---

impl ResponseExtension for RequestLog {
    fn on_response_start(
        &self,
        ctx: &mut ResponseStartContext,
    ) -> Result<(), ExtensionError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        ctx.meta.set("_responseStart", json!(now_ms));
        Ok(())
    }
}

// --- StreamExtension ---

impl StreamExtension for RequestLog {
    fn on_stream_event(
        &self,
        ctx: &mut StreamEventContext,
    ) -> Result<(), ExtensionError> {
        let path = match log_path() {
            Some(p) => p,
            None => return Ok(()),
        };

        if ctx.event_type != "message_delta" {
            return Ok(());
        }

        let request_start = match ctx
            .meta
            .get("_requestStart")
            .and_then(|v| v.as_u64())
        {
            Some(ts) => ts,
            None => return Ok(()),
        };

        let response_start = ctx
            .meta
            .get("_responseStart")
            .and_then(|v| v.as_u64())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64
            });

        let latency_ms = response_start.saturating_sub(request_start);
        let output_tokens = ctx
            .data
            .get("usage")
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let model = ctx
            .meta
            .get("_requestModel")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let cache_read = ctx
            .meta
            .get("cacheStats")
            .and_then(|v| v.get("cacheRead"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let cache_creation = ctx
            .meta
            .get("cacheStats")
            .and_then(|v| v.get("cacheCreation"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let entry = json!({
            "ts": chrono::Utc::now().to_rfc3339(),
            "model": model,
            "latencyMs": latency_ms,
            "outputTokens": output_tokens,
            "cacheRead": cache_read,
            "cacheCreation": cache_creation,
        });

        // Fail-open: silently skip I/O errors.
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let line = serde_json::to_string(&entry).unwrap_or_default();
            let _ = writeln!(file, "{}", line);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_implements_traits() {
        let ext = RequestLog::new();
        assert_eq!(ext.name(), "request-log");
        assert_eq!(ext.order(), 700);
        assert!(!ext.default_enabled());
    }

    #[test]
    fn log_path_respects_env_var() {
        // Without the env var, log_path is None
        std::env::remove_var("CACHE_FIX_REQUEST_LOG");
        assert!(log_path().is_none());
    }
}
