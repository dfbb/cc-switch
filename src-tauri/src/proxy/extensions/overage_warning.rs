// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/overage-warning.mjs
// 翻译: 2026-05-20
//
// Emit a one-time warning per Q5h-window threshold crossing when Anthropic's
// response headers indicate the user is approaching or has crossed the overage
// threshold.
//
// Advisory only. No request mutation. Two outputs:
//   1. stderr line prefixed [overage-warning] for proxy journals/logs
//   2. structured JSON record appended to ~/.claude/overage-warnings.jsonl
//
// Activation: `enabled: true` in extensions.json (this extension is always
// loaded), gated at runtime by CACHE_FIX_OVERAGE_WARNING=1.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::json;
use std::collections::HashSet;
use std::io::Write;
use std::sync::Mutex;

// --- Constants ---

const WINDOW_MS: i64 = 15 * 60 * 1000;
const WINDOW_MAX_SAMPLES: usize = 60;
const WARM_UP_MIN_SAMPLES: usize = 3;
const WEIGHTED_TOKEN_COST_USD_COARSE: f64 = 0.000005;

// --- Module-scope state ---

static WINDOW: Mutex<Vec<Sample>> = Mutex::new(Vec::new());
static DEDUP_WINDOW_RESETS_AT: Mutex<i64> = Mutex::new(0);
static DEDUP_THRESHOLDS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

#[derive(Debug, Clone, serde::Serialize)]
struct Sample {
    t: i64,
    q5h: f64,
    input: u64,
    cache_creation: u64,
    cache_read: u64,
    output: u64,
}

// --- Data types ---

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Trigger {
    status: String,
    surpassed_threshold: f64,
    overage_status: String,
    upgrade_paths: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Snapshot {
    q5h_pct: Option<i64>,
    q7d_pct: Option<i64>,
    q5h_resets_at: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Projection {
    min_to_100: Option<i64>,
    tokens_per_min: Option<i64>,
    cost_per_hr_usd_coarse: Option<f64>,
    window_samples: usize,
    window_minutes: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct OverageWarningRecord {
    ts: String,
    trigger: Trigger,
    snapshot: Snapshot,
    projection: Projection,
}

// --- Extension struct ---

pub struct OverageWarning;

impl OverageWarning {
    pub fn new() -> Self {
        // Ensure dedup thresholds set is initialized.
        let _ = DEDUP_THRESHOLDS.lock().map(|mut s| {
            if s.is_none() {
                *s = Some(HashSet::new());
            }
        });
        Self
    }
}

impl Extension for OverageWarning {
    fn name(&self) -> &str {
        "overage-warning"
    }
    fn order(&self) -> u32 {
        610
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

// --- Environment helpers ---

fn is_enabled() -> bool {
    std::env::var("CACHE_FIX_OVERAGE_WARNING")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn is_quiet() -> bool {
    std::env::var("CACHE_FIX_OVERAGE_WARNING_QUIET")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn is_debug() -> bool {
    std::env::var("CACHE_FIX_DEBUG")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn debug(msg: &str) {
    if is_debug() {
        eprintln!("[overage-warning] DEBUG: {}", msg);
    }
}

// --- Output path ---

fn output_dir() -> String {
    std::env::var("CACHE_FIX_OVERAGE_WARNING_DIR")
        .unwrap_or_else(|_| format!("{}/.claude", std::env::var("HOME").unwrap_or_default()))
}

// --- Header parsing (pure functions) ---

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

fn str_header(headers: &axum::http::HeaderMap, key: &str) -> String {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

#[derive(Debug, Clone)]
struct HeaderTriggerResult {
    eligible: bool,
    trigger: Option<Trigger>,
    snapshot: Option<Snapshot>,
    raw_q5h_util: Option<f64>,
    raw_q5h_resets_at: i64,
}

fn parse_trigger_from_headers(headers: &axum::http::HeaderMap) -> HeaderTriggerResult {
    let status = if headers.get("anthropic-ratelimit-unified-status").is_some() {
        str_header(headers, "anthropic-ratelimit-unified-status")
    } else {
        str_header(headers, "anthropic-ratelimit-unified-5h-status")
    };

    let surpassed = num_header(headers, "anthropic-ratelimit-unified-7d-surpassed-threshold");
    let overage_status = str_header(headers, "anthropic-ratelimit-unified-overage-status");
    if overage_status.is_empty() {
        // fallback
    }
    let overage_status = if overage_status.is_empty() {
        "unknown".to_string()
    } else {
        overage_status
    };

    let upgrade_paths_raw = str_header(headers, "anthropic-ratelimit-unified-upgrade-paths");
    let q5h_util = num_header(headers, "anthropic-ratelimit-unified-5h-utilization");
    let q7d_util = num_header(headers, "anthropic-ratelimit-unified-7d-utilization");
    let q5h_resets_at = int_header(headers, "anthropic-ratelimit-unified-5h-reset");

    let is_warn = status == "allowed_warning" || status == "throttled";
    if !is_warn {
        return HeaderTriggerResult {
            eligible: false,
            trigger: None,
            snapshot: None,
            raw_q5h_util: q5h_util,
            raw_q5h_resets_at: q5h_resets_at,
        };
    }
    if surpassed.is_none() {
        return HeaderTriggerResult {
            eligible: false,
            trigger: None,
            snapshot: None,
            raw_q5h_util: q5h_util,
            raw_q5h_resets_at: q5h_resets_at,
        };
    }

    let upgrade_paths: Vec<String> = if !upgrade_paths_raw.is_empty() {
        upgrade_paths_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        vec![]
    };

    HeaderTriggerResult {
        eligible: true,
        trigger: Some(Trigger {
            status,
            surpassed_threshold: surpassed.unwrap(),
            overage_status,
            upgrade_paths,
        }),
        snapshot: Some(Snapshot {
            q5h_pct: q5h_util.map(|v| (v * 100.0).round() as i64),
            q7d_pct: q7d_util.map(|v| (v * 100.0).round() as i64),
            q5h_resets_at,
        }),
        raw_q5h_util: q5h_util,
        raw_q5h_resets_at: q5h_resets_at,
    }
}

fn dedup_key(threshold: f64, q5h_resets_at: i64) -> String {
    format!("{}@{}", threshold, q5h_resets_at)
}

// --- Projection computation ---

fn compute_projection(samples: &[Sample], now: i64) -> Projection {
    let cutoff = now - WINDOW_MS;
    let fresh: Vec<&Sample> = samples.iter().filter(|s| s.t >= cutoff).collect();

    if fresh.len() < WARM_UP_MIN_SAMPLES {
        return Projection {
            min_to_100: None,
            tokens_per_min: None,
            cost_per_hr_usd_coarse: None,
            window_samples: fresh.len(),
            window_minutes: 0.0,
        };
    }

    let oldest = fresh[0];
    let newest = fresh[fresh.len() - 1];
    let window_min = (newest.t - oldest.t) as f64 / 60_000.0;

    if window_min <= 0.0 {
        return Projection {
            min_to_100: None,
            tokens_per_min: None,
            cost_per_hr_usd_coarse: None,
            window_samples: fresh.len(),
            window_minutes: 0.0,
        };
    }

    let delta_util = newest.q5h - oldest.q5h;
    let util_per_min = delta_util / window_min;

    let min_to_100 = if util_per_min > 0.0 {
        Some(((1.0 - newest.q5h) / util_per_min).max(0.0).round() as i64)
    } else {
        None
    };

    let total_tokens: u64 = fresh
        .iter()
        .map(|s| s.input + s.cache_creation + s.cache_read + s.output)
        .sum();
    let tokens_per_min = total_tokens as f64 / window_min;
    let cost_per_hr_usd_coarse = if util_per_min > 0.0 {
        Some(
            ((tokens_per_min * 60.0 * WEIGHTED_TOKEN_COST_USD_COARSE) * 100.0).round() / 100.0,
        )
    } else {
        None
    };

    Projection {
        min_to_100,
        tokens_per_min: Some(tokens_per_min.round() as i64),
        cost_per_hr_usd_coarse,
        window_samples: fresh.len(),
        window_minutes: (window_min * 10.0).round() / 10.0,
    }
}

// --- Dedup helpers ---

fn check_and_mark_dedup(threshold: f64, q5h_resets_at: i64) -> bool {
    let mut reset_at = DEDUP_WINDOW_RESETS_AT
        .lock()
        .expect("dedup lock poisoned");
    if q5h_resets_at != *reset_at {
        *reset_at = q5h_resets_at;
        if let Ok(mut set) = DEDUP_THRESHOLDS.lock() {
            *set = Some(HashSet::new());
        }
    }
    drop(reset_at);

    let mut set = DEDUP_THRESHOLDS.lock().expect("dedup set lock poisoned");
    let set = set.get_or_insert_with(HashSet::new);
    let key = dedup_key(threshold, q5h_resets_at);
    if set.contains(&key) {
        return false;
    }
    set.insert(key);
    true
}

// --- Window management ---

fn record_sample(sample: Sample) {
    let mut window = WINDOW.lock().expect("window lock poisoned");
    let cutoff = sample.t - WINDOW_MS;
    window.push(sample);
    while window.first().map_or(false, |s| s.t < cutoff) {
        window.remove(0);
    }
    while window.len() > WINDOW_MAX_SAMPLES {
        window.remove(0);
    }
}

fn get_window_snapshot() -> Vec<Sample> {
    WINDOW.lock().expect("window lock poisoned").clone()
}

// --- Stderr formatting ---

fn format_stderr_line(
    ts: &str,
    trigger: &Trigger,
    snapshot: &Snapshot,
    projection: Option<&Projection>,
) -> String {
    let upgrade = if trigger.upgrade_paths.is_empty() {
        "(none)".to_string()
    } else {
        trigger.upgrade_paths.join(", ")
    };
    let head = format!(
        "[overage-warning] {} Q5h={}% Q7d={}% (surpassed {})",
        ts,
        snapshot.q5h_pct.unwrap_or(-1),
        snapshot.q7d_pct.unwrap_or(-1),
        trigger.surpassed_threshold
    );
    if let Some(proj) = projection {
        if proj.min_to_100.is_some() && proj.cost_per_hr_usd_coarse.is_some() {
            return format!(
                "{} — projected 100% in ~{} min, estimated continued burn ≈ ${:.2}/hr at API rates (coarse). Upgrade paths: {}.",
                head,
                proj.min_to_100.unwrap(),
                proj.cost_per_hr_usd_coarse.unwrap(),
                upgrade
            );
        }
    }
    format!(
        "{} — projection unavailable (warming up). Upgrade paths: {}.",
        head, upgrade
    )
}

// --- JSONL record formatting ---

fn format_jsonl_record(
    ts: &str,
    trigger: &Trigger,
    snapshot: &Snapshot,
    projection: &Projection,
) -> OverageWarningRecord {
    OverageWarningRecord {
        ts: ts.to_string(),
        trigger: Trigger {
            status: trigger.status.clone(),
            surpassed_threshold: trigger.surpassed_threshold,
            overage_status: trigger.overage_status.clone(),
            upgrade_paths: trigger.upgrade_paths.clone(),
        },
        snapshot: Snapshot {
            q5h_pct: snapshot.q5h_pct,
            q7d_pct: snapshot.q7d_pct,
            q5h_resets_at: snapshot.q5h_resets_at,
        },
        projection: Projection {
            min_to_100: projection.min_to_100,
            tokens_per_min: projection.tokens_per_min,
            cost_per_hr_usd_coarse: projection.cost_per_hr_usd_coarse,
            window_samples: projection.window_samples,
            window_minutes: projection.window_minutes,
        },
    }
}

// --- File I/O ---

fn append_jsonl(record: &OverageWarningRecord) -> Result<(), ExtensionError> {
    let dir = output_dir();
    let out_path = format!("{}/overage-warnings.jsonl", dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| ExtensionError::io("overage-warning", e))?;
    let line =
        serde_json::to_string(record).map_err(|e| ExtensionError::json("overage-warning", e.to_string()))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&out_path)
        .map_err(|e| ExtensionError::io("overage-warning", e))?;
    writeln!(file, "{}", line).map_err(|e| ExtensionError::io("overage-warning", e))?;
    Ok(())
}

// --- ResponseExtension ---

impl ResponseExtension for OverageWarning {
    fn on_response_start(
        &self,
        ctx: &mut ResponseStartContext,
    ) -> Result<(), ExtensionError> {
        if !is_enabled() {
            return Ok(());
        }

        let q5h_raw = ctx
            .upstream_headers
            .get("anthropic-ratelimit-unified-5h-utilization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<f64>().ok());

        if let Some(q5h_util) = q5h_raw {
            ctx.meta.set(
                "_overageQuota",
                json!({"q5h_util": q5h_util}),
            );
        }

        let result = parse_trigger_from_headers(&ctx.upstream_headers);
        if !result.eligible {
            return Ok(());
        }

        ctx.meta.set(
            "_overageWarning",
            json!({
                "eligible": true,
                "emitted": false,
                "trigger": result.trigger,
                "snapshot": result.snapshot,
                "raw": {
                    "q5h_util": result.raw_q5h_util,
                    "q5h_resets_at": result.raw_q5h_resets_at,
                },
            }),
        );

        Ok(())
    }
}

// --- StreamExtension ---

impl StreamExtension for OverageWarning {
    fn on_stream_event(
        &self,
        ctx: &mut StreamEventContext,
    ) -> Result<(), ExtensionError> {
        if !is_enabled() {
            return Ok(());
        }

        // Sample collection on message_start with usage.
        if ctx.event_type == "message_start" {
            if let Some(usage) = ctx.data.get("message").and_then(|m| m.get("usage")) {
                let q5h_util = ctx
                    .meta
                    .get("_overageQuota")
                    .and_then(|v| v.get("q5h_util"))
                    .and_then(|v| v.as_f64());

                if let Some(q5h_val) = q5h_util {
                    let sample = Sample {
                        t: chrono::Utc::now().timestamp_millis(),
                        q5h: q5h_val,
                        input: usage
                            .get("input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        cache_creation: usage
                            .get("cache_creation_input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        cache_read: usage
                            .get("cache_read_input_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        output: 0,
                    };
                    record_sample(sample.clone());
                    ctx.meta.set("_overageSample", serde_json::to_value(&sample).unwrap_or_default());
                }
            }
        }

        // Emission gate on message_delta.
        if ctx.event_type == "message_delta" {
            // Update THIS response's sample with output tokens.
            if let Some(output_tokens) = ctx
                .data
                .get("usage")
                .and_then(|u| u.get("output_tokens"))
                .and_then(|v| v.as_u64())
            {
                if let Some(mut sample_val) = ctx.meta.get("_overageSample").cloned() {
                    if let Some(output) = sample_val.get_mut("output") {
                        let current = output.as_u64().unwrap_or(0);
                        *output = json!(current + output_tokens);
                        ctx.meta.set("_overageSample", sample_val);
                    }
                }
            }

            let warning_val = ctx.meta.get("_overageWarning");
            if warning_val.is_none() {
                return Ok(());
            }

            let w = warning_val.unwrap();
            let eligible = w.get("eligible").and_then(|v| v.as_bool()).unwrap_or(false);
            let emitted = w.get("emitted").and_then(|v| v.as_bool()).unwrap_or(false);

            if !eligible || emitted {
                return Ok(());
            }

            let trigger: Option<Trigger> = w
                .get("trigger")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let snapshot: Option<Snapshot> = w
                .get("snapshot")
                .and_then(|v| serde_json::from_value(v.clone()).ok());

            let (trigger, snapshot) = match (trigger, snapshot) {
                (Some(t), Some(s)) => (t, s),
                _ => return Ok(()),
            };

            let allowed = check_and_mark_dedup(
                trigger.surpassed_threshold,
                snapshot.q5h_resets_at,
            );
            if !allowed {
                // Mark as emitted in meta.
                if let Some(mut w_val) = ctx.meta.get("_overageWarning").cloned() {
                    w_val["emitted"] = json!(true);
                    ctx.meta.set("_overageWarning", w_val);
                }
                return Ok(());
            }

            let ts = chrono::Utc::now().to_rfc3339();
            let window_snap = get_window_snapshot();
            let now_ms = chrono::Utc::now().timestamp_millis();
            let projection = compute_projection(&window_snap, now_ms);

            let projection_for_output = if projection.window_samples >= WARM_UP_MIN_SAMPLES
                && projection.min_to_100.is_some()
            {
                Some(projection.clone())
            } else {
                None
            };

            let record = format_jsonl_record(
                &ts,
                &trigger,
                &snapshot,
                projection_for_output.as_ref().unwrap_or(&projection),
            );

            if !is_quiet() {
                let line = format_stderr_line(
                    &ts,
                    &trigger,
                    &snapshot,
                    projection_for_output.as_ref(),
                );
                eprintln!("{}", line);
            }

            append_jsonl(&record)?;

            // Mark as emitted in meta.
            if let Some(mut w_val) = ctx.meta.get("_overageWarning").cloned() {
                w_val["emitted"] = json!(true);
                ctx.meta.set("_overageWarning", w_val);
            }
        }

        Ok(())
    }
}

// --- Test helpers (exposed for integration tests) ---

/// Reset module-scope state. For deterministic tests.
pub fn reset_for_test() {
    if let Ok(mut w) = WINDOW.lock() {
        w.clear();
    }
    if let Ok(mut r) = DEDUP_WINDOW_RESETS_AT.lock() {
        *r = 0;
    }
    if let Ok(mut s) = DEDUP_THRESHOLDS.lock() {
        *s = Some(HashSet::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_key_is_deterministic() {
        let a = dedup_key(0.85, 1715702400);
        let b = dedup_key(0.85, 1715702400);
        assert_eq!(a, b);
    }

    #[test]
    #[serial_test::serial]
    fn check_and_mark_dedup_returns_true_once() {
        reset_for_test();
        assert!(check_and_mark_dedup(0.85, 1000));
        assert!(!check_and_mark_dedup(0.85, 1000));
    }

    #[test]
    #[serial_test::serial]
    fn check_and_mark_dedup_resets_on_new_window() {
        reset_for_test();
        assert!(check_and_mark_dedup(0.85, 1000));
        assert!(check_and_mark_dedup(0.85, 2000)); // new window
    }

    #[test]
    fn compute_projection_returns_null_with_few_samples() {
        let samples = vec![Sample {
            t: 1000,
            q5h: 0.5,
            input: 100,
            cache_creation: 0,
            cache_read: 0,
            output: 50,
        }];
        let proj = compute_projection(&samples, 2000);
        assert!(proj.min_to_100.is_none());
        assert!(proj.tokens_per_min.is_none());
    }

    #[test]
    fn compute_projection_with_enough_data() {
        let t0 = 100000;
        let samples: Vec<Sample> = (0..5)
            .map(|i| Sample {
                t: t0 + i * 60_000, // 1 min apart
                q5h: 0.5 + (i as f64) * 0.1,
                input: 1000,
                cache_creation: 0,
                cache_read: 500,
                output: 200,
            })
            .collect();
        let proj = compute_projection(&samples, t0 + 5 * 60_000);
        assert!(proj.window_samples >= WARM_UP_MIN_SAMPLES);
        assert!(proj.tokens_per_min.is_some());
    }

    #[test]
    fn extension_implements_traits() {
        let ext = OverageWarning::new();
        assert_eq!(ext.name(), "overage-warning");
        assert_eq!(ext.order(), 610);
        assert!(ext.default_enabled());
    }
}
