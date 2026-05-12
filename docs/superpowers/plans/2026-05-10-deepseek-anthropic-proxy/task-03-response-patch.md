# T03: response_patch.rs

> **并行：** 可与 T02 / T04a-d / T06a-c / T08 / T12 / T17 同时执行。前置：T01。

**Goal:** 实现 `patch_non_streaming_response`，处理非流式响应的模型名伪装、thinking 块过滤、空 content 兜底。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/response_patch.rs`

---

- [ ] **Step 1: 写失败测试**

```rust
use serde_json::{json, Value};

pub fn patch_non_streaming_response(
    body: &mut Value,
    fake_model: &str,
    effective_thinking_enabled: bool,
) {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_model_field_replaced() {
        let mut body = json!({"model": "deepseek-v4-pro", "content": []});
        patch_non_streaming_response(&mut body, "claude-opus-4-7", true);
        assert_eq!(body["model"], "claude-opus-4-7");
    }

    #[test]
    fn test_redacted_thinking_always_dropped() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "content": [
                {"type": "redacted_thinking", "data": "enc"},
                {"type": "text", "text": "hi"}
            ]
        });
        patch_non_streaming_response(&mut body, "claude-opus-4-7", true);
        let content = body["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn test_thinking_dropped_when_disabled() {
        let mut body = json!({
            "model": "m",
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "text", "text": "answer"}
            ]
        });
        patch_non_streaming_response(&mut body, "fake", false);
        let content = body["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }

    #[test]
    fn test_thinking_kept_when_enabled() {
        let mut body = json!({
            "model": "m",
            "content": [
                {"type": "thinking", "thinking": "chain"},
                {"type": "text", "text": "answer"}
            ]
        });
        patch_non_streaming_response(&mut body, "fake", true);
        let content = body["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "thinking");
    }

    #[test]
    fn test_empty_content_fallback() {
        // 过滤后 content 为空时补占位
        let mut body = json!({
            "model": "m",
            "content": [{"type": "thinking", "thinking": "chain"}]
        });
        patch_non_streaming_response(&mut body, "fake", false);
        let content = body["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "(empty)");
    }

    #[test]
    fn test_no_model_field_no_panic() {
        let mut body = json!({"content": [{"type": "text", "text": "hi"}]});
        patch_non_streaming_response(&mut body, "fake", true);
        // model 字段本来不存在，不应插入
        assert!(body.get("model").is_none());
    }

    #[test]
    fn test_non_object_body_no_panic() {
        let mut body = json!("not an object");
        patch_non_streaming_response(&mut body, "fake", true);
        // 不 panic
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::response_patch 2>&1 | tail -15
```

Expected: FAILED（todo!() panic）

- [ ] **Step 3: 实现**

```rust
use serde_json::{json, Value};

pub fn patch_non_streaming_response(
    body: &mut Value,
    fake_model: &str,
    effective_thinking_enabled: bool,
) {
    let Some(obj) = body.as_object_mut() else { return; };

    if obj.contains_key("model") {
        obj.insert("model".into(), Value::String(fake_model.into()));
    }

    if let Some(content) = obj.get_mut("content").and_then(|v| v.as_array_mut()) {
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
        if content.is_empty() {
            content.push(json!({"type": "text", "text": "(empty)"}));
        }
    }
}

#[cfg(test)]
mod tests {
    // ... 保持 Step 1 中的测试
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::response_patch 2>&1 | tail -10
```

Expected: `test result: ok. 7 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/response_patch.rs
git commit -m "feat(deepseek): implement patch_non_streaming_response"
```
