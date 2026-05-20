// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/deferred-tools-restore.mjs
// 翻译: 2026-05-20
//
// Persists and restores the deferred-tools attachment block across sessions to
// prevent MCP-reconnect-race cache busts at resume time.
//
// Snapshot key: SHA1("cwd:" + working-directory-path), derived from the system
// prompt's "# Environment" section. Fail-open: if the cwd marker is absent,
// the extension no-ops the request entirely.

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const AVAILABLE_MARKER: &str = "The following deferred tools are now available via ToolSearch";
const UNAVAILABLE_MARKER: &str = "The following deferred tools are no longer available";

const ENV_HEADER_LINE: &str = "# Environment";
const ENV_INTRO_LINE: &str = "You have been invoked in the following environment:";

fn is_skip() -> bool {
    std::env::var("CACHE_FIX_SKIP_DEFERRED_TOOLS_RESTORE")
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
        eprintln!("[deferred-tools-restore] {msg}");
    }
}

fn get_snapshot_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude")
        .join("cache-fix-state")
}

pub struct DeferredToolsRestore;

impl DeferredToolsRestore {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for DeferredToolsRestore {
    fn name(&self) -> &str {
        "deferred-tools-restore"
    }
    fn order(&self) -> u32 {
        350
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for DeferredToolsRestore {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        if is_skip() {
            return Ok(None);
        }

        let body = &ctx.body;

        // 1. Parse cwd from system prompt. No marker → no-op.
        let system = body.get("system");
        let cwd = extract_cwd_from_system(system);
        if cwd.is_none() {
            ctx.meta.set(
                "deferredToolsRestoreStats",
                serde_json::json!({"action": "skipped", "reason": "no-cwd"}),
            );
            return Ok(None);
        }
        let cwd = cwd.unwrap();
        let key = derive_snapshot_key(&cwd);

        // 2. Locate the deferred-tools block in user messages.
        let found = find_deferred_tools_block(&ctx.body);
        if found.is_none() {
            ctx.meta.set(
                "deferredToolsRestoreStats",
                serde_json::json!({"action": "skipped", "reason": "no-block", "key": key}),
            );
            return Ok(None);
        }
        let found = found.unwrap();

        let dir = get_snapshot_dir();
        let has_unavail = found.text.contains(UNAVAILABLE_MARKER);

        if !has_unavail {
            // Clean baseline → persist.
            match persist_deferred_tools(&found.text, &dir, &key) {
                Ok(bytes) => {
                    ctx.meta.set(
                        "deferredToolsRestoreStats",
                        serde_json::json!({"action": "persisted", "bytes": bytes, "key": key}),
                    );
                    if is_debug() {
                        eprintln!(
                            "[deferred-tools-restore] persisted {bytes} bytes (key={key})",
                            bytes = bytes,
                            key = key
                        );
                    }
                }
                Err(_) => {
                    ctx.meta.set(
                        "deferredToolsRestoreStats",
                        serde_json::json!({"action": "skipped", "reason": "persist-failed", "key": key}),
                    );
                }
            }
            return Ok(None);
        }

        // 3. Block has UNAVAILABLE marker → attempt restore.
        let snapshot = match restore_deferred_tools(&dir, &key) {
            Ok(s) => s,
            Err(_) => {
                ctx.meta.set(
                    "deferredToolsRestoreStats",
                    serde_json::json!({"action": "skipped", "reason": "no-snapshot", "key": key}),
                );
                return Ok(None);
            }
        };

        // Strictly-longer guard.
        if snapshot.len() <= found.text.len() {
            ctx.meta.set(
                "deferredToolsRestoreStats",
                serde_json::json!({
                    "action": "skipped",
                    "reason": "snapshot-not-longer",
                    "key": key,
                    "snapshotBytes": snapshot.len(),
                    "currentBytes": found.text.len(),
                }),
            );
            return Ok(None);
        }

        // Substitute: replace the block text in-place.
        let messages = ctx.body["messages"].as_array_mut().unwrap();
        let target_msg = &mut messages[found.msg_idx];
        let content = target_msg["content"].as_array_mut().unwrap();
        content[found.block_idx]["text"] = Value::String(snapshot.clone());

        ctx.meta.set(
            "deferredToolsRestoreStats",
            serde_json::json!({
                "action": "restored",
                "bytes": snapshot.len(),
                "previousBytes": found.text.len(),
                "key": key,
            }),
        );
        if is_debug() {
            eprintln!(
                "[deferred-tools-restore] restored {}→{} bytes at msg[{}].content[{}] (key={})",
                found.text.len(),
                snapshot.len(),
                found.msg_idx,
                found.block_idx,
                key,
            );
        }

        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// CWD extraction (strict structural parser)
// ---------------------------------------------------------------------------

/// Parse all cwd values from a single text block's "# Environment" sections.
/// Uses strict structural matching: # Environment header, intro line, then
/// bullet list with "- Primary working directory: <path>".
fn parse_all_cwds_from_block(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let lines: Vec<&str> = text.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() != ENV_HEADER_LINE {
            i += 1;
            continue;
        }
        // Skip blank lines after header.
        let mut j = i + 1;
        while j < lines.len() && lines[j].trim().is_empty() {
            j += 1;
        }
        if j >= lines.len() {
            i = j;
            continue;
        }
        if lines[j].trim() != ENV_INTRO_LINE {
            i = j + 1;
            continue;
        }
        // Walk the bullet list following the intro line.
        let mut found_in_section = false;
        for k in (j + 1)..lines.len() {
            let trimmed = lines[k].trim_start();
            if lines[k].trim().is_empty() {
                break;
            }
            if !trimmed.starts_with('-') {
                break;
            }
            if let Some(cwd) = trimmed.strip_prefix("- Primary working directory: ") {
                let cwd = cwd.trim().to_string();
                if !cwd.is_empty() {
                    found.push(cwd);
                    found_in_section = true;
                    break;
                }
            }
        }
        if found_in_section {
            i = j + 1;
        } else {
            i += 1;
        }
    }
    found
}

/// Extract the working directory from the system prompt.
/// Accepts: array of content blocks (CC's normal shape), a single string, or null.
/// Returns None if the marker is missing or ambiguous (multiple different cwds).
fn extract_cwd_from_system(system: Option<&Value>) -> Option<String> {
    let system = system?;
    let texts: Vec<String> = if let Some(s) = system.as_str() {
        vec![s.to_string()]
    } else if let Some(blocks) = system.as_array() {
        blocks
            .iter()
            .filter_map(|b| b.get("text").and_then(|v| v.as_str()).map(|t| t.to_string()))
            .collect()
    } else {
        return None;
    };

    let mut seen = std::collections::HashSet::new();
    for t in &texts {
        let matches = parse_all_cwds_from_block(t);
        for m in matches {
            seen.insert(m);
            if seen.len() > 1 {
                return None; // ambiguous → no-op
            }
        }
    }
    if seen.len() == 1 {
        Some(seen.into_iter().next().unwrap())
    } else {
        None
    }
}

fn derive_snapshot_key(cwd: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("cwd:{cwd}").as_bytes());
    let result = hasher.finalize();
    // First 8 bytes = 16 hex chars (same as JS: digest("hex").slice(0, 16)).
    result[..8]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

// ---------------------------------------------------------------------------
// Deferred-tools block finder
// ---------------------------------------------------------------------------

struct FoundBlock {
    msg_idx: usize,
    block_idx: usize,
    text: String,
}

/// Locate the deferred-tools attachment block in body.messages.
/// Only inspects user messages (skips assistant).
fn find_deferred_tools_block(body: &Value) -> Option<FoundBlock> {
    let messages = body.get("messages")?.as_array()?;
    for (m, msg) in messages.iter().enumerate() {
        if msg.get("role").and_then(|v| v.as_str()) != Some("user") {
            continue;
        }
        let content = msg.get("content")?.as_array()?;
        for (i, block) in content.iter().enumerate() {
            if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                    if text.contains(AVAILABLE_MARKER) {
                        return Some(FoundBlock {
                            msg_idx: m,
                            block_idx: i,
                            text: text.to_string(),
                        });
                    }
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Snapshot persistence
// ---------------------------------------------------------------------------

fn snapshot_path(dir: &PathBuf, key: &str) -> PathBuf {
    dir.join(format!("deferred-tools-{key}.txt"))
}

/// Persist a clean deferred-tools block to disk (atomic write pattern).
fn persist_deferred_tools(text: &str, dir: &PathBuf, key: &str) -> Result<usize, String> {
    let path = snapshot_path(dir, key);
    std::fs::create_dir_all(dir).map_err(|e| {
        let msg = format!("mkdir failed: {e}");
        debug(&msg);
        msg
    })?;

    // Atomic write: tmp file + rename.
    let tmp_path = dir.join(format!(
        "deferred-tools-{key}.{pid}.{ts}.{rnd}.tmp",
        key = key,
        pid = std::process::id(),
        ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rnd = rand_suffix(),
    ));

    std::fs::write(&tmp_path, text).map_err(|e| {
        let msg = format!("write failed: {e}");
        debug(&msg);
        msg
    })?;
    std::fs::rename(&tmp_path, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        let msg = format!("rename failed: {e}");
        debug(&msg);
        msg
    })?;

    Ok(text.len())
}

/// Read and validate a snapshot. Returns the snapshot text on success.
fn restore_deferred_tools(dir: &PathBuf, key: &str) -> Result<String, String> {
    let path = snapshot_path(dir, key);
    let snapshot = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() != std::io::ErrorKind::NotFound {
            let msg = format!("snapshot read failed at {}: {e}", path.display());
            debug(&msg);
        }
        e.to_string()
    })?;

    // Validation guards.
    if snapshot.len() < AVAILABLE_MARKER.len() {
        let msg = format!(
            "snapshot rejected (too short: {} bytes) at {}",
            snapshot.len(),
            path.display()
        );
        debug(&msg);
        return Err(msg);
    }
    if !snapshot.contains(AVAILABLE_MARKER) {
        let msg = format!(
            "snapshot rejected (missing AVAILABLE marker) at {}",
            path.display()
        );
        debug(&msg);
        return Err(msg);
    }
    if snapshot.contains(UNAVAILABLE_MARKER) {
        let msg = format!(
            "snapshot rejected (contains UNAVAILABLE marker, not a clean baseline) at {}",
            path.display()
        );
        debug(&msg);
        return Err(msg);
    }

    Ok(snapshot)
}

fn rand_suffix() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:08x}", nanos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cwd_from_system_prompt() {
        let text = "\
# Environment
You have been invoked in the following environment:
- Primary working directory: /home/user/project
- Some other bullet
";
        let cwds = parse_all_cwds_from_block(text);
        assert_eq!(cwds, vec!["/home/user/project"]);
    }

    #[test]
    fn parse_cwd_rejects_fake_marker() {
        // A bare "# Environment" mention without the correct intro line → no cwd.
        let text = "\
# Environment is important
- Primary working directory: /fake/path
";
        let cwds = parse_all_cwds_from_block(text);
        assert!(cwds.is_empty());
    }

    #[test]
    fn parse_cwd_rejects_fake_marker_without_intro() {
        // "# Environment" followed by a non-intro line → rule 2 rejects.
        // The JS parser does NOT detect code fences (it splits by \n),
        // so a code-fenced block with the env header + intro WOULD match.
        // This test covers the actual fail-open case: header present
        // but no intro line following it after blank lines.
        let text = "\
# Environment

Some other text, not the intro line
- Primary working directory: /fake/path
";
        let cwds = parse_all_cwds_from_block(text);
        assert!(cwds.is_empty());
    }

    #[test]
    fn parse_cwd_multiple_sections_same_cwd() {
        let text = "\
# Environment
You have been invoked in the following environment:
- Primary working directory: /home/user/project

# Environment
You have been invoked in the following environment:
- Primary working directory: /home/user/project
";
        let cwds = parse_all_cwds_from_block(text);
        assert_eq!(cwds.len(), 2);
    }

    #[test]
    fn extract_cwd_from_system_ambiguous() {
        let blocks = serde_json::json!([
            {"type": "text", "text": "# Environment\nYou have been invoked in the following environment:\n- Primary working directory: /path/a\n"},
            {"type": "text", "text": "# Environment\nYou have been invoked in the following environment:\n- Primary working directory: /path/b\n"},
        ]);
        let cwd = extract_cwd_from_system(Some(&blocks));
        assert!(cwd.is_none()); // ambiguous
    }

    #[test]
    fn derive_snapshot_key_is_deterministic() {
        let a = derive_snapshot_key("/home/user/project");
        let b = derive_snapshot_key("/home/user/project");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn derive_snapshot_key_differs_for_different_cwd() {
        let a = derive_snapshot_key("/home/user/project-a");
        let b = derive_snapshot_key("/home/user/project-b");
        assert_ne!(a, b);
    }

    #[test]
    fn find_deferred_tools_block_finds_in_user_messages() {
        let body = serde_json::json!({
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "The following deferred tools are now available via ToolSearch: tool1, tool2"}
                ]}
            ]
        });
        let found = find_deferred_tools_block(&body);
        assert!(found.is_some());
        assert_eq!(found.unwrap().msg_idx, 0);
    }

    #[test]
    fn find_deferred_tools_block_skips_assistant() {
        let body = serde_json::json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": "The following deferred tools are now available via ToolSearch..."}
                ]}
            ]
        });
        assert!(find_deferred_tools_block(&body).is_none());
    }
}
