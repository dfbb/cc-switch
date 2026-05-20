// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/rate-limit-log.mjs
// 翻译: 2026-05-20
//
// Append per-event record to ~/.claude/usage-log/rate-limit-events.jsonl when an
// upstream response carries the canonical Anthropic rate-limit error envelope.
// This is a SUPERSET of burst/concurrency events: the same envelope is returned
// for RPM/ITPM/OTPM and auto-mode classifier overflow.
//
// Detection signature: status === 429, body.type === "error",
// body.error.type === "rate_limit_error".
//
// Activation: enabled:false in the export default. Users opt in via
//   "rate-limit-log": { "enabled": true, "order": 660 }
// in proxy/extensions.json.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

// --- Constants ---

const BODY_EXCERPT_MAX: usize = 256;
const ACTIVE_SESSION_WINDOW_MS: u64 = 5 * 60 * 1000;

// --- Extension struct ---

pub struct RateLimitLog;

impl RateLimitLog {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for RateLimitLog {
    fn name(&self) -> &str {
        "rate-limit-log"
    }
    fn order(&self) -> u32 {
        660
    }
    fn default_enabled(&self) -> bool {
        false
    }
}

// --- Path helpers ---

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn log_path() -> String {
    format!("{}/.claude/usage-log/rate-limit-events.jsonl", home_dir())
}

fn account_path() -> String {
    format!("{}/.claude/quota-status/account.json", home_dir())
}

fn sessions_dir() -> String {
    format!("{}/.claude/quota-status/sessions", home_dir())
}

// --- Detection predicate ---

fn is_rate_limit_response(ctx: &ResponseContext) -> bool {
    if ctx.status != 429 {
        return false;
    }
    let body: Value = match serde_json::from_slice(&ctx.body) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let body_type = body.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if body_type != "error" {
        return false;
    }
    match body.get("error").and_then(|e| e.get("type")).and_then(|v| v.as_str()) {
        Some("rate_limit_error") => true,
        _ => false,
    }
}

// --- Field extractors ---

fn estimate_request_size_tokens(body: &Value) -> u64 {
    if body.is_null() {
        return 0;
    }
    let mut chars: u64 = 0;

    // System prompt
    if let Some(system) = body.get("system") {
        match system {
            Value::String(s) => chars += s.len() as u64,
            Value::Array(arr) => {
                for block in arr {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        chars += text.len() as u64;
                    }
                }
            }
            _ => {}
        }
    }

    // Messages
    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            if let Some(content) = msg.get("content") {
                match content {
                    Value::String(s) => chars += s.len() as u64,
                    Value::Array(arr) => {
                        for block in arr {
                            if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                                chars += text.len() as u64;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    (chars + 3) / 4 // ceil(chars / 4)
}

fn body_excerpt(body: &[u8]) -> String {
    let s = String::from_utf8_lossy(body);
    let excerpt: String = s.chars().take(BODY_EXCERPT_MAX).collect();
    excerpt
}

fn is_peak_hour_old_schedule(now: &chrono::DateTime<chrono::Utc>) -> bool {
    let day = now.format("%u").to_string().parse::<u32>().unwrap_or(0); // 1=Mon..7=Sun
    let hour = now.format("%H").to_string().parse::<u32>().unwrap_or(0);
    // Mon(1) through Fri(5), hours 13-18 UTC
    day >= 1 && day <= 5 && hour >= 13 && hour < 19
}

fn count_active_sessions(now_ms: u64) -> u64 {
    let dir = sessions_dir();
    let dir_path = Path::new(&dir);
    let entries = match std::fs::read_dir(dir_path) {
        Ok(e) => e,
        Err(_) => return 0,
    };

    let cutoff = now_ms.saturating_sub(ACTIVE_SESSION_WINDOW_MS);
    let mut count: u64 = 0;
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                if let Ok(ms) = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                {
                    if ms >= cutoff {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

fn read_q5h_pct_at_event() -> Option<f64> {
    let path = account_path();
    let raw = std::fs::read_to_string(&path).ok()?;
    let data: Value = serde_json::from_str(&raw).ok()?;
    data.get("five_hour")?.get("pct")?.as_f64()
}

// --- Record building ---

fn build_record(ctx: &ResponseContext, now: &chrono::DateTime<chrono::Utc>) -> Value {
    let body_val: Option<Value> = serde_json::from_slice(&ctx.body).ok();

    let body_req_id = body_val
        .as_ref()
        .and_then(|v| v.get("request_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let header_req_id = ctx
        .headers
        .get("request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let x_should_retry = ctx
        .headers
        .get("x-should-retry")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let session_id = ctx
        .meta
        .get("_sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let requested_model = ctx
        .meta
        .get("_requestedModel")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let request_path = ctx
        .meta
        .get("_requestPath")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "/v1/messages".to_string());

    let request_size_tokens = ctx
        .meta
        .get("_requestSizeTokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let upstream_connection_id = ctx
        .meta
        .get("_upstreamConnectionId")
        .and_then(|v| v.as_u64())
        .or_else(|| ctx.meta.get("_upstreamConnectionId").and_then(|v| v.as_str()).and_then(|s| s.parse::<u64>().ok()));

    let now_ms = now.timestamp_millis() as u64;

    let mut record = json!({
        "schema_version": 1,
        "ts": now.to_rfc3339(),
        "type": "rate_limit",
        "session_id": session_id,
        "requested_model": requested_model,
        "request_path": request_path,
        "request_size_tokens": request_size_tokens,
        "response_status": ctx.status,
        "response_body_excerpt": body_excerpt(&ctx.body),
        "concurrent_sessions_estimate": count_active_sessions(now_ms),
        "q5h_pct_at_event": read_q5h_pct_at_event(),
        "peak_hour_old_schedule": is_peak_hour_old_schedule(now),
        "upstream_request_id": body_req_id.or(header_req_id),
        "x_should_retry": x_should_retry,
        "upstream_connection_id": upstream_connection_id,
    });

    // Remove null fields
    if let Value::Object(ref mut map) = record {
        map.retain(|_, v| !v.is_null());
    }

    record
}

// --- File I/O ---

fn append_jsonl(record: &Value) -> Result<(), ExtensionError> {
    let path = log_path();
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ExtensionError::io("rate-limit-log", e))?;
    }
    let line =
        serde_json::to_string(record).map_err(|e| ExtensionError::json("rate-limit-log", e.to_string()))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| ExtensionError::io("rate-limit-log", e))?;
    writeln!(file, "{}", line).map_err(|e| ExtensionError::io("rate-limit-log", e))?;
    Ok(())
}

// --- RequestExtension ---

impl RequestExtension for RateLimitLog {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        if ctx.body.is_null() {
            return Ok(None);
        }

        let request_size = estimate_request_size_tokens(&ctx.body);
        ctx.meta.set("_requestSizeTokens", json!(request_size));

        if let Some(model) = ctx.body.get("model").and_then(|v| v.as_str()) {
            ctx.meta.set("_requestedModel", json!(model));
        }

        Ok(None)
    }
}

// --- ResponseExtension ---

impl ResponseExtension for RateLimitLog {
    fn on_response(&self, ctx: &mut ResponseContext) -> Result<(), ExtensionError> {
        if !is_rate_limit_response(ctx) {
            return Ok(());
        }

        let now = chrono::Utc::now();
        let record = build_record(ctx, &now);

        // Fail-open: silently skip I/O errors.
        let _ = append_jsonl(&record);

        Ok(())
    }
}

// --- StreamExtension (not needed) ---
// This extension does not implement StreamExtension.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_request_size_tokens_empty_body() {
        assert_eq!(estimate_request_size_tokens(&json!(null)), 0);
    }

    #[test]
    fn estimate_request_size_tokens_simple() {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "messages": [
                {"role": "user", "content": "hello world"} // 11 chars
            ]
        });
        // 11 chars / 4 = 2.75, ceil = 3
        assert_eq!(estimate_request_size_tokens(&body), 3);
    }

    #[test]
    fn estimate_request_size_tokens_with_system() {
        let body = json!({
            "model": "claude-sonnet-4-5",
            "system": [{"text": "You are helpful"}], // 16 chars
            "messages": [
                {"role": "user", "content": "hi"} // 2 chars
            ]
        });
        // 18 chars / 4 = 4.5, ceil = 5
        assert_eq!(estimate_request_size_tokens(&body), 5);
    }

    #[test]
    fn is_peak_hour_weekday_utc_afternoon() {
        // 2026-05-19 14:00 UTC is a Tuesday
        let dt = chrono::DateTime::parse_from_rfc3339("2026-05-19T14:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(is_peak_hour_old_schedule(&dt));
    }

    #[test]
    fn is_peak_hour_weekend() {
        // 2026-05-17 14:00 UTC is a Sunday
        let dt = chrono::DateTime::parse_from_rfc3339("2026-05-17T14:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert!(!is_peak_hour_old_schedule(&dt));
    }

    #[test]
    fn is_rate_limit_response_detects_429_error() {
        let ctx = ResponseContext {
            status: 429,
            headers: axum::http::HeaderMap::new(),
            body: br#"{"type":"error","error":{"type":"rate_limit_error","message":"Error"}}"#.to_vec(),
            meta: ExtensionMeta::default(),
        };
        assert!(is_rate_limit_response(&ctx));
    }

    #[test]
    fn is_rate_limit_response_rejects_200() {
        let ctx = ResponseContext {
            status: 200,
            headers: axum::http::HeaderMap::new(),
            body: br#"{"type":"error","error":{"type":"rate_limit_error","message":"Error"}}"#.to_vec(),
            meta: ExtensionMeta::default(),
        };
        assert!(!is_rate_limit_response(&ctx));
    }

    #[test]
    fn is_rate_limit_response_rejects_non_error_type() {
        let ctx = ResponseContext {
            status: 429,
            headers: axum::http::HeaderMap::new(),
            body: br#"{"type":"message","message":{}}"#.to_vec(),
            meta: ExtensionMeta::default(),
        };
        assert!(!is_rate_limit_response(&ctx));
    }

    #[test]
    fn extension_implements_traits() {
        let ext = RateLimitLog::new();
        assert_eq!(ext.name(), "rate-limit-log");
        assert_eq!(ext.order(), 660);
        assert!(!ext.default_enabled());
    }
}
