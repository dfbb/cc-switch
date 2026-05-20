// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/messages-cache-breakpoint.mjs
// 翻译: 2026-05-20
//
// Inject the missing breakpoint #3 cache_control marker at the boundary between
// Claude Code's auto-injected blocks (hooks, skills, CLAUDE.md, deferred-tools,
// MCP) and the first real user content inside messages[0].
//
// Activation: env CACHE_FIX_INJECT_MESSAGES_BREAKPOINT=1
// Diagnostic dump: env CACHE_FIX_DUMP_MESSAGES_HEAD=<path>
// Order 410 — runs after cache-control-normalize (400).

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// Regex matching "Contents of /...CLAUDE.md" — anchored on absolute-path prefix.
static CLAUDE_MD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Contents of /[^\n]*?CLAUDE\.md").unwrap());

/// Auto-injected block kinds (matches JS AUTO_INJECTED_KINDS set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Hooks,
    Skills,
    ClaudeMd,
    DeferredTools,
    McpResources,
    User,
}

impl BlockKind {
    fn is_auto_injected(self) -> bool {
        matches!(
            self,
            BlockKind::Hooks
                | BlockKind::Skills
                | BlockKind::ClaudeMd
                | BlockKind::DeferredTools
                | BlockKind::McpResources
        )
    }
}

pub struct MessagesCacheBreakpoint;

impl MessagesCacheBreakpoint {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for MessagesCacheBreakpoint {
    fn name(&self) -> &str {
        "messages-cache-breakpoint"
    }
    fn order(&self) -> u32 {
        410
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for MessagesCacheBreakpoint {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let dump_path = std::env::var("CACHE_FIX_DUMP_MESSAGES_HEAD")
            .ok()
            .filter(|s| !s.is_empty());
        let inject = std::env::var("CACHE_FIX_INJECT_MESSAGES_BREAKPOINT")
            .ok()
            .map_or(false, |v| v == "1");

        // Both gates off → no-op.
        if dump_path.is_none() && !inject {
            return Ok(None);
        }

        // Diagnostic dump runs first and is independent of injection.
        if let Some(ref path) = dump_path {
            if let Err(e) = write_diagnostic_dump(&ctx.body, path) {
                debug_log(&format!("dump write failed: {e}"));
            }
        }

        if !inject {
            return Ok(None);
        }

        let stats = inject_messages_breakpoint(&mut ctx.body);
        ctx.meta
            .set("messagesBreakpointStats", serde_json::to_value(&stats).unwrap_or_default());
        emit_stderr_summary(&stats);

        Ok(None)
    }
}

// ── Block classification ──

/// Extract the text content from a content block, if it is a text block.
fn get_block_text(block: &Value) -> Option<&str> {
    if !block.is_object() {
        return None;
    }
    if block.get("type")?.as_str() != Some("text") {
        return None;
    }
    block.get("text")?.as_str()
}

/// Classify a content block as auto-injected or user.
fn classify_block(block: &Value) -> BlockKind {
    let text = match get_block_text(block) {
        Some(t) => t,
        None => return BlockKind::User,
    };

    // Hooks: <system-reminder> + "hook success"
    if text.starts_with("<system-reminder>") && text.contains("hook success") {
        return BlockKind::Hooks;
    }
    // Skills: <system-reminder> + (<available-skills> OR <plugin-skills>)
    if text.starts_with("<system-reminder>")
        && (text.contains("<available-skills>") || text.contains("<plugin-skills>"))
    {
        return BlockKind::Skills;
    }
    // Project CLAUDE.md: <system-reminder> wrapper + absolute-path Contents-of marker
    if text.contains("<system-reminder>") && CLAUDE_MD_RE.is_match(text) {
        return BlockKind::ClaudeMd;
    }
    // Deferred tools: exact <deferred-tools> tag
    if text.contains("<deferred-tools>") {
        return BlockKind::DeferredTools;
    }
    // MCP: either sentinel
    if text.contains("<mcp-resources>") || text.contains("Available MCP servers:") {
        return BlockKind::McpResources;
    }
    BlockKind::User
}

/// Return the LAST index in `content` whose block classifies as auto-injected,
/// or None if no auto-injected block is found.
fn detect_auto_injected_boundary(content: &[Value]) -> Option<usize> {
    let mut last_idx = None;
    for (i, block) in content.iter().enumerate() {
        if classify_block(block).is_auto_injected() {
            last_idx = Some(i);
        }
    }
    last_idx
}

// ── Marker counting ──

/// Count all cache_control markers across system blocks and message content blocks.
fn count_all_cache_control_markers(body: &Value) -> usize {
    let mut n = 0;

    if let Some(system) = body.get("system").and_then(|v| v.as_array()) {
        for block in system {
            if block.is_object() && block.get("cache_control").is_some() {
                n += 1;
            }
        }
    }

    if let Some(messages) = body.get("messages").and_then(|v| v.as_array()) {
        for msg in messages {
            if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
                for block in content {
                    if block.is_object() && block.get("cache_control").is_some() {
                        n += 1;
                    }
                }
            }
        }
    }

    n
}

// ── Stats ──

#[derive(Debug, Clone, serde::Serialize)]
struct InjectionStats {
    enabled: bool,
    injected: bool,
    boundary_idx: i32,
    boundary_block_kind: Option<String>,
    blocks_examined: usize,
    existing_marker_count: usize,
    skip_reason: Option<String>,
}

impl Default for InjectionStats {
    fn default() -> Self {
        Self {
            enabled: true,
            injected: false,
            boundary_idx: -1,
            boundary_block_kind: None,
            blocks_examined: 0,
            existing_marker_count: 0,
            skip_reason: None,
        }
    }
}

// ── Orchestrator ──

/// Pure function: mutates body in-place to inject the breakpoint marker.
/// Returns stats describing what happened.
fn inject_messages_breakpoint(body: &mut Value) -> InjectionStats {
    let mut stats = InjectionStats::default();

    let messages = match body.get("messages").and_then(|v| v.as_array()) {
        Some(ms) => ms,
        None => {
            stats.skip_reason = Some("unexpected_role_or_shape".into());
            return stats;
        }
    };
    if messages.is_empty() {
        stats.skip_reason = Some("unexpected_role_or_shape".into());
        return stats;
    }

    // We need mutable access to messages[0].content, so we work with the mutable body.
    let first = &messages[0];
    if first.get("role").and_then(|v| v.as_str()) != Some("user") {
        stats.skip_reason = Some("unexpected_role_or_shape".into());
        return stats;
    }

    let content_arr = match first.get("content").and_then(|v| v.as_array()) {
        Some(c) => c,
        None => {
            stats.skip_reason = Some("unexpected_role_or_shape".into());
            return stats;
        }
    };

    let existing_markers = count_all_cache_control_markers(body);
    stats.existing_marker_count = existing_markers;

    if existing_markers == 0 {
        stats.skip_reason = Some("no_existing_markers".into());
        return stats;
    }
    if existing_markers >= 4 {
        stats.skip_reason = Some("at_marker_limit".into());
        if existing_markers > 4 {
            eprintln!(
                "[messages-breakpoint] warn: existing_markers={existing_markers} exceeds Anthropic's documented max of 4"
            );
        }
        return stats;
    }

    stats.blocks_examined = content_arr.len();
    let boundary_idx = match detect_auto_injected_boundary(content_arr) {
        Some(idx) => idx,
        None => {
            stats.skip_reason = Some("boundary_not_found".into());
            return stats;
        }
    };
    stats.boundary_idx = boundary_idx as i32;
    stats.boundary_block_kind = Some(format!("{:?}", classify_block(&content_arr[boundary_idx])));

    if content_arr[boundary_idx].get("cache_control").is_some() {
        stats.skip_reason = Some("boundary_already_marked".into());
        return stats;
    }

    // Clone and inject cache_control marker.
    let mut target = content_arr[boundary_idx].clone();
    target["cache_control"] = serde_json::json!({"type": "ephemeral", "ttl": "1h"});

    // Mutate body: body["messages"][0]["content"][boundary_idx] = target
    if let Some(messages_mut) = body
        .get_mut("messages")
        .and_then(|v| v.as_array_mut())
    {
        if let Some(content_mut) = messages_mut[0].get_mut("content").and_then(|v| v.as_array_mut())
        {
            content_mut[boundary_idx] = target;
        }
    }

    stats.injected = true;
    stats
}

// ── Diagnostic dump ──

const DUMP_TEXT_PREFIX_CHARS: usize = 200;

#[derive(Debug, Clone, serde::Serialize)]
struct DumpRecord {
    ts: String,
    role: Option<String>,
    block_count: usize,
    existing_marker_count: usize,
    blocks: Vec<DumpBlock>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct DumpBlock {
    idx: usize,
    #[serde(rename = "type")]
    block_type: Option<String>,
    kind: String,
    text_prefix: Option<String>,
    has_cache_control: bool,
}

fn build_dump_record(body: &Value) -> DumpRecord {
    let messages = body.get("messages").and_then(|v| v.as_array());
    let first = messages.and_then(|m| m.first());
    let content = first.and_then(|f| f.get("content").and_then(|v| v.as_array()));

    let blocks: Vec<DumpBlock> = content
        .map(|c| {
            c.iter()
                .enumerate()
                .map(|(idx, block)| {
                    let kind = format!("{:?}", classify_block(block));
                    let text = get_block_text(block);
                    DumpBlock {
                        idx,
                        block_type: block
                            .get("type")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        kind,
                        text_prefix: text.map(|t| t.chars().take(DUMP_TEXT_PREFIX_CHARS).collect()),
                        has_cache_control: block.get("cache_control").is_some(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    DumpRecord {
        ts: chrono_now(),
        role: first.and_then(|f| f.get("role").and_then(|v| v.as_str()).map(|s| s.to_string())),
        block_count: blocks.len(),
        existing_marker_count: count_all_cache_control_markers(body),
        blocks,
    }
}

fn write_diagnostic_dump(body: &Value, path: &str) -> Result<(), String> {
    let record = build_dump_record(body);
    let line = serde_json::to_string(&record).map_err(|e| e.to_string())?;

    // Ensure parent directory exists.
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir: {e}"))?;
    }

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("write: {e}"))?;

    Ok(())
}

// ── Helpers ──

fn chrono_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn emit_stderr_summary(stats: &InjectionStats) {
    if stats.injected {
        eprintln!(
            "[messages-breakpoint] injected boundary_idx={} kind={:?} existing_markers={}",
            stats.boundary_idx,
            stats.boundary_block_kind.as_deref().unwrap_or("null"),
            stats.existing_marker_count
        );
    } else {
        eprintln!(
            "[messages-breakpoint] skipped reason={:?} existing_markers={}",
            stats.skip_reason.as_deref().unwrap_or("null"),
            stats.existing_marker_count
        );
    }
}

fn debug_log(msg: &str) {
    if std::env::var("CACHE_FIX_DEBUG").ok().as_deref() == Some("1") {
        eprintln!("[messages-breakpoint] DEBUG: {msg}");
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use serde_json::json;

    #[test]
    fn classify_hooks_block() {
        let block = json!({
            "type": "text",
            "text": "<system-reminder>hook success: loaded</system-reminder>"
        });
        assert_eq!(classify_block(&block), BlockKind::Hooks);
    }

    #[test]
    fn classify_skills_block() {
        let block = json!({
            "type": "text",
            "text": "<system-reminder><available-skills>...</available-skills></system-reminder>"
        });
        assert_eq!(classify_block(&block), BlockKind::Skills);
    }

    #[test]
    fn classify_claude_md_block() {
        let block = json!({
            "type": "text",
            "text": "<system-reminder>Contents of /home/user/CLAUDE.md: ...</system-reminder>"
        });
        assert_eq!(classify_block(&block), BlockKind::ClaudeMd);
    }

    #[test]
    fn classify_deferred_tools_block() {
        let block = json!({
            "type": "text",
            "text": "<deferred-tools>...</deferred-tools>"
        });
        assert_eq!(classify_block(&block), BlockKind::DeferredTools);
    }

    #[test]
    fn classify_mcp_block() {
        let block = json!({
            "type": "text",
            "text": "Available MCP servers: server1, server2"
        });
        assert_eq!(classify_block(&block), BlockKind::McpResources);
    }

    #[test]
    fn classify_user_block() {
        let block = json!({
            "type": "text",
            "text": "Hello, please fix this bug"
        });
        assert_eq!(classify_block(&block), BlockKind::User);
    }

    #[test]
    fn classify_non_text_block_returns_user() {
        let block = json!({"type": "image", "source": {}});
        assert_eq!(classify_block(&block), BlockKind::User);
    }

    #[test]
    fn detect_boundary_finds_last_auto_injected() {
        let content = json!([
            {"type": "text", "text": "<system-reminder>hook success: loaded</system-reminder>"},
            {"type": "text", "text": "<system-reminder><available-skills>x</available-skills></system-reminder>"},
            {"type": "text", "text": "Real user question here"}
        ]);
        let arr = content.as_array().unwrap();
        assert_eq!(detect_auto_injected_boundary(arr), Some(1));
    }

    #[test]
    fn detect_boundary_returns_none_when_no_auto_injected() {
        let content = json!([
            {"type": "text", "text": "Just a user message"},
            {"type": "text", "text": "Another user message"}
        ]);
        let arr = content.as_array().unwrap();
        assert_eq!(detect_auto_injected_boundary(arr), None);
    }

    #[test]
    fn count_markers_in_system_and_messages() {
        let body = json!({
            "system": [
                {"type": "text", "text": "s1", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "s2"}
            ],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "m1", "cache_control": {"type": "ephemeral"}},
                    {"type": "text", "text": "m2", "cache_control": {"type": "ephemeral"}}
                ]}
            ]
        });
        assert_eq!(count_all_cache_control_markers(&body), 3);
    }

    #[test]
    fn count_markers_zero_when_none() {
        let body = json!({
            "system": [{"type": "text", "text": "s1"}],
            "messages": [{"role": "user", "content": [{"type": "text", "text": "hi"}]}]
        });
        assert_eq!(count_all_cache_control_markers(&body), 0);
    }

    #[test]
    fn inject_when_markers_between_1_and_3() {
        let mut body = json!({
            "system": [
                {"type": "text", "text": "s1", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "<system-reminder>hook success: loaded</system-reminder>"},
                    {"type": "text", "text": "Real user query"}
                ]}
            ]
        });
        let stats = inject_messages_breakpoint(&mut body);
        assert!(stats.injected);
        assert_eq!(stats.boundary_idx, 0);

        // Verify the boundary block now has cache_control.
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert!(content[0].get("cache_control").is_some());
        assert_eq!(
            content[0]["cache_control"]["type"].as_str().unwrap(),
            "ephemeral"
        );
    }

    #[test]
    fn skip_when_no_existing_markers() {
        let mut body = json!({
            "system": [{"type": "text", "text": "s1"}],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "<system-reminder>hook success: loaded</system-reminder>"},
                    {"type": "text", "text": "Real user query"}
                ]}
            ]
        });
        let stats = inject_messages_breakpoint(&mut body);
        assert!(!stats.injected);
        assert_eq!(stats.skip_reason, Some("no_existing_markers".into()));
    }

    #[test]
    fn skip_when_at_marker_limit() {
        let mut body = json!({
            "system": [
                {"type": "text", "text": "s1", "cache_control": {"type": "ephemeral"}},
                {"type": "text", "text": "s2", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "m1", "cache_control": {"type": "ephemeral"}},
                    {"type": "text", "text": "m2", "cache_control": {"type": "ephemeral"}},
                    {"type": "text", "text": "<system-reminder>hook success</system-reminder>"},
                    {"type": "text", "text": "Real query"}
                ]}
            ]
        });
        let stats = inject_messages_breakpoint(&mut body);
        assert!(!stats.injected);
        assert_eq!(stats.skip_reason, Some("at_marker_limit".into()));
    }

    #[test]
    fn skip_when_boundary_already_marked() {
        let mut body = json!({
            "system": [
                {"type": "text", "text": "s1", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "<system-reminder>hook success: loaded</system-reminder>", "cache_control": {"type": "ephemeral"}},
                    {"type": "text", "text": "Real query"}
                ]}
            ]
        });
        let stats = inject_messages_breakpoint(&mut body);
        assert!(!stats.injected);
        assert_eq!(stats.skip_reason, Some("boundary_already_marked".into()));
    }

    #[test]
    fn on_request_sets_meta() {
        let ext = MessagesCacheBreakpoint::new();
        let mut ctx = RequestContext {
            body: json!({
                "system": [
                    {"type": "text", "text": "s1", "cache_control": {"type": "ephemeral"}}
                ],
                "messages": [
                    {"role": "user", "content": [
                        {"type": "text", "text": "<system-reminder>hook success: loaded</system-reminder>"},
                        {"type": "text", "text": "Real user query"}
                    ]}
                ]
            }),
            headers: HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };

        // Enable injection via env var for the test.
        std::env::set_var("CACHE_FIX_INJECT_MESSAGES_BREAKPOINT", "1");
        let result = ext.on_request(&mut ctx).unwrap();
        std::env::remove_var("CACHE_FIX_INJECT_MESSAGES_BREAKPOINT");

        assert!(result.is_none());
        assert!(ctx.meta.get("messagesBreakpointStats").is_some());
    }
}
