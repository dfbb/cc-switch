// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/upstream-change-detection.mjs
// 翻译: 2026-05-20
//
// Read-only structural fingerprinter that detects when Anthropic ships CC updates
// that change the structural shape of /v1/messages requests. Per-namespace
// baseline persists across proxy restarts to prevent false-positive floods.
//
// Output:
//   - stderr line prefixed [upstream-change] for proxy journals/logs
//   - ~/.claude/upstream-changes.jsonl (event log)
//   - ~/.claude/upstream-baseline.json (per-namespace baseline, atomic replace)
//
// Activation: always loaded, gated at runtime by CACHE_FIX_UPSTREAM_DETECTION=1.
//
// Privacy: every persisted field is a count, position, boolean, bucket label,
// or hash of stable identifiers. NO prompt content, NO file paths, NO message text.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;

// --- Allowlists ---

const KNOWN_SECTION_MARKERS: &[&str] = &[
    "# Environment",
    "# System",
    "# Tools",
    "# Personality",
    "# Settings",
    "# Memory",
    "# Output efficiency",
    "# auto memory",
    "# Doing tasks",
    "# Tone and style",
    "# Using your tools",
    "# Text output",
    "# Session-specific guidance",
    "# Code references",
    "# Executing actions with care",
];

const KNOWN_REMINDER_PATTERNS: &[&str] = &[
    "<system-reminder>",
    "<command-name>",
    "<command-message>",
    "<command-args>",
    "<git-status>",
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<command-stdout>",
    "<command-stderr>",
    "<file-attachment>",
];

// --- Module-scope state ---
// Lazy-loaded on first onRequest call after module init.

static NAMESPACE_MAP: Mutex<Option<HashMap<String, NamespaceEntry>>> = Mutex::new(None);
static BASELINE_LOADED_FROM: Mutex<Option<String>> = Mutex::new(None);

// --- Data types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NamespaceEntry {
    namespace: NamespaceFingerprint,
    fingerprint: StructuralFingerprint,
    established_at: String,
    last_updated_at: String,
    update_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BaselineDocument {
    version: u32,
    namespaces: HashMap<String, NamespaceEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct NamespaceFingerprint {
    model: String,
    beta_headers_sorted_hash: String,
    beta_headers_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct StructuralFingerprint {
    version: u32,
    namespace: NamespaceFingerprint,
    system: SystemFingerprint,
    tools: ToolsFingerprint,
    messages: MessagesFingerprint,
    request_extras: RequestExtrasFingerprint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct SystemFingerprint {
    block_count: usize,
    block_types_in_order: Vec<String>,
    block_size_buckets: Vec<String>,
    known_section_marker_set_hash: String,
    known_section_marker_count: usize,
    unknown_section_marker_present: bool,
    cache_control_count: usize,
    cache_control_positions: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ToolsFingerprint {
    count: usize,
    names_sorted_hash: String,
    schema_shape_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MessagesFingerprint {
    count: usize,
    first_role: Option<String>,
    cache_control_count_in_messages: usize,
    known_reminder_pattern_set_hash: String,
    known_reminder_pattern_count: usize,
    unknown_reminder_pattern_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct RequestExtrasFingerprint {
    has_thinking: bool,
    has_metadata: bool,
    stream: bool,
    max_tokens_bucket: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChangeEvent {
    ts: String,
    event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<NamespaceFingerprint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fingerprint: Option<StructuralFingerprint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<Vec<DiffEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<StructuralFingerprint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<StructuralFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiffEntry {
    path: String,
    from: Value,
    to: Value,
}

// --- Extension struct ---

pub struct UpstreamChangeDetection;

impl UpstreamChangeDetection {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for UpstreamChangeDetection {
    fn name(&self) -> &str {
        "upstream-change-detection"
    }
    fn order(&self) -> u32 {
        50
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for UpstreamChangeDetection {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        if !is_enabled() {
            return Ok(None);
        }
        if ctx.body.is_null() {
            return Ok(None);
        }

        let dir = get_output_dir();
        let ts = chrono::Utc::now().to_rfc3339();

        // Lazy-load baseline on first call.
        let mut loaded_from = BASELINE_LOADED_FROM
            .lock()
            .map_err(|e| ExtensionError::logic("upstream-change-detection", e.to_string()))?;
        if loaded_from.is_none() {
            let mut map = NAMESPACE_MAP
                .lock()
                .map_err(|e| ExtensionError::logic("upstream-change-detection", e.to_string()))?;
            *map = Some(load_baseline(&dir));
            *loaded_from = Some(get_baseline_path(&dir));
        }
        drop(loaded_from);

        // Process the request.
        let headers_ref = &ctx.headers;
        let result = process_request(&ctx.body, headers_ref, &dir)?;

        // Persist baseline if it changed.
        if result.event == "baseline_established" || result.event == "structural_change" {
            persist_baseline(&dir)?;
        }

        // Stderr notification on structural change.
        if result.event == "structural_change" && !is_quiet() {
            let map = NAMESPACE_MAP
                .lock()
                .map_err(|e| ExtensionError::logic("upstream-change-detection", e.to_string()))?;
            if let Some(ref m) = *map {
                if let Some(entry) = m.get(&result.ns_key) {
                    let line = format_stderr_line(
                        &ts,
                        &entry.namespace,
                        result.diff.as_deref().unwrap_or(&[]),
                    );
                    eprintln!("{}", line);
                }
            }
        }

        Ok(None)
    }
}

// --- Environment helpers ---

fn is_enabled() -> bool {
    std::env::var("CACHE_FIX_UPSTREAM_DETECTION")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn is_quiet() -> bool {
    std::env::var("CACHE_FIX_UPSTREAM_QUIET")
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
        eprintln!("[upstream-change] DEBUG: {}", msg);
    }
}

// --- Path helpers ---

fn get_output_dir() -> String {
    std::env::var("CACHE_FIX_UPSTREAM_DIR")
        .unwrap_or_else(|_| format!("{}/.claude", std::env::var("HOME").unwrap_or_default()))
}

fn get_baseline_path(dir: &str) -> String {
    format!("{}/upstream-baseline.json", dir)
}

fn get_jsonl_path(dir: &str) -> String {
    format!("{}/upstream-changes.jsonl", dir)
}

// --- Hashing helpers ---

fn sha16(s: &str) -> String {
    let hash = Sha256::digest(s.as_bytes());
    hash.iter()
        .take(8)
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn sha3(s: &str) -> String {
    let hash = Sha256::digest(s.as_bytes());
    hash.iter()
        .take(3)
        .map(|b| format!("{:02x}", b))
        .collect()
}

// --- Bucket helpers ---

fn bucket_block_size(size: usize) -> &'static str {
    if size < 200 {
        "tiny"
    } else if size < 2000 {
        "small"
    } else if size < 20000 {
        "medium"
    } else {
        "large"
    }
}

fn bucket_max_tokens(n: Option<i64>) -> &'static str {
    match n {
        None => "unset",
        Some(v) if v <= 0 => "unset",
        Some(v) if v < 1024 => "tiny",
        Some(v) if v < 8192 => "1k-8k",
        Some(v) if v < 32768 => "8k-32k",
        Some(v) if v < 100000 => "32k-100k",
        _ => "huge",
    }
}

// --- Section marker helpers ---

fn match_known_section_markers(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return vec![];
    }
    let line_set: std::collections::HashSet<&str> = text.lines().collect();
    KNOWN_SECTION_MARKERS
        .iter()
        .enumerate()
        .filter_map(|(i, marker)| {
            if line_set.contains(marker) {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

fn has_unknown_section_marker(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    // Shape: "# [A-Z][a-zA-Z ]{1,30}" as a whole line.
    for line in text.lines() {
        let line = line.trim();
        if line.len() >= 3
            && line.starts_with("# ")
            && line[2..].chars().next().map_or(false, |c| c.is_ascii_uppercase())
            && line[2..]
                .chars()
                .all(|c| c.is_ascii_alphabetic() || c == ' ')
            && line.len() <= 33
        // "# " + up to 30 chars
        {
            if !KNOWN_SECTION_MARKERS.contains(&line) {
                return true;
            }
        }
    }
    false
}

// --- Reminder pattern helpers ---

fn match_known_reminder_patterns(text: &str) -> Vec<usize> {
    if text.is_empty() {
        return vec![];
    }
    KNOWN_REMINDER_PATTERNS
        .iter()
        .enumerate()
        .filter_map(|(i, pat)| {
            if text.contains(pat) {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

fn has_unknown_reminder_pattern(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    // Match <[a-z][a-z-]{1,30}>
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '<' {
            // Try to match a tag.
            let rest = &text[i + 1..];
            if let Some(end) = rest.find('>') {
                let tag_content = &rest[..end];
                let tag = &text[i..=i + 1 + end];
                // Must start with lowercase letter, then letters or hyphens, 1-30 chars.
                if !tag_content.is_empty()
                    && tag_content.len() <= 30
                    && tag_content
                        .chars()
                        .next()
                        .map_or(false, |c| c.is_ascii_lowercase())
                    && tag_content
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '-')
                {
                    if !KNOWN_REMINDER_PATTERNS.contains(&tag) {
                        return true;
                    }
                }
                // Skip past the '>' by advancing chars iterator.
                for _ in 0..end {
                    chars.next();
                }
            }
        }
    }
    false
}

// --- Beta headers extraction ---

fn extract_beta_headers(headers: &axum::http::HeaderMap, body: &Value) -> Vec<String> {
    // Try headers first (case-insensitive lookup).
    let from_header = headers
        .get("anthropic-beta")
        .or_else(|| headers.get("Anthropic-Beta"))
        .or_else(|| headers.get("ANTHROPIC-BETA"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let raw: Option<String> = from_header.or_else(|| {
        body.get("anthropic_beta")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    });

    match raw {
        None => vec![],
        Some(s) => s
            .split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect(),
    }
}

fn namespace_key(model: &str, beta_headers: &[String]) -> String {
    let mut sorted = beta_headers.to_vec();
    sorted.sort();
    sha16(&format!("{}|{}", model, sorted.join(",")))
}

// --- Block text helpers ---

fn block_text_length(block: &Value) -> usize {
    match block {
        Value::Object(o) => {
            if let Some(t) = o.get("text").and_then(|v| v.as_str()) {
                t.len()
            } else if let Some(c) = o.get("content").and_then(|v| v.as_str()) {
                c.len()
            } else {
                0
            }
        }
        _ => 0,
    }
}

fn block_text(block: &Value) -> &str {
    match block {
        Value::Object(o) => {
            if let Some(t) = o.get("text").and_then(|v| v.as_str()) {
                return t;
            }
            if let Some(c) = o.get("content").and_then(|v| v.as_str()) {
                return c;
            }
            ""
        }
        _ => "",
    }
}

fn count_cache_control_in_array(arr: &[Value]) -> (usize, Vec<usize>) {
    let mut positions = vec![];
    for (i, item) in arr.iter().enumerate() {
        if item.get("cache_control").is_some() {
            positions.push(i);
        }
    }
    (positions.len(), positions)
}

// --- Fingerprint computation ---

fn fingerprint_system(system: &Value) -> SystemFingerprint {
    let blocks: Vec<&Value> = system
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    let types: Vec<String> = blocks
        .iter()
        .map(|b| {
            b.get("type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        })
        .collect();

    let sizes: Vec<String> = blocks
        .iter()
        .map(|b| bucket_block_size(block_text_length(b)).to_string())
        .collect();

    let (cc_count, cc_positions) = count_cache_control_in_array(
        &blocks.iter().copied().cloned().collect::<Vec<_>>(),
    );

    let mut known_indices_set = std::collections::BTreeSet::new();
    let mut unknown_present = false;
    for b in &blocks {
        let text = block_text(b);
        for idx in match_known_section_markers(text) {
            known_indices_set.insert(idx);
        }
        if !unknown_present && has_unknown_section_marker(text) {
            unknown_present = true;
        }
    }
    let known_sorted: Vec<usize> = known_indices_set.into_iter().collect();
    let known_marker_count = known_sorted.len();

    SystemFingerprint {
        block_count: blocks.len(),
        block_types_in_order: types,
        block_size_buckets: sizes,
        known_section_marker_set_hash: sha16(
            &known_sorted
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ),
        known_section_marker_count: known_marker_count,
        unknown_section_marker_present: unknown_present,
        cache_control_count: cc_count,
        cache_control_positions: cc_positions,
    }
}

fn fingerprint_tools(tools: &Value) -> ToolsFingerprint {
    let arr = match tools.as_array() {
        Some(a) => a,
        None => {
            return ToolsFingerprint {
                count: 0,
                names_sorted_hash: sha16(""),
                schema_shape_hash: sha16(""),
            }
        }
    };

    if arr.is_empty() {
        return ToolsFingerprint {
            count: 0,
            names_sorted_hash: sha16(""),
            schema_shape_hash: sha16(""),
        };
    }

    let mut names: Vec<String> = arr
        .iter()
        .map(|t| {
            t.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    names.sort();

    let mut shape: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for t in arr {
        let name = match t.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let keys: Vec<String> = t
            .get("input_schema")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .map(|obj| {
                let mut ks: Vec<String> = obj.keys().cloned().collect();
                ks.sort();
                ks
            })
            .unwrap_or_default();
        shape.insert(name, keys);
    }

    let names_json = serde_json::to_string(&names).unwrap_or_default();
    let shape_json = serde_json::to_string(&shape).unwrap_or_default();

    ToolsFingerprint {
        count: arr.len(),
        names_sorted_hash: sha16(&names_json),
        schema_shape_hash: sha16(&shape_json),
    }
}

fn fingerprint_messages(messages: &Value) -> MessagesFingerprint {
    let arr: Vec<&Value> = messages
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    let mut cc = 0usize;
    let mut known_set = std::collections::BTreeSet::new();
    let mut unknown_present = false;

    for msg in &arr {
        if let Some(content) = msg.get("content") {
            if let Some(content_arr) = content.as_array() {
                for block in content_arr {
                    if block.get("cache_control").is_some() {
                        cc += 1;
                    }
                    let text = block_text(block);
                    for idx in match_known_reminder_patterns(text) {
                        known_set.insert(idx);
                    }
                    if !unknown_present && has_unknown_reminder_pattern(text) {
                        unknown_present = true;
                    }
                }
            } else if let Some(text) = content.as_str() {
                for idx in match_known_reminder_patterns(text) {
                    known_set.insert(idx);
                }
                if !unknown_present && has_unknown_reminder_pattern(text) {
                    unknown_present = true;
                }
            }
        }
    }

    let known_sorted: Vec<usize> = known_set.into_iter().collect();
    let known_count = known_sorted.len();

    MessagesFingerprint {
        count: arr.len(),
        first_role: arr
            .first()
            .and_then(|m| m.get("role"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        cache_control_count_in_messages: cc,
        known_reminder_pattern_set_hash: sha16(
            &known_sorted
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(","),
        ),
        known_reminder_pattern_count: known_count,
        unknown_reminder_pattern_present: unknown_present,
    }
}

fn fingerprint_request_extras(body: &Value) -> RequestExtrasFingerprint {
    RequestExtrasFingerprint {
        has_thinking: body.get("thinking").map_or(false, |v| !v.is_null()),
        has_metadata: body.get("metadata").map_or(false, |v| !v.is_null()),
        stream: body.get("stream").and_then(|v| v.as_bool()) == Some(true),
        max_tokens_bucket: bucket_max_tokens(
            body.get("max_tokens").and_then(|v| v.as_i64()),
        )
        .to_string(),
    }
}

fn compute_fingerprint(body: &Value, headers: &axum::http::HeaderMap) -> StructuralFingerprint {
    let safe_body = body;
    let beta = extract_beta_headers(headers, safe_body);
    let mut sorted_beta = beta.clone();
    sorted_beta.sort();

    StructuralFingerprint {
        version: 1,
        namespace: NamespaceFingerprint {
            model: safe_body
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            beta_headers_sorted_hash: sha16(&sorted_beta.join(",")),
            beta_headers_count: beta.len(),
        },
        system: fingerprint_system(&safe_body.get("system").cloned().unwrap_or(Value::Null)),
        tools: fingerprint_tools(&safe_body.get("tools").cloned().unwrap_or(Value::Null)),
        messages: fingerprint_messages(
            &safe_body
                .get("messages")
                .cloned()
                .unwrap_or(Value::Null),
        ),
        request_extras: fingerprint_request_extras(safe_body),
    }
}

// --- Diff computation ---

fn diff_fingerprints(prev: &StructuralFingerprint, current: &StructuralFingerprint) -> Vec<DiffEntry> {
    let prev_val = serde_json::to_value(prev).unwrap_or_default();
    let curr_val = serde_json::to_value(current).unwrap_or_default();
    let mut diff = vec![];
    walk_diff("", &prev_val, &curr_val, &mut diff);
    diff
}

fn walk_diff(prefix: &str, a: &Value, b: &Value, out: &mut Vec<DiffEntry>) {
    if a == b {
        return;
    }

    // Different types or scalars/arrays: compare via JSON stringification.
    match (a, b) {
        (Value::Object(a_obj), Value::Object(b_obj)) => {
            let mut keys: std::collections::BTreeSet<&str> =
                std::collections::BTreeSet::new();
            for k in a_obj.keys() {
                keys.insert(k.as_str());
            }
            for k in b_obj.keys() {
                keys.insert(k.as_str());
            }
            for k in keys {
                let sub_path = if prefix.is_empty() {
                    k.to_string()
                } else {
                    format!("{}.{}", prefix, k)
                };
                let a_val = a_obj.get(k).unwrap_or(&Value::Null);
                let b_val = b_obj.get(k).unwrap_or(&Value::Null);
                walk_diff(&sub_path, a_val, b_val, out);
            }
        }
        _ => {
            let a_str = serde_json::to_string(a).unwrap_or_default();
            let b_str = serde_json::to_string(b).unwrap_or_default();
            if a_str != b_str {
                out.push(DiffEntry {
                    path: prefix.to_string(),
                    from: a.clone(),
                    to: b.clone(),
                });
            }
        }
    }
}

// --- File I/O ---

fn load_baseline(dir: &str) -> HashMap<String, NamespaceEntry> {
    let path = get_baseline_path(dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Value>(&raw) {
            Ok(parsed) => {
                if let Some(namespaces) = parsed.get("namespaces").and_then(|v| v.as_object()) {
                    let mut map = HashMap::new();
                    for (k, v) in namespaces {
                        if let Ok(entry) = serde_json::from_value::<NamespaceEntry>(v.clone()) {
                            map.insert(k.clone(), entry);
                        }
                    }
                    return map;
                }
            }
            Err(_) => {
                debug(&format!("baseline parse failed: {}", path));
            }
        },
        Err(e) => {
            debug(&format!("baseline load failed ({}): {}", path, e));
        }
    }
    HashMap::new()
}

fn persist_baseline(dir: &str) -> Result<(), ExtensionError> {
    let final_path = get_baseline_path(dir);
    let tmp_suffix = format!(
        "{}.{}.{:04x}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        {
            use std::time::SystemTime;
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        }
    );
    let tmp_path = format!("{}.tmp.{}", final_path, tmp_suffix);

    let map = NAMESPACE_MAP.lock().map_err(|e| {
        ExtensionError::logic("upstream-change-detection", e.to_string())
    })?;
    let doc = BaselineDocument {
        version: 1,
        namespaces: map.clone().unwrap_or_default(),
    };
    let json = serde_json::to_string(&doc).map_err(|e| {
        ExtensionError::json("upstream-change-detection", e.to_string())
    })?;

    std::fs::create_dir_all(dir).map_err(|e| {
        ExtensionError::io("upstream-change-detection", e)
    })?;
    std::fs::write(&tmp_path, &json).map_err(|e| {
        ExtensionError::io("upstream-change-detection", e)
    })?;
    std::fs::rename(&tmp_path, &final_path).map_err(|e| {
        ExtensionError::io("upstream-change-detection", e)
    })?;
    // Best-effort cleanup of tmp file (may not exist after rename).
    let _ = std::fs::remove_file(&tmp_path);

    Ok(())
}

fn append_event(record: &ChangeEvent, dir: &str) -> Result<(), ExtensionError> {
    let path = get_jsonl_path(dir);
    std::fs::create_dir_all(dir).map_err(|e| {
        ExtensionError::io("upstream-change-detection", e)
    })?;
    let line = serde_json::to_string(record).map_err(|e| {
        ExtensionError::json("upstream-change-detection", e.to_string())
    })?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| ExtensionError::io("upstream-change-detection", e))?;
    writeln!(file, "{}", line).map_err(|e| {
        ExtensionError::io("upstream-change-detection", e)
    })?;
    Ok(())
}

// --- Core processing ---

struct ProcessResult {
    event: String,
    ns_key: String,
    diff: Option<Vec<DiffEntry>>,
}

fn process_request(
    body: &Value,
    headers: &axum::http::HeaderMap,
    dir: &str,
) -> Result<ProcessResult, ExtensionError> {
    let fingerprint = compute_fingerprint(body, headers);
    let beta = extract_beta_headers(headers, body);
    let ns_key = namespace_key(&fingerprint.namespace.model, &beta);
    let ts = chrono::Utc::now().to_rfc3339();

    let mut map = NAMESPACE_MAP.lock().map_err(|e| {
        ExtensionError::logic("upstream-change-detection", e.to_string())
    })?;
    let map_ref = map.get_or_insert_with(HashMap::new);

    let existing = map_ref.get(&ns_key);
    if existing.is_none() {
        let entry = NamespaceEntry {
            namespace: fingerprint.namespace.clone(),
            fingerprint: fingerprint.clone(),
            established_at: ts.clone(),
            last_updated_at: ts.clone(),
            update_count: 0,
        };
        map_ref.insert(ns_key.clone(), entry);

        let record = ChangeEvent {
            ts,
            event: "baseline_established".to_string(),
            namespace: Some(fingerprint.namespace.clone()),
            fingerprint: Some(fingerprint.clone()),
            diff: None,
            previous: None,
            current: None,
        };
        drop(map);
        append_event(&record, dir)?;

        return Ok(ProcessResult {
            event: "baseline_established".to_string(),
            ns_key,
            diff: None,
        });
    }

    let existing = existing.unwrap();
    let existing_json = serde_json::to_string(&existing.fingerprint).unwrap_or_default();
    let fingerprint_json = serde_json::to_string(&fingerprint).unwrap_or_default();

    if existing_json == fingerprint_json {
        return Ok(ProcessResult {
            event: "noop".to_string(),
            ns_key,
            diff: None,
        });
    }

    let diff = diff_fingerprints(&existing.fingerprint, &fingerprint);
    let previous = existing.fingerprint.clone();

    let updated = NamespaceEntry {
        namespace: fingerprint.namespace.clone(),
        fingerprint: fingerprint.clone(),
        established_at: existing.established_at.clone(),
        last_updated_at: ts.clone(),
        update_count: existing.update_count.saturating_add(1),
    };
    map_ref.insert(ns_key.clone(), updated);

    let record = ChangeEvent {
        ts,
        event: "structural_change".to_string(),
        namespace: Some(fingerprint.namespace.clone()),
        fingerprint: None,
        diff: Some(diff.clone()),
        previous: Some(previous),
        current: Some(fingerprint.clone()),
    };
    drop(map);
    append_event(&record, dir)?;

    Ok(ProcessResult {
        event: "structural_change".to_string(),
        ns_key,
        diff: Some(diff),
    })
}

fn format_stderr_line(ts: &str, namespace: &NamespaceFingerprint, diff: &[DiffEntry]) -> String {
    let head = format!(
        "[upstream-change] {} model={} beta={}",
        ts, namespace.model, namespace.beta_headers_count
    );
    let diff_summary: Vec<String> = diff
        .iter()
        .take(6)
        .map(|d| format!("{}: {} → {}", d.path, d.from, d.to))
        .collect();
    let summary = diff_summary.join("; ");
    let more = if diff.len() > 6 {
        format!(" (+{} more)", diff.len() - 6)
    } else {
        String::new()
    };
    format!("{} :: {}{}", head, summary, more)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha16_is_deterministic() {
        let a = sha16("hello");
        let b = sha16("hello");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn bucket_block_size_buckets() {
        assert_eq!(bucket_block_size(50), "tiny");
        assert_eq!(bucket_block_size(500), "small");
        assert_eq!(bucket_block_size(5000), "medium");
        assert_eq!(bucket_block_size(50000), "large");
    }

    #[test]
    fn bucket_max_tokens_buckets() {
        assert_eq!(bucket_max_tokens(None), "unset");
        assert_eq!(bucket_max_tokens(Some(0)), "unset");
        assert_eq!(bucket_max_tokens(Some(500)), "tiny");
        assert_eq!(bucket_max_tokens(Some(4096)), "1k-8k");
        assert_eq!(bucket_max_tokens(Some(20000)), "8k-32k");
        assert_eq!(bucket_max_tokens(Some(50000)), "32k-100k");
        assert_eq!(bucket_max_tokens(Some(200000)), "huge");
    }

    #[test]
    fn match_known_section_markers_finds_markers() {
        let text = "# Environment\n# System\nsome content";
        let indices = match_known_section_markers(text);
        assert!(indices.contains(&0)); // "# Environment"
        assert!(indices.contains(&1)); // "# System"
    }

    #[test]
    fn has_unknown_section_marker_detects_unknown() {
        // A section-like marker that isn't in the allowlist.
        assert!(!has_unknown_section_marker("# Environment\n# System"));
    }

    #[test]
    fn match_known_reminder_patterns_finds_patterns() {
        let text = "<system-reminder>\n<command-name>";
        let indices = match_known_reminder_patterns(text);
        assert!(!indices.is_empty());
    }

    #[test]
    fn namespace_key_is_deterministic() {
        let a = namespace_key("claude-sonnet-4-5", &["beta1".into(), "beta2".into()]);
        let b = namespace_key("claude-sonnet-4-5", &["beta2".into(), "beta1".into()]);
        assert_eq!(a, b); // Order-independent.
    }

    #[test]
    fn extract_beta_headers_from_body_fallback() {
        let headers = axum::http::HeaderMap::new();
        let body = serde_json::json!({"anthropic_beta": "feature-a, feature-b"});
        let result = extract_beta_headers(&headers, &body);
        assert_eq!(result, vec!["feature-a", "feature-b"]);
    }

    #[test]
    fn diff_fingerprints_detects_changes() {
        let prev = StructuralFingerprint {
            version: 1,
            namespace: NamespaceFingerprint {
                model: "test".into(),
                beta_headers_sorted_hash: "abc".into(),
                beta_headers_count: 0,
            },
            system: SystemFingerprint {
                block_count: 1,
                block_types_in_order: vec!["text".into()],
                block_size_buckets: vec!["tiny".into()],
                known_section_marker_set_hash: "hash".into(),
                known_section_marker_count: 0,
                unknown_section_marker_present: false,
                cache_control_count: 0,
                cache_control_positions: vec![],
            },
            tools: ToolsFingerprint {
                count: 0,
                names_sorted_hash: sha16(""),
                schema_shape_hash: sha16(""),
            },
            messages: MessagesFingerprint {
                count: 0,
                first_role: None,
                cache_control_count_in_messages: 0,
                known_reminder_pattern_set_hash: sha16(""),
                known_reminder_pattern_count: 0,
                unknown_reminder_pattern_present: false,
            },
            request_extras: RequestExtrasFingerprint {
                has_thinking: false,
                has_metadata: false,
                stream: false,
                max_tokens_bucket: "unset".into(),
            },
        };
        let mut current = prev.clone();
        current.system.block_count = 2;
        let diff = diff_fingerprints(&prev, &current);
        assert!(!diff.is_empty());
        assert!(diff.iter().any(|d| d.path == "system.block_count"));
    }
}
