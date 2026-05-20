// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/cache-telemetry.mjs
// 翻译: 2026-05-20
//
// Multi-hook extension that captures cache/usage telemetry across three phases:
//   Request:       extract session ID from request headers → ctx.meta._sessionId
//   ResponseStart: extract quota data from upstream_headers → ctx.meta._quotaData
//   Stream:        extract cache stats from message_start/message_delta SSE events
//                  → persist account.json + per-session JSON to ~/.claude/quota-status/
//
// Env: CACHE_FIX_QUOTA_STATUS_TTL_DAYS (default 7) controls stale session sweep.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use chrono::{Datelike, Timelike};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// --- Constants ---

const SWEEP_THROTTLE_MS: u64 = 60_000;
const DEFAULT_TTL_DAYS: u64 = 7;

// --- Module-scope state ---

static LEGACY_CLEANUP_DONE: AtomicBool = AtomicBool::new(false);
static LAST_SWEEP_MS: AtomicU64 = AtomicU64::new(0);

// --- Extension struct ---

pub struct CacheTelemetry;

impl CacheTelemetry {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for CacheTelemetry {
    fn name(&self) -> &str {
        "cache-telemetry"
    }
    fn order(&self) -> u32 {
        600
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

// --- Request phase: capture session ID ---

impl RequestExtension for CacheTelemetry {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let sid = resolve_session_id(&ctx.headers);
        if let Some(s) = sid {
            ctx.meta.set("_sessionId", Value::String(s));
        }
        Ok(None)
    }
}

// --- ResponseStart phase: capture quota data ---

impl ResponseExtension for CacheTelemetry {
    fn on_response_start(
        &self,
        ctx: &mut ResponseStartContext,
    ) -> Result<(), ExtensionError> {
        let quota = parse_headers(&ctx.upstream_headers);
        if let Some(q) = quota {
            ctx.meta.set("_quotaData", q);
        }
        Ok(())
    }
}

// --- Stream phase: extract cache stats + persist ---

impl StreamExtension for CacheTelemetry {
    fn on_stream_event(
        &self,
        ctx: &mut StreamEventContext,
    ) -> Result<(), ExtensionError> {
        // message_start: capture input-side cache usage.
        if ctx.event_type == "message_start" {
            if let Some(message) = ctx.data.get("message") {
                if let Some(usage) = message.get("usage") {
                    let stats = json!({
                        "cacheRead": usage.get("cache_read_input_tokens")
                            .and_then(|v| v.as_u64()).unwrap_or(0),
                        "cacheCreation": usage.get("cache_creation_input_tokens")
                            .and_then(|v| v.as_u64()).unwrap_or(0),
                        "inputTokens": usage.get("input_tokens")
                            .and_then(|v| v.as_u64()).unwrap_or(0),
                    });
                    ctx.meta.set("cacheStats", stats);
                }
            }
        }

        // message_delta: capture output tokens + persist final snapshot.
        if ctx.event_type == "message_delta" {
            // Merge output_tokens into cacheStats.
            if let Some(usage) = ctx.data.get("usage") {
                let mut stats = ctx
                    .meta
                    .get("cacheStats")
                    .cloned()
                    .unwrap_or(json!({}));
                if let Some(obj) = stats.as_object_mut() {
                    obj.insert(
                        "outputTokens".to_string(),
                        Value::Number(
                            usage
                                .get("output_tokens")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0)
                                .into(),
                        ),
                    );
                }
                ctx.meta.set("cacheStats", stats);
            }

            // Persist if we have both cacheStats and quotaData.
            let stats = match ctx.meta.get("cacheStats") {
                Some(s) => s,
                None => return Ok(()),
            };
            let quota = match ctx.meta.get("_quotaData") {
                Some(q) => q,
                None => return Ok(()),
            };

            let cr = stats.get("cacheRead").and_then(|v| v.as_u64()).unwrap_or(0);
            let cc = stats.get("cacheCreation").and_then(|v| v.as_u64()).unwrap_or(0);
            let total = cr + cc;
            let hit_rate = if total > 0 {
                format!("{:.1}", (cr as f64 / total as f64) * 100.0)
            } else {
                "N/A".to_string()
            };

            let ephemeral_1h = cc;
            let ephemeral_5m: u64 = 0;
            let ttl = if cr > 0 {
                "1h"
            } else if cc > 0 {
                "5m"
            } else {
                "unknown"
            };

            let timestamp = chrono::Utc::now().to_rfc3339();
            let raw_sid = ctx
                .meta
                .get("_sessionId")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let filename = session_filename(raw_sid.as_deref());

            // Build account payload: quota data + timestamp.
            let mut account_payload = quota.clone();
            if let Some(obj) = account_payload.as_object_mut() {
                obj.insert("timestamp".to_string(), Value::String(timestamp.clone()));
            }
            let account_json = serde_json::to_string_pretty(&account_payload)
                .unwrap_or_default();

            // Build session payload.
            let session_payload = json!({
                "cache": {
                    "ttl_tier": ttl,
                    "cache_creation": cc,
                    "cache_read": cr,
                    "ephemeral_1h": ephemeral_1h,
                    "ephemeral_5m": ephemeral_5m,
                    "hit_rate": hit_rate,
                    "timestamp": timestamp,
                },
                "timestamp": timestamp,
                "session_id": raw_sid,
            });
            let session_json = serde_json::to_string_pretty(&session_payload)
                .unwrap_or_default();

            // Persist to filesystem (best-effort, errors are silent).
            let (_quota_dir, account_path, sessions_dir) = paths();
            if let Err(e) = std::fs::create_dir_all(&sessions_dir) {
                log::warn!(
                    "[cache-telemetry] 无法创建 sessions 目录 {}: {e}",
                    sessions_dir
                );
                return Ok(());
            }

            cleanup_legacy_once();
            let _ = atomic_write(&account_path, &account_json);
            let _ = atomic_write(
                &format!("{sessions_dir}/{filename}.json"),
                &session_json,
            );
            sweep_stale_sessions(get_ttl_days());
        }

        Ok(())
    }
}

// --- Paths ---

fn paths() -> (String, String, String) {
    let home = dirs::home_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let quota_dir = format!("{home}/.claude/quota-status");
    let account_path = format!("{quota_dir}/account.json");
    let sessions_dir = format!("{quota_dir}/sessions");
    (quota_dir, account_path, sessions_dir)
}

// --- sessionFilename: derive a filesystem-safe name from a raw session id ---

fn is_safe_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn session_filename(raw_id: Option<&str>) -> String {
    match raw_id {
        None | Some("") => "unknown".to_string(),
        Some(s) if is_safe_name(s) => s.to_string(),
        Some(s) => {
            let mut hasher = Sha256::new();
            hasher.update(s.as_bytes());
            let hash = hasher.finalize();
            let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
            format!("inv-{}", &hex[..16.min(hex.len())])
        }
    }
}

// --- resolveSessionId: extract session identity from request headers ---

fn resolve_session_id(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-claude-code-session-id")
        .or_else(|| headers.get("x-session-id"))
        .or_else(|| headers.get("x-anthropic-session-id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

// --- parseHeaders: extract Anthropic rate-limit headers from upstream ---

fn parse_headers(headers: &axum::http::HeaderMap) -> Option<Value> {
    let h = |key: &str| -> String {
        headers
            .get(key)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let num = |key: &str| -> f64 { h(key).parse::<f64>().unwrap_or(0.0) };
    let int = |key: &str| -> i64 { h(key).parse::<i64>().unwrap_or(0) };

    let q5h_util = num("anthropic-ratelimit-unified-5h-utilization");
    let q7d_util = num("anthropic-ratelimit-unified-7d-utilization");
    let q5h_reset = int("anthropic-ratelimit-unified-5h-reset");
    let q7d_reset = int("anthropic-ratelimit-unified-7d-reset");
    let status =
        h("anthropic-ratelimit-unified-status");
    let status = if status.is_empty() {
        h("anthropic-ratelimit-unified-5h-status")
    } else {
        status
    };
    let overage_status = h("anthropic-ratelimit-unified-overage-status");
    let _overage_util = num("anthropic-ratelimit-unified-overage-utilization");
    let overage_reset = int("anthropic-ratelimit-unified-overage-reset");
    let unified_reset = int("anthropic-ratelimit-unified-reset");

    // Accept any reset timestamp.
    if q5h_reset == 0 && q7d_reset == 0 && unified_reset == 0 && overage_reset == 0 {
        return None;
    }

    // Compute peak hour (UTC 13-18, Mon-Fri).
    let now = chrono::Utc::now();
    let hour = now.hour();
    let day = now.weekday().num_days_from_monday();
    let peak = day >= 1 && day <= 5 && hour >= 13 && hour < 19;

    // Collect relevant headers for all_headers.
    let mut all_headers = serde_json::Map::new();
    for (name, value) in headers.iter() {
        let key = name.as_str().to_lowercase();
        if key.starts_with("anthropic-") || key == "cf-ray" || key == "request-id" {
            if let Ok(v) = value.to_str() {
                all_headers.insert(key, Value::String(v.to_string()));
            }
        }
    }

    Some(json!({
        "five_hour": {
            "utilization": q5h_util,
            "pct": (q5h_util * 100.0).round() as i64,
            "resets_at": q5h_reset,
        },
        "seven_day": {
            "utilization": q7d_util,
            "pct": (q7d_util * 100.0).round() as i64,
            "resets_at": q7d_reset,
        },
        "status": if status.is_empty() { "unknown" } else { &status },
        "overage_status": if overage_status.is_empty() { "unknown" } else { &overage_status },
        "peak_hour": peak,
        "all_headers": all_headers,
    }))
}

// --- Atomic file write: write to tmp then rename ---

fn atomic_write(final_path: &str, content: &str) -> Result<(), std::io::Error> {
    let now_ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = format!("{final_path}.tmp.{}.{now_ns:x}", std::process::id());
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, final_path)?;
    // Best-effort cleanup of tmp file (may not exist after rename).
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

// --- Cleanup legacy quota-status.json ---

fn cleanup_legacy_once() {
    if LEGACY_CLEANUP_DONE.swap(true, Ordering::Relaxed) {
        return;
    }
    let home = dirs::home_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let legacy_path = format!("{home}/.claude/quota-status.json");
    let _ = std::fs::remove_file(&legacy_path);
}

// --- Sweep stale session files ---

fn sweep_stale_sessions(ttl_days: u64) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let last = LAST_SWEEP_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(last) < SWEEP_THROTTLE_MS {
        return;
    }
    LAST_SWEEP_MS.store(now_ms, Ordering::Relaxed);

    let cutoff_ms = now_ms.saturating_sub(ttl_days * 86_400_000);
    let (_, _, sessions_dir) = paths();

    let entries = match std::fs::read_dir(&sessions_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if let Ok(meta) = std::fs::metadata(&path) {
            if let Ok(mtime) = meta.modified() {
                if let Ok(mtime_ms) = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                {
                    if mtime_ms < cutoff_ms {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }
}

// --- TTL from env ---

fn get_ttl_days() -> u64 {
    match std::env::var("CACHE_FIX_QUOTA_STATUS_TTL_DAYS") {
        Ok(raw) if !raw.is_empty() => {
            raw.parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .map(|n| n as u64)
                .unwrap_or(DEFAULT_TTL_DAYS)
        }
        _ => DEFAULT_TTL_DAYS,
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_filename_unknown() {
        assert_eq!(session_filename(None), "unknown");
        assert_eq!(session_filename(Some("")), "unknown");
    }

    #[test]
    fn session_filename_safe_passthrough() {
        assert_eq!(
            session_filename(Some("abc123_XY-Z")),
            "abc123_XY-Z"
        );
    }

    #[test]
    fn session_filename_hashes_unsafe() {
        let result = session_filename(Some("session/with/slashes"));
        assert!(result.starts_with("inv-"));
        assert_eq!(result.len(), 4 + 16); // "inv-" + 16 hex chars
    }

    #[test]
    fn session_filename_deterministic() {
        let a = session_filename(Some("same-input"));
        let b = session_filename(Some("same-input"));
        assert_eq!(a, b);
    }

    #[test]
    fn is_safe_name_valid() {
        assert!(is_safe_name("abc"));
        assert!(is_safe_name("ABC123"));
        assert!(is_safe_name("hello_world-123"));
    }

    #[test]
    fn is_safe_name_invalid() {
        assert!(!is_safe_name(""));
        assert!(!is_safe_name("hello world"));
        assert!(!is_safe_name("path/to/file"));
        assert!(!is_safe_name(&"a".repeat(129)));
    }

    #[test]
    fn resolve_session_id_prefers_claude_code() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-claude-code-session-id", "cc-123".parse().unwrap());
        headers.insert("x-session-id", "other-456".parse().unwrap());
        assert_eq!(
            resolve_session_id(&headers),
            Some("cc-123".to_string())
        );
    }

    #[test]
    fn resolve_session_id_falls_back() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-session-id", "fallback-789".parse().unwrap());
        assert_eq!(
            resolve_session_id(&headers),
            Some("fallback-789".to_string())
        );
    }

    #[test]
    fn resolve_session_id_returns_none_when_empty() {
        let headers = axum::http::HeaderMap::new();
        assert_eq!(resolve_session_id(&headers), None);
    }

    #[test]
    fn parse_headers_returns_none_when_no_reset() {
        let headers = axum::http::HeaderMap::new();
        assert!(parse_headers(&headers).is_none());
    }

    #[test]
    fn parse_headers_extracts_basic_quota() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-unified-5h-reset",
            "1700000000".parse().unwrap(),
        );
        let result = parse_headers(&headers);
        assert!(result.is_some());
        let q = result.unwrap();
        assert_eq!(
            q.get("five_hour")
                .and_then(|v| v.get("resets_at"))
                .and_then(|v| v.as_i64()),
            Some(1700000000)
        );
    }

    #[test]
    fn parse_headers_collects_all_headers() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-unified-5h-reset",
            "1700000000".parse().unwrap(),
        );
        headers.insert("request-id", "req_abc".parse().unwrap());
        headers.insert("cf-ray", "123-ray".parse().unwrap());
        let result = parse_headers(&headers).unwrap();
        let all = result
            .get("all_headers")
            .and_then(|v| v.as_object())
            .unwrap();
        assert!(all.contains_key("request-id"));
        assert!(all.contains_key("cf-ray"));
    }

    #[test]
    fn get_ttl_days_default() {
        // Safe to call in tests — reads env var that's likely not set.
        let days = get_ttl_days();
        assert_eq!(days, DEFAULT_TTL_DAYS);
    }

    #[test]
    fn atomic_write_roundtrip() {
        let dir = std::env::temp_dir();
        let path = format!(
            "{}/cache-telemetry-test-{}.json",
            dir.to_string_lossy(),
            std::process::id()
        );
        let content = r#"{"hello":"world"}"#;
        atomic_write(&path, content).unwrap();
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, content);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extension_name_and_order() {
        let ext = CacheTelemetry::new();
        assert_eq!(ext.name(), "cache-telemetry");
        assert_eq!(ext.order(), 600);
        assert!(ext.default_enabled());
    }

    #[test]
    fn request_phase_stores_session_id() {
        let ext = CacheTelemetry::new();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-claude-code-session-id", "test-sid".parse().unwrap());

        let mut ctx = RequestContext {
            body: serde_json::json!({}),
            headers,
            meta: ExtensionMeta::default(),
        };

        let result = ext.on_request(&mut ctx).unwrap();
        assert!(result.is_none());
        assert_eq!(
            ctx.meta
                .get("_sessionId")
                .and_then(|v| v.as_str()),
            Some("test-sid")
        );
    }

    #[test]
    fn response_start_phase_stores_quota_data() {
        let ext = CacheTelemetry::new();
        let mut upstream_headers = axum::http::HeaderMap::new();
        upstream_headers.insert(
            "anthropic-ratelimit-unified-5h-reset",
            "1700000000".parse().unwrap(),
        );

        let mut ctx = ResponseStartContext {
            status: 200,
            headers: axum::http::HeaderMap::new(),
            upstream_headers,
            meta: ExtensionMeta::default(),
        };

        ext.on_response_start(&mut ctx).unwrap();
        assert!(ctx.meta.get("_quotaData").is_some());
    }

    #[test]
    fn stream_message_start_captures_cache_stats() {
        let ext = CacheTelemetry::new();
        let mut ctx = StreamEventContext {
            event_type: "message_start".to_string(),
            data: serde_json::json!({
                "message": {
                    "usage": {
                        "cache_read_input_tokens": 100,
                        "cache_creation_input_tokens": 50,
                        "input_tokens": 200
                    }
                }
            }),
            response_headers: axum::http::HeaderMap::new(),
            drop: false,
            meta: ExtensionMeta::default(),
            telemetry: TelemetryCollector::default(),
        };

        ext.on_stream_event(&mut ctx).unwrap();
        let stats = ctx.meta.get("cacheStats").unwrap();
        assert_eq!(stats.get("cacheRead").and_then(|v| v.as_u64()), Some(100));
        assert_eq!(
            stats.get("cacheCreation").and_then(|v| v.as_u64()),
            Some(50)
        );
        assert_eq!(
            stats.get("inputTokens").and_then(|v| v.as_u64()),
            Some(200)
        );
    }

    #[test]
    fn stream_message_delta_merges_output_tokens() {
        let ext = CacheTelemetry::new();

        // Pre-populate quota so the persist path is reached.
        let mut meta = ExtensionMeta::default();
        meta.set(
            "_quotaData",
            serde_json::json!({
                "five_hour": {"utilization": 0.5, "pct": 50, "resets_at": 1},
                "seven_day": {"utilization": 0.3, "pct": 30, "resets_at": 1},
                "status": "ok",
                "overage_status": "unknown",
                "peak_hour": false,
                "all_headers": {}
            }),
        );
        meta.set(
            "_sessionId",
            Value::String("test-session".to_string()),
        );
        // Pre-populate cacheStats from message_start.
        meta.set(
            "cacheStats",
            serde_json::json!({
                "cacheRead": 100,
                "cacheCreation": 50,
                "inputTokens": 200
            }),
        );

        let mut ctx = StreamEventContext {
            event_type: "message_delta".to_string(),
            data: serde_json::json!({
                "usage": {
                    "output_tokens": 300
                }
            }),
            response_headers: axum::http::HeaderMap::new(),
            drop: false,
            meta,
            telemetry: TelemetryCollector::default(),
        };

        ext.on_stream_event(&mut ctx).unwrap();

        let stats = ctx.meta.get("cacheStats").unwrap();
        assert_eq!(
            stats.get("outputTokens").and_then(|v| v.as_u64()),
            Some(300)
        );
    }
}
