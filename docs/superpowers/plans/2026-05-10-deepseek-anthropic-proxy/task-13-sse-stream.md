# T13: sse_stream.rs — patch_sse_event + wrap_sse_stream

> **并行：** ⚠️ 需等 T12 完成（依赖 `SseBlockPolicyState` + `transform_native_sse_block_event`）。

**Goal:** 实现 `patch_sse_event`（薄封装）和 `wrap_sse_stream`（缓冲 + `\n\n` 切分 + per-event patch + yield with `\n\n`）。

**Files:**
- Create: `src-tauri/src/proxy/providers/deepseek_anthropic/sse_stream.rs`

---

- [ ] **Step 1: 写失败测试**

```rust
pub fn patch_sse_event(
    _event: &str,
    _state: &mut SseBlockPolicyState,
    _fake_model: &str,
    _thinking_enabled: bool,
) -> Vec<String> {
    todo!()
}

pub fn wrap_sse_stream<S>(
    _upstream: S,
    _fake_model: String,
    _thinking_enabled: bool,
) -> impl futures::Stream<Item = Result<bytes::Bytes, std::io::Error>>
where
    S: futures::Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static,
{
    futures::stream::empty()
}

#[cfg(test)]
mod tests_sse_stream {
    use super::*;
    use crate::proxy::providers::deepseek_anthropic::sse_state::SseBlockPolicyState;
    use bytes::Bytes;
    use futures::StreamExt;
    use serde_json::json;

    fn make_state() -> SseBlockPolicyState {
        SseBlockPolicyState::default()
    }

    fn event(event_type: &str, data: serde_json::Value) -> String {
        format!("event: {}\ndata: {}", event_type, data)
    }

    // --- patch_sse_event 单元测试 ---

    #[test]
    fn test_patch_event_no_trailing_newline() {
        let mut state = make_state();
        let e = event("ping", json!({}));
        let result = patch_sse_event(&e, &mut state, "fake", false);
        assert_eq!(result.len(), 1);
        assert!(!result[0].ends_with('\n'), "patch_sse_event elements must not end with \\n");
    }

    #[test]
    fn test_patch_event_message_start_rewrites_model() {
        let mut state = make_state();
        let e = event("message_start", json!({
            "type": "message_start",
            "message": {"model": "deepseek-v4-pro"}
        }));
        let result = patch_sse_event(&e, &mut state, "claude-opus-4-7", true);
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("claude-opus-4-7"));
    }

    #[test]
    fn test_patch_event_dropped_returns_empty() {
        let mut state = make_state();
        let e = event("content_block_start", json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": {"type": "thinking"}
        }));
        let result = patch_sse_event(&e, &mut state, "fake", false);
        assert!(result.is_empty());
    }

    // --- wrap_sse_stream 集成测试 ---

    fn bytes_from_events(events: Vec<&str>) -> Vec<Bytes> {
        // Each event with \n\n suffix — simulates SSE wire format
        events.into_iter()
            .map(|e| Bytes::from(format!("{}\n\n", e)))
            .collect()
    }

    async fn collect_stream_output(
        chunks: Vec<Bytes>,
        fake_model: &str,
        thinking_enabled: bool,
    ) -> Vec<String> {
        let upstream = futures::stream::iter(chunks.into_iter().map(Ok::<_, std::io::Error>));
        let stream = wrap_sse_stream(upstream, fake_model.to_string(), thinking_enabled);
        let collected: Vec<_> = stream
            .map(|r| String::from_utf8(r.unwrap().to_vec()).unwrap())
            .collect::<Vec<_>>()
            .await;
        collected
    }

    #[tokio::test]
    async fn test_wrap_stream_each_event_ends_with_double_newline() {
        let raw = format!("event: ping\ndata: {{}}\n\n");
        let chunks = vec![Bytes::from(raw)];
        let output = collect_stream_output(chunks, "fake", false).await;
        for item in &output {
            assert!(item.ends_with("\n\n"), "each yielded item must end with \\n\\n: {:?}", item);
        }
    }

    #[tokio::test]
    async fn test_wrap_stream_chunk_split_across_boundary() {
        // Split the event mid-way across two chunks
        let full = format!("event: ping\ndata: {{}}\n\n");
        let (left, right) = full.split_at(full.len() / 2);
        let chunks = vec![
            Bytes::from(left.to_string()),
            Bytes::from(right.to_string()),
        ];
        let output = collect_stream_output(chunks, "fake", false).await;
        assert!(!output.is_empty(), "event split across chunks should still be emitted");
        let full_output = output.join("");
        assert!(full_output.contains("ping"));
    }

    #[tokio::test]
    async fn test_wrap_stream_drops_thinking_events_when_disabled() {
        let start_event = format!(
            "event: content_block_start\ndata: {}\n\n",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking"}})
        );
        let chunks = vec![Bytes::from(start_event)];
        let output = collect_stream_output(chunks, "fake", false).await;
        // thinking disabled → no output for that event
        assert!(
            output.is_empty() || !output.iter().any(|s| s.contains("thinking")),
            "thinking block should be dropped when disabled"
        );
    }

    #[tokio::test]
    async fn test_wrap_stream_model_rewritten_in_message_start() {
        let evt = format!(
            "event: message_start\ndata: {}\n\n",
            json!({"type":"message_start","message":{"model":"deepseek-v4-pro"}})
        );
        let chunks = vec![Bytes::from(evt)];
        let output = collect_stream_output(chunks, "claude-sonnet-4-6", false).await;
        let joined = output.join("");
        assert!(joined.contains("claude-sonnet-4-6"), "model should be rewritten");
        assert!(!joined.contains("deepseek-v4-pro"), "original model should not appear");
    }

    #[tokio::test]
    async fn test_wrap_stream_multi_event_each_has_terminator() {
        let events = vec![
            format!("event: ping\ndata: {{}}\n\n"),
            format!(
                "event: message_start\ndata: {}\n\n",
                json!({"type":"message_start","message":{"model":"deepseek-v4-pro"}})
            ),
        ];
        let chunks: Vec<Bytes> = events.iter().map(|e| Bytes::from(e.clone())).collect();
        let output = collect_stream_output(chunks, "fake", false).await;
        for item in &output {
            assert!(item.ends_with("\n\n"), "every yielded event must have \\n\\n: {:?}", item);
        }
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::sse_stream::tests_sse_stream 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
use bytes::Bytes;
use futures::{Stream, StreamExt};
use crate::proxy::providers::deepseek_anthropic::sse_state::{
    SseBlockPolicyState, transform_native_sse_block_event,
};

pub fn patch_sse_event(
    event: &str,
    state: &mut SseBlockPolicyState,
    fake_model: &str,
    thinking_enabled: bool,
) -> Vec<String> {
    transform_native_sse_block_event(event, state, fake_model, thinking_enabled)
}

pub fn wrap_sse_stream<S>(
    upstream: S,
    fake_model: String,
    thinking_enabled: bool,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    let mut buffer: Vec<u8> = Vec::new();
    let mut state = SseBlockPolicyState::default();

    upstream.flat_map(move |chunk_result| {
        let out: Vec<Result<Bytes, std::io::Error>> = match chunk_result {
            Err(e) => vec![Err(e)],
            Ok(chunk) => {
                buffer.extend_from_slice(&chunk);
                let mut events_out: Vec<Result<Bytes, std::io::Error>> = Vec::new();

                // Split buffer on \n\n boundary
                loop {
                    if let Some(pos) = find_double_newline(&buffer) {
                        let event_bytes = buffer[..pos].to_vec();
                        buffer.drain(..pos + 2); // consume including \n\n

                        let event_str = match std::str::from_utf8(&event_bytes) {
                            Ok(s) => s.trim_end_matches('\n').to_string(),
                            Err(_) => {
                                log::warn!("non-utf8 SSE event chunk");
                                continue;
                            }
                        };

                        let patched = patch_sse_event(&event_str, &mut state, &fake_model, thinking_enabled);
                        for e in patched {
                            events_out.push(Ok(Bytes::from(format!("{}\n\n", e))));
                        }
                    } else {
                        break;
                    }
                }
                events_out
            }
        };
        futures::stream::iter(out)
    })
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::sse_stream::tests_sse_stream 2>&1 | tail -10
```

Expected: `test result: ok. N passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/sse_stream.rs
git commit -m "feat(deepseek): implement sse_stream wrap_sse_stream + patch_sse_event"
```
