// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/image-strip.mjs
// 翻译: 2026-05-20
//
// Multi-pass image stripping pipeline. Handles:
//   - Pass 0: KEEP_LAST tool_result image strip (legacy v3.2.1)
//   - Pass 1: Rejection-cap strip by max dimension
//   - Pass 2: Request-size guard (evict images to fit size budget)
//   - Pass 3: Native-cap Lanczos resize (requires image crate)
//   - Hard image-count cap
//
// Activation gates:
//   - CACHE_FIX_IMAGE_KEEP_LAST=N        → legacy Pass 0
//   - CACHE_FIX_IMAGE_MAX_DIM=N          → legacy Pass 1 strip-only
//   - CACHE_FIX_IMAGE_GUARD=1            → v3.3.0 pipeline (Pass 1+2+count cap)
//   - CACHE_FIX_IMAGE_GUARD=1 + CACHE_FIX_IMAGE_PRESERVE_DETAIL=1 → +Pass 3

use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::Value;
use std::sync::Mutex;

// --- One-time warning state ---

static PRESERVE_DETAIL_WARNED: Mutex<bool> = Mutex::new(false);

// --- Legacy v3.2.1 constants ---

const PLACEHOLDER: &str = "[image stripped from history — file may still be on disk]";

fn oversized_placeholder(max_dim: u32, w: u32, h: u32) -> String {
    format!(
        "[image stripped — exceeded {}px max dimension (was {}x{}px)]",
        max_dim, w, h
    )
}

// --- Env helpers (read at call time) ---

fn is_image_guard_enabled() -> bool {
    std::env::var("CACHE_FIX_IMAGE_GUARD")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn is_preserve_detail_enabled() -> bool {
    std::env::var("CACHE_FIX_IMAGE_PRESERVE_DETAIL")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn get_max_dim() -> u32 {
    std::env::var("CACHE_FIX_IMAGE_MAX_DIM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn get_keep_last() -> u32 {
    std::env::var("CACHE_FIX_IMAGE_KEEP_LAST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn get_request_size_max() -> usize {
    std::env::var("CACHE_FIX_IMAGE_REQUEST_SIZE_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(31_457_280) // 30 MB
}

fn is_debug() -> bool {
    std::env::var("CACHE_FIX_DEBUG")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn get_image_count_max() -> usize {
    std::env::var("CACHE_FIX_IMAGE_COUNT_MAX")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(100)
}

// --- Extension struct ---

pub struct ImageStrip;

impl ImageStrip {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for ImageStrip {
    fn name(&self) -> &str {
        "image-strip"
    }
    fn order(&self) -> u32 {
        150
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

impl RequestExtension for ImageStrip {
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        let guard_on = is_image_guard_enabled();
        let preserve_on = is_preserve_detail_enabled();

        // ctx.meta overrides allow tests to drive legacy paths without env vars.
        let keep_last = ctx
            .meta
            .get("imageKeepLast")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or_else(get_keep_last);

        let max_dim = ctx
            .meta
            .get("imageMaxDim")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32)
            .unwrap_or_else(get_max_dim);

        // Short-circuit: nothing to do.
        if !guard_on && keep_last == 0 && max_dim == 0 {
            // Surface the PRESERVE_DETAIL-without-GUARD warning.
            if preserve_on {
                let mut warned = PRESERVE_DETAIL_WARNED.lock().unwrap();
                if !*warned {
                    eprintln!(
                        "[image-guard] CACHE_FIX_IMAGE_PRESERVE_DETAIL=1 has no effect without CACHE_FIX_IMAGE_GUARD=1"
                    );
                    *warned = true;
                }
            }
            return Ok(None);
        }

        if ctx.body.get("messages").is_none() {
            return Ok(None);
        }

        // ========== Legacy path (v3.2.1 back-compat) ==========
        if !guard_on {
            let mut log_parts: Vec<String> = vec![];

            if keep_last > 0 {
                let (new_messages, stats) =
                    strip_old_tool_result_images(&ctx.body["messages"], keep_last);
                if let Some(stats) = stats {
                    ctx.body["messages"] = new_messages;
                    ctx.meta.set(
                        "imageStripStats",
                        serde_json::to_value(&stats).unwrap_or_default(),
                    );
                    log_parts.push(format!(
                        "keep_last: {} stripped (~{} tokens saved)",
                        stats.stripped_count, stats.estimated_tokens
                    ));
                }
            }

            if max_dim > 0 {
                let messages_ref = if keep_last > 0 {
                    &ctx.body["messages"]
                } else {
                    &ctx.body["messages"]
                };
                // We need mutable access — re-borrow.
                let (new_messages, stats) =
                    strip_oversized_images(messages_ref, max_dim);
                if let Some(stats) = stats {
                    ctx.body["messages"] = new_messages;
                    ctx.meta.set(
                        "imageStripOversizedStats",
                        serde_json::to_value(&stats).unwrap_or_default(),
                    );
                    log_parts.push(format!(
                        "max_dim: {} oversized stripped (~{} tokens saved)",
                        stats.stripped_count, stats.estimated_tokens
                    ));
                }
            }

            if !log_parts.is_empty() && is_debug() {
                eprintln!("[image-strip] {}", log_parts.join("; "));
            }
            return Ok(None);
        }

        // ========== v3.3.0 pipeline path ==========
        // Pass 0: KEEP_LAST runs first (back-compat behavior preserved).
        if keep_last > 0 {
            let (new_messages, stats) =
                strip_old_tool_result_images(&ctx.body["messages"], keep_last);
            if let Some(stats) = stats {
                ctx.body["messages"] = new_messages;
                ctx.meta.set(
                    "imageStripStats",
                    serde_json::to_value(&stats).unwrap_or_default(),
                );
            }
        }

        // Run the pipeline.
        let stats = run_image_guard(&mut ctx.body)?;
        ctx.meta.set(
            "imageGuardStats",
            serde_json::to_value(&stats).unwrap_or_default(),
        );

        // Emit summary only if the pipeline actually did something observable.
        let did_something = stats.images_stripped_pass1 > 0
            || stats.images_dropped_for_size > 0
            || stats.images_dropped_for_count_cap > 0
            || stats.resize_attempted > 0
            || stats.resize_succeeded > 0
            || stats.unsupported_format_count > 0
            || stats.dimension_probe_fail_count > 0;

        if did_something && is_debug() {
            let mut parts: Vec<String> = vec![];
            if stats.resize_succeeded > 0 {
                parts.push(format!("resized={}", stats.resize_succeeded));
            }
            if stats.resize_failed > 0 {
                parts.push(format!("resize_failed={}", stats.resize_failed));
            }
            if stats.library_missing {
                parts.push("sharp=missing".into());
            }
            if stats.images_stripped_pass1 > 0 {
                parts.push(format!("stripped={}", stats.images_stripped_pass1));
            }
            if stats.images_dropped_for_size > 0 {
                parts.push(format!("evicted={}", stats.images_dropped_for_size));
            }
            if stats.images_dropped_for_count_cap > 0 {
                parts.push(format!(
                    "count_capped={}",
                    stats.images_dropped_for_count_cap
                ));
            }
            if stats.unsupported_format_count > 0 {
                parts.push(format!("unsupported={}", stats.unsupported_format_count));
            }
            let summary = if parts.is_empty() {
                "ran".to_string()
            } else {
                parts.join(" ")
            };
            let final_images = stats.total_images as i64
                - stats.images_stripped_pass1 as i64
                - stats.images_dropped_for_size as i64
                - stats.images_dropped_for_count_cap as i64;
            eprintln!(
                "[image-guard] {} req_bytes={}->{} (headroom={}) images={}->{}",
                summary,
                stats.request_bytes_before,
                stats.request_bytes_after,
                stats.request_bytes_headroom,
                stats.total_images,
                final_images.max(0)
            );
        }

        Ok(None)
    }
}

// =============================================================================
// Image dimension parsing (pure Rust, no external image library needed)
// =============================================================================

/// Parse PNG dimensions from raw bytes (header probe).
fn parse_png_dimensions(buffer: &[u8]) -> Option<(u32, u32)> {
    const PNG_MAGIC: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    if buffer.len() < 24 {
        return None;
    }
    if buffer[..8] != PNG_MAGIC {
        return None;
    }
    // Verify IHDR chunk type at offset 12-15.
    if buffer[12] != 0x49
        || buffer[13] != 0x48
        || buffer[14] != 0x44
        || buffer[15] != 0x52
    {
        return None;
    }
    let width = u32::from_be_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);
    let height = u32::from_be_bytes([buffer[20], buffer[21], buffer[22], buffer[23]]);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// Parse JPEG dimensions from raw bytes (header probe).
fn parse_jpeg_dimensions(buffer: &[u8]) -> Option<(u32, u32)> {
    if buffer.len() < 4 || buffer[0] != 0xff || buffer[1] != 0xd8 {
        return None;
    }

    const JPEG_SOF_MARKERS: &[u8] = &[
        0xc0, 0xc1, 0xc2, 0xc3, 0xc5, 0xc6, 0xc7, 0xc9, 0xca, 0xcb, 0xcd, 0xce, 0xcf,
    ];

    let mut i = 2usize;
    let max = buffer.len();
    let mut iterations = 0u32;

    while i < max.saturating_sub(8) && iterations < 1000 {
        iterations += 1;
        if buffer[i] != 0xff {
            i += 1;
            continue;
        }
        // Skip multiple 0xFF prefixes.
        while i < max.saturating_sub(1) && buffer[i] == 0xff {
            i += 1;
        }
        let marker = buffer[i];
        i += 1;

        // Markers without length: SOI(D8), EOI(D9), RST0-7(D0-D7).
        if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }

        if i + 1 >= max {
            return None;
        }
        let seg_len = ((buffer[i] as usize) << 8) | (buffer[i + 1] as usize);
        if seg_len < 2 || i + seg_len > max {
            return None;
        }

        if JPEG_SOF_MARKERS.contains(&marker) {
            // SOF layout: [length 2B][precision 1B][height 2B][width 2B]
            if i + 6 >= max {
                return None;
            }
            let height = ((buffer[i + 3] as u32) << 8) | (buffer[i + 4] as u32);
            let width = ((buffer[i + 5] as u32) << 8) | (buffer[i + 6] as u32);
            if width == 0 || height == 0 {
                return None;
            }
            return Some((width, height));
        }

        i += seg_len;
    }
    None
}

/// Parse image dimensions from base64-encoded data.
/// Decodes only the first ~1024 bytes of the image header.
fn parse_image_dimensions(media_type: &str, base64_data: &str) -> Option<(u32, u32)> {
    if media_type.is_empty() || base64_data.is_empty() {
        return None;
    }
    const HEADER_PROBE_BYTES: usize = 1024;
    let probe_chars = base64_data.len().min(HEADER_PROBE_BYTES * 2);
    let probe = &base64_data[..probe_chars];
    let buffer = STANDARD.decode(probe).ok()?;
    if buffer.is_empty() {
        return None;
    }

    match media_type.to_lowercase().as_str() {
        "image/png" => parse_png_dimensions(&buffer),
        "image/jpeg" | "image/jpg" => parse_jpeg_dimensions(&buffer),
        _ => None, // Unsupported format — fail-open.
    }
}

// =============================================================================
// Stats
// =============================================================================

#[derive(Debug, Clone, Default, serde::Serialize)]
struct ImageGuardStats {
    total_images: usize,
    count_axis_path: String,
    unsupported_format_count: usize,
    dimension_probe_fail_count: usize,
    resize_attempted: usize,
    resize_succeeded: usize,
    resize_failed: usize,
    library_missing: bool,
    images_stripped_pass1: usize,
    images_dropped_for_size: usize,
    images_dropped_for_count_cap: usize,
    request_bytes_before: usize,
    request_bytes_after: usize,
    request_bytes_headroom: usize,
    image_bytes_total: usize,
    image_bytes_dropped: usize,
    estimated_image_tokens_total: usize,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
struct StripStats {
    stripped_count: usize,
    stripped_bytes: usize,
    estimated_tokens: usize,
}

// =============================================================================
// Image reference walker
// =============================================================================

#[derive(Debug, Clone)]
struct ImageRef {
    msg_idx: usize,
    block_idx: usize,
    item_idx: Option<usize>, // None = direct user-msg content block; Some = nested in tool_result
    item: Value,
}

/// Enumerate every image in body.messages, both user-msg direct content
/// and tool_result.content.
fn walk_images(messages: &Value) -> Vec<ImageRef> {
    let mut out = vec![];
    let arr = match messages.as_array() {
        Some(a) => a,
        None => return out,
    };
    for (m, msg) in arr.iter().enumerate() {
        let content = match msg.get("content").and_then(|v| v.as_array()) {
            Some(c) => c,
            None => continue,
        };
        for (b, block) in content.iter().enumerate() {
            if block.get("type").and_then(|v| v.as_str()) == Some("image") {
                out.push(ImageRef {
                    msg_idx: m,
                    block_idx: b,
                    item_idx: None,
                    item: block.clone(),
                });
            } else if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                if let Some(tool_content) = block.get("content").and_then(|v| v.as_array()) {
                    for (i, item) in tool_content.iter().enumerate() {
                        if item.get("type").and_then(|v| v.as_str()) == Some("image") {
                            out.push(ImageRef {
                                msg_idx: m,
                                block_idx: b,
                                item_idx: Some(i),
                                item: item.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    out
}

/// Mutate a single image at the given reference, replacing it with `replacement`.
fn replace_image_in_place(messages: &mut Value, ref_: &ImageRef, replacement: Value) {
    let arr = match messages.as_array_mut() {
        Some(a) => a,
        None => return,
    };
    let msg = match arr.get_mut(ref_.msg_idx) {
        Some(m) => m,
        None => return,
    };
    let content = match msg.get_mut("content").and_then(|v| v.as_array_mut()) {
        Some(c) => c,
        None => return,
    };
    let block = match content.get_mut(ref_.block_idx) {
        Some(b) => b,
        None => return,
    };
    if let Some(item_idx) = ref_.item_idx {
        if let Some(tool_content) = block.get_mut("content").and_then(|v| v.as_array_mut()) {
            if item_idx < tool_content.len() {
                tool_content[item_idx] = replacement;
            }
        }
    } else {
        content[ref_.block_idx] = replacement;
    }
}

// =============================================================================
// Legacy Pass 0: KEEP_LAST tool_result image strip
// =============================================================================

fn strip_old_tool_result_images(messages: &Value, keep_last: u32) -> (Value, Option<StripStats>) {
    let arr = match messages.as_array() {
        Some(a) => a,
        None => return (messages.clone(), None),
    };
    if keep_last == 0 {
        return (messages.clone(), None);
    }

    let user_msg_indices: Vec<usize> = arr
        .iter()
        .enumerate()
        .filter_map(|(i, msg)| {
            if msg.get("role").and_then(|v| v.as_str()) == Some("user") {
                Some(i)
            } else {
                None
            }
        })
        .collect();

    let keep_last = keep_last as usize;
    if user_msg_indices.len() <= keep_last {
        return (messages.clone(), None);
    }

    let cutoff_idx = user_msg_indices[user_msg_indices.len() - keep_last];

    let mut stripped_count = 0usize;
    let mut stripped_bytes = 0usize;

    let result: Vec<Value> = arr
        .iter()
        .enumerate()
        .map(|(msg_idx, msg)| {
            if msg.get("role").and_then(|v| v.as_str()) != Some("user")
                || msg_idx >= cutoff_idx
            {
                return msg.clone();
            }

            let content = match msg.get("content").and_then(|v| v.as_array()) {
                Some(c) => c,
                None => return msg.clone(),
            };

            let mut msg_modified = false;
            let new_content: Vec<Value> = content
                .iter()
                .map(|block| {
                    if block.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
                        return block.clone();
                    }
                    let tool_content =
                        match block.get("content").and_then(|v| v.as_array()) {
                            Some(tc) => tc,
                            None => return block.clone(),
                        };

                    let mut tool_modified = false;
                    let new_tool_content: Vec<Value> = tool_content
                        .iter()
                        .map(|item| {
                            if item.get("type").and_then(|v| v.as_str()) == Some("image") {
                                stripped_count += 1;
                                if let Some(data) =
                                    item.get("source").and_then(|s| s.get("data"))
                                {
                                    if let Some(s) = data.as_str() {
                                        stripped_bytes += s.len();
                                    }
                                }
                                tool_modified = true;
                                serde_json::json!({"type": "text", "text": PLACEHOLDER})
                            } else {
                                item.clone()
                            }
                        })
                        .collect();

                    if tool_modified {
                        msg_modified = true;
                        let mut new_block = block.clone();
                        new_block["content"] = Value::Array(new_tool_content);
                        new_block
                    } else {
                        block.clone()
                    }
                })
                .collect();

            if msg_modified {
                let mut new_msg = msg.clone();
                new_msg["content"] = Value::Array(new_content);
                new_msg
            } else {
                msg.clone()
            }
        })
        .collect();

    if stripped_count > 0 {
        let stats = StripStats {
            stripped_count,
            stripped_bytes,
            estimated_tokens: (stripped_bytes as f64 * 0.125).ceil() as usize,
        };
        (Value::Array(result), Some(stats))
    } else {
        (messages.clone(), None)
    }
}

// =============================================================================
// Legacy Pass 1 (strip-only by max dim)
// =============================================================================

fn strip_oversized_images(messages: &Value, max_dim: u32) -> (Value, Option<StripStats>) {
    let arr = match messages.as_array() {
        Some(a) => a,
        None => return (messages.clone(), None),
    };
    if max_dim == 0 {
        return (messages.clone(), None);
    }

    let mut stripped_count = 0usize;
    let mut stripped_bytes = 0usize;

    let maybe_strip = |item: &Value| -> Option<Value> {
        if item.get("type").and_then(|v| v.as_str()) != Some("image") {
            return None;
        }
        let src = item.get("source")?;
        let data = src.get("data")?.as_str()?;
        let media_type = src.get("media_type")?.as_str()?;
        let dims = parse_image_dimensions(media_type, data)?;
        if dims.0 <= max_dim && dims.1 <= max_dim {
            return None;
        }
        Some(serde_json::json!({
            "type": "text",
            "text": oversized_placeholder(max_dim, dims.0, dims.1)
        }))
    };

    let result: Vec<Value> = arr
        .iter()
        .map(|msg| {
            let content = match msg.get("content").and_then(|v| v.as_array()) {
                Some(c) => c,
                None => return msg.clone(),
            };

            let mut mutated = false;
            let new_content: Vec<Value> = content
                .iter()
                .map(|block| {
                    if block.get("type").and_then(|v| v.as_str()) == Some("image") {
                        if let Some(replacement) = maybe_strip(block) {
                            if let Some(data) =
                                block.get("source").and_then(|s| s.get("data"))
                            {
                                if let Some(s) = data.as_str() {
                                    stripped_bytes += s.len();
                                }
                            }
                            stripped_count += 1;
                            mutated = true;
                            return replacement;
                        }
                        return block.clone();
                    }

                    if block.get("type").and_then(|v| v.as_str()) == Some("tool_result") {
                        let tool_content =
                            match block.get("content").and_then(|v| v.as_array())
                            {
                                Some(tc) => tc,
                                None => return block.clone(),
                            };

                        let mut tool_mutated = false;
                        let new_tool_content: Vec<Value> = tool_content
                            .iter()
                            .map(|item| {
                                if let Some(replacement) = maybe_strip(item) {
                                    if let Some(data) =
                                        item.get("source").and_then(|s| s.get("data"))
                                    {
                                        if let Some(s) = data.as_str() {
                                            stripped_bytes += s.len();
                                        }
                                    }
                                    stripped_count += 1;
                                    tool_mutated = true;
                                    replacement
                                } else {
                                    item.clone()
                                }
                            })
                            .collect();

                        if tool_mutated {
                            mutated = true;
                            let mut new_block = block.clone();
                            new_block["content"] = Value::Array(new_tool_content);
                            return new_block;
                        }
                    }

                    block.clone()
                })
                .collect();

            if mutated {
                let mut new_msg = msg.clone();
                new_msg["content"] = Value::Array(new_content);
                new_msg
            } else {
                msg.clone()
            }
        })
        .collect();

    if stripped_count > 0 {
        let stats = StripStats {
            stripped_count,
            stripped_bytes,
            estimated_tokens: (stripped_bytes as f64 * 0.125).ceil() as usize,
        };
        (Value::Array(result), Some(stats))
    } else {
        (messages.clone(), None)
    }
}

// =============================================================================
// v3.3.0 pipeline helpers
// =============================================================================

fn pick_pass1_cap(image_count: usize, max_dim_override: u32) -> u32 {
    if max_dim_override > 0 {
        return max_dim_override;
    }
    if image_count > 20 {
        2000
    } else {
        8000
    }
}

fn pick_pass3_native_cap(model_string: Option<&str>) -> u32 {
    match model_string {
        Some(s) if s.starts_with("claude-opus-4-7") => 2576,
        _ => 1568,
    }
}

fn estimate_image_tokens(width: u32, height: u32, model_token_cap: u32) -> usize {
    if width == 0 || height == 0 {
        return 0;
    }
    let raw = ((width as f64 * height as f64) / 750.0).ceil() as usize;
    if model_token_cap > 0 && raw > model_token_cap as usize {
        model_token_cap as usize
    } else {
        raw
    }
}

fn native_token_cap(model_string: Option<&str>) -> u32 {
    match model_string {
        Some(s) if s.starts_with("claude-opus-4-7") => 4784,
        _ => 1568,
    }
}

/// Eviction order: oldest first (low msgIdx wins), within a message tool_result
/// images are preferred over direct images at the same age.
fn pick_eviction_targets(messages: &Value) -> Vec<ImageRef> {
    let mut refs = walk_images(messages);
    refs.sort_by(|a, b| {
        a.msg_idx
            .cmp(&b.msg_idx)
            .then_with(|| {
                let a_tool = if a.item_idx.is_some() { 0 } else { 1 };
                let b_tool = if b.item_idx.is_some() { 0 } else { 1 };
                a_tool.cmp(&b_tool)
            })
            .then_with(|| a.block_idx.cmp(&b.block_idx))
            .then_with(|| a.item_idx.unwrap_or(0).cmp(&b.item_idx.unwrap_or(0)))
    });
    refs
}

// =============================================================================
// Pass 3: Native-cap resize via image crate
// =============================================================================

struct ResizeResult {
    ok: bool,
    base64: Option<String>,
    dims: Option<(u32, u32)>,
    #[allow(dead_code)]
    bytes: Option<usize>,
    reason: Option<String>,
}

impl ResizeResult {
    fn ok_same(base64: String, w: u32, h: u32, bytes: usize) -> Self {
        Self {
            ok: true,
            base64: Some(base64),
            dims: Some((w, h)),
            bytes: Some(bytes),
            reason: None,
        }
    }

    fn unsupported_media_type() -> Self {
        Self {
            ok: false,
            base64: None,
            dims: None,
            bytes: None,
            reason: Some("unsupported_media_type".into()),
        }
    }

    fn decode_failed() -> Self {
        Self {
            ok: false,
            base64: None,
            dims: None,
            bytes: None,
            reason: Some("decode_failed".into()),
        }
    }

    fn resize_failed() -> Self {
        Self {
            ok: false,
            base64: None,
            dims: None,
            bytes: None,
            reason: Some("resize_failed".into()),
        }
    }

    fn library_missing() -> Self {
        Self {
            ok: false,
            base64: None,
            dims: None,
            bytes: None,
            reason: Some("library_missing".into()),
        }
    }
}

/// Resize a base64-encoded image to `cap_px` on the long edge using Lanczos3.
/// Preserves aspect ratio (fit inside), never upscales.
fn resize_image_to_cap(base64_data: &str, media_type: &str, cap_px: u32) -> ResizeResult {
    if base64_data.is_empty() {
        return ResizeResult::decode_failed();
    }

    let format = match media_type.to_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpeg",
        _ => return ResizeResult::unsupported_media_type(),
    };

    // Decode base64.
    let decoded = match STANDARD.decode(base64_data) {
        Ok(d) => d,
        Err(_) => return ResizeResult::decode_failed(),
    };
    if decoded.is_empty() {
        return ResizeResult::decode_failed();
    }

    // Load image.
    let img = match image::load_from_memory(&decoded) {
        Ok(i) => i,
        Err(_) => return ResizeResult::decode_failed(),
    };

    let (w, h) = (img.width(), img.height());
    let long_edge = w.max(h);

    // withoutEnlargement: if already within cap, return original.
    if long_edge <= cap_px {
        return ResizeResult::ok_same(base64_data.to_string(), w, h, decoded.len());
    }

    // Resize with Lanczos3, fit inside.
    let resized = img.resize(cap_px, cap_px, image::imageops::FilterType::Lanczos3);
    let new_w = resized.width();
    let new_h = resized.height();

    // Re-encode in same format.
    let mut buf = std::io::Cursor::new(Vec::new());
    let write_format = match format {
        "png" => image::ImageFormat::Png,
        "jpeg" => image::ImageFormat::Jpeg,
        _ => return ResizeResult::unsupported_media_type(),
    };
    if resized.write_to(&mut buf, write_format).is_err() {
        return ResizeResult::resize_failed();
    }
    let buf = buf.into_inner();
    let new_base64 = STANDARD.encode(&buf);

    ResizeResult::ok_same(new_base64, new_w, new_h, buf.len())
}

// =============================================================================
// Pass 3 runtime: native-cap resize
// =============================================================================

fn run_pass3_native_cap_resize(body: &mut Value, stats: &mut ImageGuardStats) {
    if stats.library_missing {
        return;
    }

    // Extract model before mutable borrow of messages.
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let native_cap = pick_pass3_native_cap(model.as_deref());
    let token_cap = native_token_cap(model.as_deref());

    let messages = match body.get_mut("messages") {
        Some(m) => m,
        None => return,
    };

    let plan = walk_images_for_pass3(messages, native_cap);
    for step in plan {
        if step.action == "skip_unmeasurable" {
            continue;
        }
        if step.action != "resize" {
            continue;
        }

        let src = match step.ref_.item.get("source") {
            Some(s) => s,
            None => continue,
        };
        let data = match src.get("data").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => continue,
        };
        let media_type = match src.get("media_type").and_then(|v| v.as_str()) {
            Some(mt) => mt,
            None => continue,
        };

        stats.resize_attempted += 1;
        let result = resize_image_to_cap(data, media_type, step.cap_px);

        if result.ok {
            stats.resize_succeeded += 1;
            if let (Some(new_base64), Some((new_w, new_h))) =
                (result.base64, result.dims)
            {
                let mut new_src = src.clone();
                new_src["data"] = Value::String(new_base64);
                let mut new_image = step.ref_.item.clone();
                new_image["source"] = new_src;
                replace_image_in_place(messages, &step.ref_, new_image);

                let tokens_before =
                    estimate_image_tokens(step.dims.0, step.dims.1, token_cap);
                let tokens_after =
                    estimate_image_tokens(new_w, new_h, token_cap);
                stats.estimated_image_tokens_total = stats
                    .estimated_image_tokens_total
                    .saturating_add(tokens_after)
                    .saturating_sub(tokens_before);
            }
        } else if result.reason.as_deref() == Some("library_missing") {
            stats.library_missing = true;
            return; // Sticky: stop attempting Pass 3 for this request.
        } else {
            stats.resize_failed += 1;
            // Leave image untouched; Pass 1 will evaluate it against its own cap.
        }
    }
}

// =============================================================================
// Pass 3 walker
// =============================================================================

struct Pass3Step {
    ref_: ImageRef,
    dims: (u32, u32),
    action: String,
    cap_px: u32,
}

fn walk_images_for_pass3(messages: &Value, native_cap_px: u32) -> Vec<Pass3Step> {
    let refs = walk_images(messages);
    let mut plan = vec![];
    for ref_ in refs {
        let src = match ref_.item.get("source") {
            Some(s) => s,
            None => continue,
        };
        let data = match src.get("data").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => continue,
        };
        let media_type = match src.get("media_type").and_then(|v| v.as_str()) {
            Some(mt) => mt,
            None => continue,
        };
        let dims = match parse_image_dimensions(media_type, data) {
            Some(d) => d,
            None => {
                plan.push(Pass3Step {
                    ref_,
                    dims: (0, 0),
                    action: "skip_unmeasurable".into(),
                    cap_px: native_cap_px,
                });
                continue;
            }
        };
        let long_edge = dims.0.max(dims.1);
        if long_edge > native_cap_px {
            plan.push(Pass3Step {
                ref_,
                dims,
                action: "resize".into(),
                cap_px: native_cap_px,
            });
        }
    }
    plan
}

// =============================================================================
// Pass 1 runtime: conditional rejection-cap strip
// =============================================================================

struct Pass1Opts {
    max_dim_override: u32,
}

fn run_pass1_rejection_cap_strip(
    body: &mut Value,
    stats: &mut ImageGuardStats,
    opts: &Pass1Opts,
) {
    let messages = match body.get_mut("messages") {
        Some(m) => m,
        None => return,
    };

    let refs = walk_images(messages);
    let image_count = refs.len();
    stats.total_images = stats.total_images.max(image_count);
    stats.count_axis_path = if image_count > 20 {
        "many".into()
    } else {
        "few".into()
    };
    let cap = pick_pass1_cap(image_count, opts.max_dim_override);

    for ref_ in refs {
        let src = match ref_.item.get("source") {
            Some(s) => s,
            None => continue,
        };
        let data = match src.get("data").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => continue,
        };
        let media_type = match src.get("media_type").and_then(|v| v.as_str()) {
            Some(mt) => mt,
            None => continue,
        };

        let dims = match parse_image_dimensions(media_type, data) {
            Some(d) => d,
            None => {
                let mt_lower = media_type.to_lowercase();
                if mt_lower == "image/png" || mt_lower == "image/jpeg" || mt_lower == "image/jpg"
                {
                    stats.dimension_probe_fail_count += 1;
                } else {
                    stats.unsupported_format_count += 1;
                }
                continue;
            }
        };

        let long_edge = dims.0.max(dims.1);
        if long_edge > cap {
            replace_image_in_place(
                messages,
                &ref_,
                serde_json::json!({
                    "type": "text",
                    "text": oversized_placeholder(cap, dims.0, dims.1)
                }),
            );
            stats.images_stripped_pass1 += 1;
        }
    }
}

// =============================================================================
// Pass 2 runtime: request-size guard
// =============================================================================

fn run_pass2_request_size_guard(body: &mut Value, stats: &mut ImageGuardStats) {
    let budget = get_request_size_max();

    // Serialize to compute byte length.
    let serialized = serde_json::to_string(body).unwrap_or_default();
    let before = serialized.len();
    if stats.request_bytes_before == 0 {
        stats.request_bytes_before = before;
    }

    if before <= budget {
        stats.request_bytes_after = before;
        stats.request_bytes_headroom = budget - before;
        return;
    }

    let queue = pick_eviction_targets(&body["messages"]);
    let mut bytes = before;

    for ref_ in &queue {
        if bytes <= budget {
            break;
        }
        let dropped_bytes = ref_
            .item
            .get("source")
            .and_then(|s| s.get("data"))
            .and_then(|d| d.as_str())
            .map(|s| s.len())
            .unwrap_or(0);

        replace_image_in_place(
            &mut body["messages"],
            ref_,
            serde_json::json!({
                "type": "text",
                "text": "[image dropped to fit request-size budget]"
            }),
        );
        stats.images_dropped_for_size += 1;
        stats.image_bytes_dropped += dropped_bytes;

        // Re-measure.
        bytes = serde_json::to_string(body).unwrap_or_default().len();
    }

    stats.request_bytes_after = bytes;
    stats.request_bytes_headroom = budget.saturating_sub(bytes);
}

// =============================================================================
// Hard image-count cap
// =============================================================================

fn run_image_count_cap(body: &mut Value, stats: &mut ImageGuardStats) {
    let cap = get_image_count_max();
    let queue = pick_eviction_targets(&body["messages"]);
    if queue.len() <= cap {
        return;
    }

    let to_drop = queue.len() - cap;
    for ref_ in queue.iter().take(to_drop) {
        let dropped_bytes = ref_
            .item
            .get("source")
            .and_then(|s| s.get("data"))
            .and_then(|d| d.as_str())
            .map(|s| s.len())
            .unwrap_or(0);

        replace_image_in_place(
            &mut body["messages"],
            ref_,
            serde_json::json!({
                "type": "text",
                "text": "[image dropped — exceeded image-count cap]"
            }),
        );
        stats.images_dropped_for_count_cap += 1;
        stats.image_bytes_dropped += dropped_bytes;
    }

    if to_drop > 0 {
        let budget = get_request_size_max();
        let after = serde_json::to_string(body).unwrap_or_default().len();
        stats.request_bytes_after = after;
        stats.request_bytes_headroom = budget.saturating_sub(after);
    }
}

// =============================================================================
// Telemetry finalization
// =============================================================================

fn finalize_telemetry(body: &Value, stats: &mut ImageGuardStats) {
    let refs = walk_images(&body["messages"]);
    let mut total_bytes = 0usize;
    let mut total_tokens = 0usize;
    let token_cap = native_token_cap(
        body.get("model").and_then(|v| v.as_str()),
    );

    for ref_ in refs {
        let src = match ref_.item.get("source") {
            Some(s) => s,
            None => continue,
        };
        let data = match src.get("data").and_then(|v| v.as_str()) {
            Some(d) => d,
            None => continue,
        };
        total_bytes += data.len();

        let media_type = src.get("media_type").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(dims) = parse_image_dimensions(media_type, data) {
            total_tokens += estimate_image_tokens(dims.0, dims.1, token_cap);
        }
    }

    stats.image_bytes_total = total_bytes;
    stats.estimated_image_tokens_total = total_tokens;
}

// =============================================================================
// Top-level pipeline orchestrator
// =============================================================================

fn run_image_guard(body: &mut Value) -> Result<ImageGuardStats, ExtensionError> {
    let mut stats = ImageGuardStats::default();

    if body.get("messages").and_then(|v| v.as_array()).is_none() {
        return Ok(stats);
    }

    // Capture initial population count.
    stats.total_images = walk_images(&body["messages"]).len();
    stats.count_axis_path = if stats.total_images > 20 {
        "many".into()
    } else {
        "few".into()
    };

    let guard_on = is_image_guard_enabled();
    let preserve_on = is_preserve_detail_enabled();
    let max_dim_override = get_max_dim();

    // Warn if PRESERVE_DETAIL is set without IMAGE_GUARD (one-time per process).
    if !guard_on && preserve_on {
        let mut warned = PRESERVE_DETAIL_WARNED.lock().unwrap();
        if !*warned {
            eprintln!(
                "[image-guard] CACHE_FIX_IMAGE_PRESERVE_DETAIL=1 has no effect without CACHE_FIX_IMAGE_GUARD=1"
            );
            *warned = true;
        }
    }

    // Pass 3: native-cap resize (only when both gates are on).
    if guard_on && preserve_on {
        run_pass3_native_cap_resize(body, &mut stats);
    }

    // Pass 1: rejection-cap strip.
    if guard_on || max_dim_override > 0 {
        run_pass1_rejection_cap_strip(
            body,
            &mut stats,
            &Pass1Opts {
                max_dim_override,
            },
        );
    }

    // Pass 2: request-size guard — IMAGE_GUARD only.
    if guard_on {
        run_pass2_request_size_guard(body, &mut stats);
    }

    // Hard image-count cap — IMAGE_GUARD only.
    if guard_on {
        run_image_count_cap(body, &mut stats);
    }

    // Final telemetry sweep.
    finalize_telemetry(body, &mut stats);

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_png_dimensions_valid() {
        // Build a minimal valid PNG with a 10x20 IHDR.
        let buf = vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, // magic
            0x00, 0x00, 0x00, 0x0d, // IHDR length 13
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            0x00, 0x00, 0x00, 0x0a, // width=10
            0x00, 0x00, 0x00, 0x14, // height=20
            0x08, 0x02, 0x00, 0x00, 0x00, // remaining IHDR
            0x00, 0x00, 0x00, 0x00, // CRC placeholder
        ];
        let result = parse_png_dimensions(&buf);
        assert_eq!(result, Some((10, 20)));
    }

    #[test]
    fn parse_png_dimensions_invalid_magic() {
        let buf = vec![0u8; 30];
        assert!(parse_png_dimensions(&buf).is_none());
    }

    #[test]
    fn parse_jpeg_dimensions_basic() {
        // Minimal JPEG: SOI + SOF0 with 10x20 dimensions + padding to satisfy segLen check.
        let buf = vec![
            0xff, 0xd8, // SOI
            0xff, 0xc0, // SOF0 marker
            0x00, 0x0b, // length=11 (includes 2-byte length field)
            0x08, // precision
            0x00, 0x14, // height=20
            0x00, 0x0a, // width=10
            0x01, 0x01, 0x00, // 1 component (id=1, sampling=1x1, table=0)
            0x00, // padding so i + segLen (4 + 11 = 15) does not exceed buffer.len()
        ];
        let result = parse_jpeg_dimensions(&buf);
        assert_eq!(result, Some((10, 20)));
    }

    #[test]
    fn parse_jpeg_dimensions_no_soi() {
        let buf = vec![0u8; 10];
        assert!(parse_jpeg_dimensions(&buf).is_none());
    }

    #[test]
    fn pick_pass1_cap_few_images() {
        assert_eq!(pick_pass1_cap(5, 0), 8000);
    }

    #[test]
    fn pick_pass1_cap_many_images() {
        assert_eq!(pick_pass1_cap(25, 0), 2000);
    }

    #[test]
    fn pick_pass1_cap_override_wins() {
        assert_eq!(pick_pass1_cap(5, 1000), 1000);
        assert_eq!(pick_pass1_cap(25, 4000), 4000);
    }

    #[test]
    fn pick_pass3_native_cap_opus47() {
        assert_eq!(pick_pass3_native_cap(Some("claude-opus-4-7-20250514")), 2576);
    }

    #[test]
    fn pick_pass3_native_cap_default() {
        assert_eq!(pick_pass3_native_cap(Some("claude-sonnet-4-5")), 1568);
        assert_eq!(pick_pass3_native_cap(None), 1568);
    }

    #[test]
    fn estimate_image_tokens_basic() {
        let tokens = estimate_image_tokens(750, 750, 2000);
        assert_eq!(tokens, 750); // 750*750/750 = 750
    }

    #[test]
    fn estimate_image_tokens_capped() {
        let tokens = estimate_image_tokens(2000, 2000, 1568);
        // 2000*2000/750 ≈ 5334, capped at 1568
        assert_eq!(tokens, 1568);
    }

    #[test]
    fn walk_images_finds_direct_and_nested() {
        let messages = serde_json::json!([
            {
                "role": "user",
                "content": [
                    {"type": "image", "source": {"data": "abc", "media_type": "image/png"}},
                    {"type": "tool_result", "content": [
                        {"type": "image", "source": {"data": "def", "media_type": "image/jpeg"}}
                    ]}
                ]
            }
        ]);
        let refs = walk_images(&messages);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].item_idx, None); // direct
        assert_eq!(refs[1].item_idx, Some(0)); // nested in tool_result
    }

    #[test]
    fn pick_eviction_targets_orders_by_age() {
        let messages = serde_json::json!([
            {"role": "user", "content": [{"type": "image", "source": {"data": "a", "media_type": "image/png"}}]},
            {"role": "user", "content": [{"type": "image", "source": {"data": "b", "media_type": "image/png"}}]}
        ]);
        let targets = pick_eviction_targets(&messages);
        assert_eq!(targets.len(), 2);
        // msg_idx 0 should come first (oldest).
        assert_eq!(targets[0].msg_idx, 0);
        assert_eq!(targets[1].msg_idx, 1);
    }
}
