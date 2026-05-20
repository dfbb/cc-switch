// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/prefix-diff.mjs
// 翻译: 2026-05-20
//
// Diagnostic extension for hunting cache-bust sources. Snapshots a small
// projection of the prefix (system prompt + tools + first 5 messages) on
// every request and diffs against the prior snapshot. No request mutation.
//
// Activation: env CACHE_FIX_PREFIXDIFF=1
// Debug: env CACHE_FIX_DEBUG=1

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

pub struct PrefixDiff;

impl PrefixDiff {
    pub fn new() -> Self {
        Self
    }

    /// Called by on_request if enabled.
    fn execute(&self, body: &Value) {
        let _ = snapshot_prefix(body, get_snapshot_dir());
    }
}

impl Extension for PrefixDiff {
    fn name(&self) -> &str {
        "prefix-diff"
    }
    fn order(&self) -> u32 {
        680
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for PrefixDiff {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        if std::env::var("CACHE_FIX_PREFIXDIFF").ok().as_deref() != Some("1") {
            return Ok(None);
        }

        // snapshotPrefix never panics; double-belt try/catch is defense in depth.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.execute(&ctx.body);
        })) {
            Ok(()) => {}
            Err(e) => {
                let msg = if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                debug_log(&format!("onRequest unexpected: {msg}"));
            }
        }

        Ok(None)
    }
}

// ── Environment ──

fn get_snapshot_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("cache-fix-snapshots")
}

fn debug_log(msg: &str) {
    if std::env::var("CACHE_FIX_DEBUG").ok().as_deref() == Some("1") {
        eprintln!("[prefix-diff] {msg}");
    }
}

// ── Hashing helpers ──

fn hex_digest_first_n(data: &[u8], n: usize) -> String {
    let full: String = data.iter().map(|b| format!("{b:02x}")).collect();
    full.chars().take(n).collect()
}

fn compute_session_key(system: &Value) -> String {
    let json = serde_json::to_string(system).unwrap_or_default();
    let truncated: String = json.chars().take(2000).collect();
    let hash = Sha256::digest(truncated.as_bytes());
    hex_digest_first_n(&hash, 12)
}

fn compute_tools_hash(tools: Option<&Value>) -> String {
    let arr = match tools.and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return "none".to_string(),
    };
    if arr.is_empty() {
        return "none".to_string();
    }
    let names: Vec<&str> = arr
        .iter()
        .map(|t| t.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        .collect();
    let json = serde_json::to_string(&names).unwrap_or_default();
    let hash = Sha256::digest(json.as_bytes());
    hex_digest_first_n(&hash, 16)
}

fn compute_system_hash(system: Option<&Value>) -> String {
    let sys = match system {
        Some(s) => s,
        None => return "none".to_string(),
    };
    let json = serde_json::to_string(sys).unwrap_or_default();
    let hash = Sha256::digest(json.as_bytes());
    hex_digest_first_n(&hash, 16)
}

// ── Message truncation ──

/// Project the first 5 user/assistant messages: strip cache_control,
/// truncate text >500 chars with `...[N chars]` marker.
fn truncate_prefix_messages(messages: Option<&Value>) -> Vec<Value> {
    let arr = match messages.and_then(|v| v.as_array()) {
        Some(a) => a,
        None => return vec![],
    };

    arr.iter()
        .take(5)
        .map(|msg| {
            let role = msg.get("role").cloned();
            let content = msg.get("content");

            let cleaned_content = if let Some(content_arr) =
                content.and_then(|v| v.as_array())
            {
                let blocks: Vec<Value> = content_arr
                    .iter()
                    .map(|block| {
                        if !block.is_object() {
                            return block.clone();
                        }
                        let mut cleaned = serde_json::Map::new();
                        // Copy all keys except cache_control.
                        if let Some(obj) = block.as_object() {
                            for (k, v) in obj {
                                if k != "cache_control" {
                                    cleaned.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        // Truncate text if > 500 chars.
                        if let Some(text) = cleaned
                            .get("text")
                            .and_then(|v| v.as_str())
                        {
                            if text.len() > 500 {
                                cleaned.insert(
                                    "text".to_string(),
                                    Value::String(format!(
                                        "{}...[{chars} chars]",
                                        &text[..500],
                                        chars = text.len()
                                    )),
                                );
                            }
                        }
                        Value::Object(cleaned)
                    })
                    .collect();
                Value::Array(blocks)
            } else if let Some(text) = content.and_then(|v| v.as_str()) {
                if text.len() > 500 {
                    Value::String(format!(
                        "{}...[{chars} chars]",
                        &text[..500],
                        chars = text.len()
                    ))
                } else {
                    content.cloned().unwrap_or(Value::Null)
                }
            } else {
                content.cloned().unwrap_or(Value::Null)
            };

            let mut obj = serde_json::Map::new();
            if let Some(r) = role {
                obj.insert("role".to_string(), r);
            }
            obj.insert("content".to_string(), cleaned_content);
            Value::Object(obj)
        })
        .collect()
}

// ── Snapshot ──

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Snapshot {
    timestamp: String,
    #[serde(rename = "messageCount")]
    message_count: usize,
    #[serde(rename = "toolsHash")]
    tools_hash: String,
    #[serde(rename = "systemHash")]
    system_hash: String,
    #[serde(rename = "prefixMessages")]
    prefix_messages: Vec<Value>,
}

fn build_snapshot(body: &Value) -> Option<Snapshot> {
    if body.get("system").is_none() {
        return None;
    }

    let message_count = body
        .get("messages")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    Some(Snapshot {
        timestamp: chrono_now(),
        message_count,
        tools_hash: compute_tools_hash(body.get("tools")),
        system_hash: compute_system_hash(body.get("system")),
        prefix_messages: truncate_prefix_messages(body.get("messages")),
    })
}

// ── Diff ──

#[derive(Debug, Clone, Serialize)]
struct DiffResult {
    timestamp: String,
    #[serde(rename = "prevTimestamp")]
    prev_timestamp: String,
    #[serde(rename = "toolsMatch")]
    tools_match: bool,
    #[serde(rename = "systemMatch")]
    system_match: bool,
    #[serde(rename = "messageCountPrev")]
    message_count_prev: usize,
    #[serde(rename = "messageCountNow")]
    message_count_now: usize,
    #[serde(rename = "prefixDiffs")]
    prefix_diffs: Vec<PrefixDiffEntry>,
}

#[derive(Debug, Clone, Serialize)]
struct PrefixDiffEntry {
    index: usize,
    prev: Value,
    now: Value,
}

fn compute_diff(prev: &Snapshot, current: &Snapshot) -> DiffResult {
    let mut diff = DiffResult {
        timestamp: current.timestamp.clone(),
        prev_timestamp: prev.timestamp.clone(),
        tools_match: prev.tools_hash == current.tools_hash,
        system_match: prev.system_hash == current.system_hash,
        message_count_prev: prev.message_count,
        message_count_now: current.message_count,
        prefix_diffs: vec![],
    };

    let max_idx = prev.prefix_messages.len().max(current.prefix_messages.len());
    for i in 0..max_idx {
        let prev_val = prev.prefix_messages.get(i);
        let now_val = current.prefix_messages.get(i);
        let prev_ser = serde_json::to_string(&prev_val.unwrap_or(&Value::Null)).unwrap_or_default();
        let now_ser = serde_json::to_string(&now_val.unwrap_or(&Value::Null)).unwrap_or_default();
        if prev_ser != now_ser {
            diff.prefix_diffs.push(PrefixDiffEntry {
                index: i,
                prev: prev_val.cloned().unwrap_or(Value::Null),
                now: now_val.cloned().unwrap_or(Value::Null),
            });
        }
    }

    diff
}

fn diff_has_changes(diff: &DiffResult) -> bool {
    !diff.prefix_diffs.is_empty()
        || !diff.tools_match
        || !diff.system_match
        || diff.message_count_prev != diff.message_count_now
}

// ── Atomic file write ──

/// Atomic write: stage to a unique temp file, then rename to final path.
fn atomic_write_json(path: &std::path::Path, obj: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("no parent directory")?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let _ext = path.extension().and_then(|s| s.to_str()).unwrap_or("json");

    let tmp_name = format!(
        "{stem}.{pid}.{ts}.{rnd}.tmp",
        pid = std::process::id(),
        ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        rnd = {
            use std::collections::hash_map::RandomState;
            use std::hash::{BuildHasher, Hasher};
            RandomState::new().build_hasher().finish()
        }
    );
    let tmp_path = parent.join(&tmp_name);

    let json = serde_json::to_string_pretty(obj).map_err(|e| e.to_string())?;
    std::fs::write(&tmp_path, json.as_bytes()).map_err(|e| format!("write tmp: {e}"))?;
    std::fs::rename(&tmp_path, path).map_err(|e| format!("rename: {e}"))?;

    Ok(())
}

// ── Orchestrator ──

#[derive(Debug, Default)]
struct SnapshotResult {
    key: String,
    wrote_snapshot: bool,
    wrote_diff: bool,
}

/// Snapshot the prefix of `body` and diff against the prior snapshot.
/// Never panics — all I/O is gated by try/catch.
fn snapshot_prefix(body: &Value, dir: PathBuf) -> Option<SnapshotResult> {
    let current = build_snapshot(body)?;
    let session_key = compute_session_key(&body["system"]);
    let last_path = dir.join(format!("{session_key}-last.json"));
    let diff_path = dir.join(format!("{session_key}-diff.json"));

    // Ensure directory exists.
    if let Err(e) = std::fs::create_dir_all(&dir) {
        debug_log(&format!("mkdir failed for {}: {e}", dir.display()));
        return Some(SnapshotResult {
            key: session_key,
            wrote_snapshot: false,
            wrote_diff: false,
        });
    }

    // Read prior snapshot if it exists.
    let prev: Option<Snapshot> =
        match std::fs::read_to_string(&last_path) {
            Ok(txt) => match serde_json::from_str(&txt) {
                Ok(s) => Some(s),
                Err(e) => {
                    debug_log(&format!(
                        "prior snapshot unreadable at {}: {e}",
                        last_path.display()
                    ));
                    None
                }
            },
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    debug_log(&format!(
                        "prior snapshot unreadable at {}: {e}",
                        last_path.display()
                    ));
                }
                None
            }
        };

    // Compute and write diff if anything changed.
    let mut wrote_diff = false;
    if let Some(ref prev) = prev {
        let diff = compute_diff(prev, &current);
        if diff_has_changes(&diff) {
            match atomic_write_json(&diff_path, &diff) {
                Ok(()) => {
                    wrote_diff = true;
                    eprintln!(
                        "[prefix-diff] {key}: {diffs} differences, tools={tools}, system={sys}, messages={mc_prev}→{mc_now}",
                        key = session_key,
                        diffs = diff.prefix_diffs.len(),
                        tools = if diff.tools_match { "match" } else { "DIFFER" },
                        sys = if diff.system_match { "match" } else { "DIFFER" },
                        mc_prev = diff.message_count_prev,
                        mc_now = diff.message_count_now,
                    );
                }
                Err(e) => {
                    debug_log(&format!("diff write failed at {}: {e}", diff_path.display()));
                }
            }
        }
    }

    // Always write the new snapshot atomically.
    let mut wrote_snapshot = false;
    match atomic_write_json(&last_path, &current) {
        Ok(()) => {
            wrote_snapshot = true;
        }
        Err(e) => {
            debug_log(&format!(
                "snapshot write failed at {}: {e}",
                last_path.display()
            ));
        }
    }

    Some(SnapshotResult {
        key: session_key,
        wrote_snapshot,
        wrote_diff,
    })
}

// ── Timestamp helper ──

fn chrono_now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use serde_json::json;

    #[test]
    fn compute_session_key_is_deterministic() {
        let system = json!([{"type": "text", "text": "You are a helpful assistant"}]);
        let a = compute_session_key(&system);
        let b = compute_session_key(&system);
        assert_eq!(a, b);
        assert_eq!(a.len(), 12);
    }

    #[test]
    fn compute_session_key_differs_for_different_system() {
        let sys1 = json!([{"type": "text", "text": "System A"}]);
        let sys2 = json!([{"type": "text", "text": "System B"}]);
        assert_ne!(compute_session_key(&sys1), compute_session_key(&sys2));
    }

    #[test]
    fn compute_tools_hash_returns_none_for_empty() {
        assert_eq!(compute_tools_hash(None), "none");
        assert_eq!(compute_tools_hash(Some(&json!([]))), "none");
    }

    #[test]
    fn compute_tools_hash_returns_hash_for_tools() {
        let tools = json!([
            {"name": "read_file", "description": "Reads a file"},
            {"name": "write_file", "description": "Writes a file"}
        ]);
        let hash = compute_tools_hash(Some(&tools));
        assert_eq!(hash.len(), 16);
        assert_ne!(hash, "none");
    }

    #[test]
    fn compute_system_hash_returns_none_for_null() {
        assert_eq!(compute_system_hash(None), "none");
    }

    #[test]
    fn compute_system_hash_returns_hash() {
        let system = json!([{"type": "text", "text": "You are a helpful assistant"}]);
        let hash = compute_system_hash(Some(&system));
        assert_eq!(hash.len(), 16);
        assert_ne!(hash, "none");
    }

    #[test]
    fn truncate_prefix_messages_limits_to_5() {
        let messages = json!([
            {"role": "user", "content": [{"type": "text", "text": "msg1"}]},
            {"role": "user", "content": [{"type": "text", "text": "msg2"}]},
            {"role": "user", "content": [{"type": "text", "text": "msg3"}]},
            {"role": "user", "content": [{"type": "text", "text": "msg4"}]},
            {"role": "user", "content": [{"type": "text", "text": "msg5"}]},
            {"role": "user", "content": [{"type": "text", "text": "msg6"}]},
        ]);
        let result = truncate_prefix_messages(Some(&messages));
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn truncate_prefix_messages_strips_cache_control() {
        let messages = json!([
            {"role": "user", "content": [
                {"type": "text", "text": "hello", "cache_control": {"type": "ephemeral", "ttl": "1h"}}
            ]}
        ]);
        let result = truncate_prefix_messages(Some(&messages));
        let content = &result[0]["content"][0];
        assert!(content.get("cache_control").is_none());
        assert_eq!(content["text"].as_str().unwrap(), "hello");
    }

    #[test]
    fn truncate_prefix_messages_truncates_long_text() {
        let long_text = "a".repeat(600);
        let messages = json!([
            {"role": "user", "content": [
                {"type": "text", "text": &long_text}
            ]}
        ]);
        let result = truncate_prefix_messages(Some(&messages));
        let text = result[0]["content"][0]["text"].as_str().unwrap();
        assert!(text.len() < 600);
        assert!(text.contains("...[600 chars]"));
    }

    #[test]
    fn build_snapshot_returns_none_when_no_system() {
        let body = json!({"messages": []});
        assert!(build_snapshot(&body).is_none());
    }

    #[test]
    fn build_snapshot_works_with_system() {
        let body = json!({
            "system": [{"type": "text", "text": "You are helpful"}],
            "tools": [{"name": "tool1"}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]}
            ]
        });
        let snapshot = build_snapshot(&body).unwrap();
        assert_eq!(snapshot.message_count, 1);
        assert_ne!(snapshot.tools_hash, "none");
        assert_ne!(snapshot.system_hash, "none");
        assert_eq!(snapshot.prefix_messages.len(), 1);
    }

    #[test]
    fn compute_diff_detects_changes() {
        let prev = Snapshot {
            timestamp: "2024-01-01T00:00:00Z".into(),
            message_count: 1,
            tools_hash: "aaaa".into(),
            system_hash: "bbbb".into(),
            prefix_messages: vec![],
        };
        let current = Snapshot {
            timestamp: "2024-01-01T01:00:00Z".into(),
            message_count: 2,
            tools_hash: "cccc".into(),
            system_hash: "dddd".into(),
            prefix_messages: vec![],
        };
        let diff = compute_diff(&prev, &current);
        assert!(diff_has_changes(&diff));
        assert!(!diff.tools_match);
        assert!(!diff.system_match);
        assert_ne!(diff.message_count_prev, diff.message_count_now);
    }

    #[test]
    fn compute_diff_no_changes_when_identical() {
        let snap = Snapshot {
            timestamp: "2024-01-01T00:00:00Z".into(),
            message_count: 1,
            tools_hash: "aaaa".into(),
            system_hash: "bbbb".into(),
            prefix_messages: vec![],
        };
        let diff = compute_diff(&snap, &snap);
        assert!(!diff_has_changes(&diff));
    }

    #[test]
    fn on_request_does_nothing_when_disabled() {
        // Clear env var to ensure disabled.
        std::env::remove_var("CACHE_FIX_PREFIXDIFF");

        let ext = PrefixDiff::new();
        let mut ctx = RequestContext {
            body: json!({}),
            headers: HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let result = ext.on_request(&mut ctx).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn on_request_runs_when_enabled() {
        std::env::set_var("CACHE_FIX_PREFIXDIFF", "1");

        let ext = PrefixDiff::new();
        let mut ctx = RequestContext {
            body: json!({
                "system": [{"type": "text", "text": "test system"}],
                "messages": [],
                "tools": []
            }),
            headers: HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let result = ext.on_request(&mut ctx).unwrap();
        assert!(result.is_none());

        std::env::remove_var("CACHE_FIX_PREFIXDIFF");
    }
}
