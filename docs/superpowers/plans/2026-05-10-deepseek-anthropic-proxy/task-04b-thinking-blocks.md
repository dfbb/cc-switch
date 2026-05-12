# T04b: sanitize_thinking_blocks + strip_reasoning_content

> **并行：** 可与 T02 / T03 / T04a / T04c / T04d / T06a-c / T08 / T12 / T17 同时执行。前置：T01。

**Goal:** 实现消息历史中 thinking 块过滤（`sanitize_thinking_blocks`）和 assistant 顶层 `reasoning_content` 字段删除（`strip_reasoning_content`）。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`（追加函数和测试）

---

- [ ] **Step 1: 写失败测试**

在 `request_sanitizer.rs` 追加：

```rust
pub(crate) fn sanitize_thinking_blocks(messages: &mut Vec<Value>, effective_thinking_enabled: bool) {
    todo!()
}

pub(crate) fn strip_reasoning_content(messages: &mut Vec<Value>) {
    todo!()
}

#[cfg(test)]
mod tests_thinking_blocks {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_thinking_kept_when_enabled() {
        let mut msgs = vec![json!({
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "chain"},
                {"type": "text", "text": "answer"}
            ]
        })];
        sanitize_thinking_blocks(&mut msgs, true);
        let c = msgs[0]["content"].as_array().unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0]["type"], "thinking");
    }

    #[test]
    fn test_thinking_dropped_when_disabled() {
        let mut msgs = vec![json!({
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "chain"},
                {"type": "text", "text": "answer"}
            ]
        })];
        sanitize_thinking_blocks(&mut msgs, false);
        let c = msgs[0]["content"].as_array().unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["type"], "text");
    }

    #[test]
    fn test_redacted_thinking_always_dropped() {
        let mut msgs = vec![json!({
            "role": "assistant",
            "content": [
                {"type": "redacted_thinking", "data": "enc"},
                {"type": "text", "text": "answer"}
            ]
        })];
        sanitize_thinking_blocks(&mut msgs, true);  // enabled=true 仍然删除 redacted
        let c = msgs[0]["content"].as_array().unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["type"], "text");
    }

    #[test]
    fn test_user_content_not_touched() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{"type": "thinking", "thinking": "x"}]
        })];
        sanitize_thinking_blocks(&mut msgs, false);
        // user 消息不动
        let c = msgs[0]["content"].as_array().unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0]["type"], "thinking");
    }

    #[test]
    fn test_strip_reasoning_content_deleted() {
        let mut msgs = vec![json!({
            "role": "assistant",
            "reasoning_content": "some cot",
            "content": [{"type": "text", "text": "answer"}]
        })];
        strip_reasoning_content(&mut msgs);
        assert!(msgs[0].get("reasoning_content").is_none());
        // content 不受影响
        assert!(msgs[0]["content"].as_array().is_some());
    }

    #[test]
    fn test_strip_reasoning_content_thinking_block_unaffected() {
        let mut msgs = vec![json!({
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "chain"},
                {"type": "text", "text": "answer"}
            ]
        })];
        strip_reasoning_content(&mut msgs);
        // content 中的 thinking 块不受 strip_reasoning_content 影响
        let c = msgs[0]["content"].as_array().unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0]["type"], "thinking");
    }

    #[test]
    fn test_strip_reasoning_content_user_msg_unaffected() {
        let mut msgs = vec![json!({
            "role": "user",
            "reasoning_content": "should not exist but ignore",
            "content": [{"type": "text", "text": "hi"}]
        })];
        strip_reasoning_content(&mut msgs);
        // 对所有消息统一删除该字段（防御性清理）
        assert!(msgs[0].get("reasoning_content").is_none());
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_thinking_blocks 2>&1 | tail -15
```

Expected: FAILED（todo!() panic）

- [ ] **Step 3: 实现**

```rust
pub(crate) fn sanitize_thinking_blocks(messages: &mut Vec<Value>, effective_thinking_enabled: bool) {
    for msg in messages.iter_mut() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        content.retain(|block| {
            let Some(t) = block.get("type").and_then(|s| s.as_str()) else {
                return true;
            };
            if t.starts_with("redacted_thinking") {
                return false;
            }
            if t == "thinking" && !effective_thinking_enabled {
                return false;
            }
            true
        });
    }
}

pub(crate) fn strip_reasoning_content(messages: &mut Vec<Value>) {
    for msg in messages.iter_mut() {
        if let Some(obj) = msg.as_object_mut() {
            obj.remove("reasoning_content");
        }
    }
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_thinking_blocks 2>&1 | tail -10
```

Expected: `test result: ok. 7 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs
git commit -m "feat(deepseek): implement sanitize_thinking_blocks and strip_reasoning_content"
```
