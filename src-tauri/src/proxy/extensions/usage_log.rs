// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/usage-log.mjs
// 翻译: 2026-05-20
//
// Append per-call usage record to ~/.claude/usage.jsonl.
// The emitted record matches MeterRowSchema v:1 from claude-code-meter.
//
// Activation: enabled:false in the export default. Users opt in by adding:
//   "usage-log": { "enabled": true, "order": 650 }
// CACHE_FIX_USAGE_LOG=<path> overrides the destination path only.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::sync::Mutex;

// --- Module-scope state ---

static SID: Mutex<Option<String>> = Mutex::new(None);
static LAST_Q5H: Mutex<Option<f64>> = Mutex::new(None);
static LAST_Q7D: Mutex<Option<f64>> = Mutex::new(None);

fn get_sid() -> String {
    let mut sid = SID.lock().expect("sid lock poisoned");
    if let Some(ref s) = *sid {
        return s.clone();
    }
    let new_sid = generate_sid();
    *sid = Some(new_sid.clone());
    new_sid
}

fn get_last_q5h() -> Option<f64> {
    *LAST_Q5H.lock().expect("q5h lock poisoned")
}

fn set_last_q5h(val: f64) {
    *LAST_Q5H.lock().expect("q5h lock poisoned") = Some(val);
}

fn get_last_q7d() -> Option<f64> {
    *LAST_Q7D.lock().expect("q7d lock poisoned")
}

fn set_last_q7d(val: f64) {
    *LAST_Q7D.lock().expect("q7d lock poisoned") = Some(val);
}

// --- Extension struct ---

pub struct UsageLog;

impl UsageLog {
    pub fn new() -> Self {
        // Pre-initialize SID.
        let _ = get_sid();
        Self
    }
}

impl Extension for UsageLog {
    fn name(&self) -> &str {
        "usage-log"
    }
    fn order(&self) -> u32 {
        650
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
    std::env::var("CACHE_FIX_USAGE_LOG")
        .unwrap_or_else(|_| format!("{}/.claude/usage.jsonl", home_dir()))
}

// --- Pure helpers ---

fn generate_sid() -> String {
    let seed = format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        // Simple random-ish component
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let hash = Sha256::digest(seed.as_bytes());
    let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    hex[..8].to_string()
}

fn hash_org_id(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    if raw.is_empty() {
        return None;
    }
    let hash = Sha256::digest(raw.as_bytes());
    let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    Some(hex[..16].to_string())
}

// --- Message field extractors ---

struct MessageStartFields {
    model: String,
    speed: String,
    service_tier: String,
    input_tokens: u64,
    cache_creation_input_tokens: u64,
    cache_read_input_tokens: u64,
    ephemeral_1h_input_tokens: u64,
    ephemeral_5m_input_tokens: u64,
    web_search_requests: u64,
}

fn extract_message_start_fields(event: &Value) -> Option<MessageStartFields> {
    if event.get("type")?.as_str()? != "message_start" {
        return None;
    }
    let msg = event.get("message")?;
    let usage = msg.get("usage")?;

    let cc = usage.get("cache_creation").unwrap_or(&Value::Null);
    let sti = usage.get("server_tool_use").unwrap_or(&Value::Null);

    Some(MessageStartFields {
        model: msg
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        speed: usage
            .get("speed")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        service_tier: usage
            .get("service_tier")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        input_tokens: usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_creation_input_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_read_input_tokens: usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        ephemeral_1h_input_tokens: cc
            .get("ephemeral_1h_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        ephemeral_5m_input_tokens: cc
            .get("ephemeral_5m_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        web_search_requests: sti
            .get("web_search_requests")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
    })
}

fn extract_message_delta_fields(event: &Value) -> Option<u64> {
    if event.get("type")?.as_str()? != "message_delta" {
        return None;
    }
    event
        .get("usage")?
        .get("output_tokens")
        .and_then(|v| v.as_u64())
}

// --- Quota header parsing ---

fn num_header(headers: &axum::http::HeaderMap, key: &str) -> Option<f64> {
    headers.get(key)?.to_str().ok()?.parse::<f64>().ok()
}

fn int_header(headers: &axum::http::HeaderMap, key: &str) -> i64 {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

fn str_header_opt(headers: &axum::http::HeaderMap, key: &str) -> Option<String> {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

fn str_header(headers: &axum::http::HeaderMap, key: &str) -> String {
    str_header_opt(headers, key).unwrap_or_default()
}

struct QuotaHeaders {
    q5h: f64,
    q7d: f64,
    q5h_reset: i64,
    q7d_reset: i64,
    qstatus: String,
    qoverage: String,
    qclaim: String,
    qfallback_pct: f64,
    qoverage_util: Option<f64>,
    qrepresentative_claim: Option<String>,
    org_id_raw: Option<String>,
    overage_disabled_reason: Option<String>,
}

fn parse_quota_headers(headers: &axum::http::HeaderMap) -> QuotaHeaders {
    QuotaHeaders {
        q5h: num_header(headers, "anthropic-ratelimit-unified-5h-utilization").unwrap_or(0.0),
        q7d: num_header(headers, "anthropic-ratelimit-unified-7d-utilization").unwrap_or(0.0),
        q5h_reset: int_header(headers, "anthropic-ratelimit-unified-5h-reset"),
        q7d_reset: int_header(headers, "anthropic-ratelimit-unified-7d-reset"),
        qstatus: str_header(headers, "anthropic-ratelimit-unified-status"),
        qoverage: str_header(headers, "anthropic-ratelimit-unified-overage-status"),
        qclaim: str_header(headers, "anthropic-ratelimit-unified-claim"),
        qfallback_pct: num_header(headers, "anthropic-ratelimit-unified-fallback-percentage")
            .unwrap_or(0.0),
        qoverage_util: num_header(
            headers,
            "anthropic-ratelimit-unified-overage-utilization",
        ),
        qrepresentative_claim: str_header_opt(
            headers,
            "anthropic-ratelimit-unified-representative-claim",
        ),
        org_id_raw: str_header_opt(headers, "anthropic-organization-id"),
        overage_disabled_reason: str_header_opt(
            headers,
            "anthropic-ratelimit-unified-overage-disabled-reason",
        ),
    }
}

// --- Delta computation ---

fn compute_delta(current: f64, previous: Option<f64>) -> f64 {
    match previous {
        None => 0.0,
        Some(prev) => current - prev,
    }
}

// --- Record assembly ---

fn assemble_record(
    start: &MessageStartFields,
    output_tokens: u64,
    quota: &QuotaHeaders,
    requested_model: Option<&str>,
    sid: &str,
    prev_q5h: Option<f64>,
    prev_q7d: Option<f64>,
    now: &chrono::DateTime<chrono::Utc>,
) -> Value {
    let total_input = start.input_tokens
        + start.cache_creation_input_tokens
        + start.cache_read_input_tokens;
    let cache_hit_rate = if total_input > 0 {
        start.cache_read_input_tokens as f64 / total_input as f64
    } else {
        0.0
    };

    let mut record = json!({
        "v": 1,
        "ts": now.to_rfc3339(),
        "sid": sid,
        "model": start.model,
        "speed": start.speed,
        "service_tier": start.service_tier,
        "input_tokens": start.input_tokens,
        "output_tokens": output_tokens,
        "cache_creation_input_tokens": start.cache_creation_input_tokens,
        "cache_read_input_tokens": start.cache_read_input_tokens,
        "ephemeral_1h_input_tokens": start.ephemeral_1h_input_tokens,
        "ephemeral_5m_input_tokens": start.ephemeral_5m_input_tokens,
        "web_search_requests": start.web_search_requests,
        "q5h": quota.q5h,
        "q7d": quota.q7d,
        "q5h_reset": quota.q5h_reset,
        "q7d_reset": quota.q7d_reset,
        "qstatus": quota.qstatus,
        "qoverage": quota.qoverage,
        "qclaim": quota.qclaim,
        "qfallback_pct": quota.qfallback_pct,
        "cache_hit_rate": cache_hit_rate,
        "q5h_delta": compute_delta(quota.q5h, prev_q5h),
        "q7d_delta": compute_delta(quota.q7d, prev_q7d),
    });

    let map = record.as_object_mut().unwrap();

    // Optional: requested_model + model_mismatch
    if let Some(req_model) = requested_model {
        if !req_model.is_empty() {
            map.insert("requested_model".to_string(), json!(req_model));
            if !start.model.is_empty() && req_model != start.model {
                map.insert("model_mismatch".to_string(), json!(true));
            }
        }
    }

    // Optional: qoverage_util
    if let Some(qoverage_util) = quota.qoverage_util {
        map.insert("qoverage_util".to_string(), json!(qoverage_util));
    }

    // Optional: qrepresentative_claim
    if let Some(ref claim) = quota.qrepresentative_claim {
        if !claim.is_empty() {
            map.insert("qrepresentative_claim".to_string(), json!(claim));
        }
    }

    // Optional: org_id (hashed)
    if let Some(hashed) = hash_org_id(quota.org_id_raw.as_deref()) {
        map.insert("org_id".to_string(), json!(hashed));
    }

    // Optional: overage_disabled_reason
    if let Some(ref reason) = quota.overage_disabled_reason {
        if !reason.is_empty() {
            map.insert("overage_disabled_reason".to_string(), json!(reason));
        }
    }

    record
}

// --- File I/O ---

fn append_jsonl(record: &Value) -> Result<(), ExtensionError> {
    let path = log_path();
    let parent = std::path::Path::new(&path)
        .parent()
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| ExtensionError::io("usage-log", e))?;
    let line =
        serde_json::to_string(record).map_err(|e| ExtensionError::json("usage-log", e.to_string()))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| ExtensionError::io("usage-log", e))?;
    writeln!(file, "{}", line).map_err(|e| ExtensionError::io("usage-log", e))?;
    Ok(())
}

// --- StreamExtension ---

impl StreamExtension for UsageLog {
    fn on_stream_event(
        &self,
        ctx: &mut StreamEventContext,
    ) -> Result<(), ExtensionError> {
        // message_start: capture per-response state into meta.
        if ctx.event_type == "message_start" {
            if let Some(start) = extract_message_start_fields(&ctx.data) {
                ctx.meta.set(
                    "_usageLog",
                    json!({
                        "start": {
                            "model": start.model,
                            "speed": start.speed,
                            "service_tier": start.service_tier,
                            "input_tokens": start.input_tokens,
                            "cache_creation_input_tokens": start.cache_creation_input_tokens,
                            "cache_read_input_tokens": start.cache_read_input_tokens,
                            "ephemeral_1h_input_tokens": start.ephemeral_1h_input_tokens,
                            "ephemeral_5m_input_tokens": start.ephemeral_5m_input_tokens,
                            "web_search_requests": start.web_search_requests,
                        }
                    }),
                );
            }
            return Ok(());
        }

        // message_delta: assemble and emit the final record.
        if ctx.event_type == "message_delta" {
            let output_tokens = match ctx
                .data
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
            {
                Some(n) => n,
                None => return Ok(()),
            };

            let start_val = match ctx.meta.get("_usageLog").and_then(|v| v.get("start")) {
                Some(s) => s.clone(),
                None => return Ok(()),
            };

            let start = MessageStartFields {
                model: start_val
                    .get("model")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                speed: start_val
                    .get("speed")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                service_tier: start_val
                    .get("service_tier")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                input_tokens: start_val
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cache_creation_input_tokens: start_val
                    .get("cache_creation_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                cache_read_input_tokens: start_val
                    .get("cache_read_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                ephemeral_1h_input_tokens: start_val
                    .get("ephemeral_1h_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                ephemeral_5m_input_tokens: start_val
                    .get("ephemeral_5m_input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                web_search_requests: start_val
                    .get("web_search_requests")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
            };

            let quota = parse_quota_headers(&ctx.response_headers);
            let sid = get_sid();
            let requested_model = ctx.telemetry.model.as_deref();
            let prev_q5h = get_last_q5h();
            let prev_q7d = get_last_q7d();
            let now = chrono::Utc::now();

            let record = assemble_record(
                &start,
                output_tokens,
                &quota,
                requested_model,
                &sid,
                prev_q5h,
                prev_q7d,
                &now,
            );

            // Update delta tracking AFTER assembly so first call delta is 0.
            set_last_q5h(quota.q5h);
            set_last_q7d(quota.q7d);

            // Fail-open: silently skip I/O errors.
            let _ = append_jsonl(&record);
        }

        Ok(())
    }
}

// --- Test helpers ---

/// Reset module-scope delta state for tests.
pub fn reset_delta_state_for_test() {
    *LAST_Q5H.lock().expect("q5h lock poisoned") = None;
    *LAST_Q7D.lock().expect("q7d lock poisoned") = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_sid_is_8_chars_hex() {
        let sid = generate_sid();
        assert_eq!(sid.len(), 8);
        assert!(sid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_org_id_is_16_chars_hex() {
        let result = hash_org_id(Some("my-org-id"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 16);
    }

    #[test]
    fn hash_org_id_none() {
        assert!(hash_org_id(None).is_none());
        assert!(hash_org_id(Some("")).is_none());
    }

    #[test]
    fn extract_message_start_fields_basic() {
        let event = json!({
            "type": "message_start",
            "message": {
                "model": "claude-sonnet-4-5",
                "usage": {
                    "input_tokens": 100,
                    "output_tokens": 0,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 50,
                    "speed": "standard",
                    "service_tier": ""
                }
            }
        });
        let fields = extract_message_start_fields(&event);
        assert!(fields.is_some());
        let f = fields.unwrap();
        assert_eq!(f.model, "claude-sonnet-4-5");
        assert_eq!(f.input_tokens, 100);
        assert_eq!(f.cache_read_input_tokens, 50);
    }

    #[test]
    fn extract_message_start_fields_rejects_non_message_start() {
        let event = json!({
            "type": "message_delta",
            "usage": { "output_tokens": 42 }
        });
        assert!(extract_message_start_fields(&event).is_none());
    }

    #[test]
    fn extract_message_delta_fields_basic() {
        let event = json!({
            "type": "message_delta",
            "usage": { "output_tokens": 200 }
        });
        assert_eq!(extract_message_delta_fields(&event), Some(200));
    }

    #[test]
    fn extract_message_delta_fields_rejects_non_delta() {
        let event = json!({
            "type": "content_block_delta",
            "delta": { "text": "hello" }
        });
        assert_eq!(extract_message_delta_fields(&event), None);
    }

    #[test]
    fn compute_delta_returns_zero_on_first_call() {
        assert_eq!(compute_delta(0.5, None), 0.0);
    }

    #[test]
    fn compute_delta_with_previous() {
        let delta = compute_delta(0.8, Some(0.5));
        assert!((delta - 0.3).abs() < 0.0001, "expected ~0.3, got {}", delta);
    }

    #[test]
    fn assemble_record_basic() {
        let start = MessageStartFields {
            model: "claude-sonnet-4-5".to_string(),
            speed: "standard".to_string(),
            service_tier: "".to_string(),
            input_tokens: 100,
            cache_creation_input_tokens: 10,
            cache_read_input_tokens: 40,
            ephemeral_1h_input_tokens: 0,
            ephemeral_5m_input_tokens: 0,
            web_search_requests: 0,
        };
        let quota = QuotaHeaders {
            q5h: 0.5,
            q7d: 0.3,
            q5h_reset: 1715702400,
            q7d_reset: 1715788800,
            qstatus: "allowed".to_string(),
            qoverage: "within".to_string(),
            qclaim: "standard".to_string(),
            qfallback_pct: 0.0,
            qoverage_util: None,
            qrepresentative_claim: None,
            org_id_raw: None,
            overage_disabled_reason: None,
        };
        let now = chrono::Utc::now();

        let record = assemble_record(&start, 50, &quota, None, "abc12345", Some(0.4), Some(0.2), &now);

        assert_eq!(record["v"], json!(1));
        assert_eq!(record["model"], json!("claude-sonnet-4-5"));
        assert_eq!(record["output_tokens"], json!(50));
        assert_eq!(record["q5h"], json!(0.5));
        // cache_hit_rate = cache_read / (input + cache_creation + cache_read) = 40/150 ≈ 0.267
        let chr = record["cache_hit_rate"].as_f64().unwrap();
        assert!((chr - 0.2666).abs() < 0.01);
        // q5h_delta = 0.5 - 0.4 = 0.1
        assert!((record["q5h_delta"].as_f64().unwrap() - 0.1).abs() < 0.001);
    }

    #[test]
    fn assemble_record_includes_model_mismatch() {
        let start = MessageStartFields {
            model: "claude-sonnet-4-5".to_string(),
            speed: "".to_string(),
            service_tier: "".to_string(),
            input_tokens: 10,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            ephemeral_1h_input_tokens: 0,
            ephemeral_5m_input_tokens: 0,
            web_search_requests: 0,
        };
        let quota = QuotaHeaders {
            q5h: 0.0,
            q7d: 0.0,
            q5h_reset: 0,
            q7d_reset: 0,
            qstatus: "".to_string(),
            qoverage: "".to_string(),
            qclaim: "".to_string(),
            qfallback_pct: 0.0,
            qoverage_util: None,
            qrepresentative_claim: None,
            org_id_raw: None,
            overage_disabled_reason: None,
        };
        let now = chrono::Utc::now();
        let record = assemble_record(
            &start,
            0,
            &quota,
            Some("anthropic.claude-sonnet-4-5-20250514"),
            "sid",
            None,
            None,
            &now,
        );
        assert_eq!(record["requested_model"], json!("anthropic.claude-sonnet-4-5-20250514"));
        assert_eq!(record["model_mismatch"], json!(true));
    }

    #[test]
    fn extension_implements_traits() {
        let ext = UsageLog::new();
        assert_eq!(ext.name(), "usage-log");
        assert_eq!(ext.order(), 650);
        assert!(!ext.default_enabled());
    }
}
