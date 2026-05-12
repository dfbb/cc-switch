# T04c: normalize_tool_result_content

> **并行：** 可与 T02 / T03 / T04a / T04b / T04d / T06a-c / T08 / T12 / T17 同时执行。前置：T01。

**Goal:** 实现 `normalize_tool_result_content`，将 tool_result 的 content 字段从数组/dict 形态归一化为字符串（DeepSeek Anthropic 端点要求）。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`（追加函数和测试）

---

- [ ] **Step 1: 写失败测试**

```rust
pub(crate) fn normalize_tool_result_content(messages: &mut Vec<Value>) {
    todo!()
}

#[cfg(test)]
mod tests_normalize_tool_result {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_string_content_unchanged() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "already string"}]
        })];
        normalize_tool_result_content(&mut msgs);
        assert_eq!(msgs[0]["content"][0]["content"], "already string");
    }

    #[test]
    fn test_array_of_text_blocks_joined() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": [
                    {"type": "text", "text": "line1"},
                    {"type": "text", "text": "line2"}
                ]
            }]
        })];
        normalize_tool_result_content(&mut msgs);
        assert_eq!(msgs[0]["content"][0]["content"], "line1\nline2");
    }

    #[test]
    fn test_array_with_non_text_block_json_serialized() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": [
                    {"type": "text", "text": "result:"},
                    {"type": "image", "source": {"type": "url", "url": "http://x"}}
                ]
            }]
        })];
        normalize_tool_result_content(&mut msgs);
        let s = msgs[0]["content"][0]["content"].as_str().unwrap();
        assert!(s.contains("result:"));
        assert!(s.contains("image"));
    }

    #[test]
    fn test_dict_content_json_serialized() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": "t1",
                "content": {"key": "val"}
            }]
        })];
        normalize_tool_result_content(&mut msgs);
        let s = msgs[0]["content"][0]["content"].as_str().unwrap();
        assert!(s.contains("key"));
        assert!(s.contains("val"));
    }

    #[test]
    fn test_null_content_becomes_empty_string() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": "t1"}]
        })];
        normalize_tool_result_content(&mut msgs);
        assert_eq!(msgs[0]["content"][0]["content"], "");
    }

    #[test]
    fn test_non_tool_result_blocks_not_touched() {
        let mut msgs = vec![json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "hi"},
                {"type": "tool_result", "tool_use_id": "t1", "content": ["should be string"]}
            ]
        })];
        normalize_tool_result_content(&mut msgs);
        // text block 不变
        assert_eq!(msgs[0]["content"][0]["text"], "hi");
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_normalize_tool_result 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
pub(crate) fn normalize_tool_result_content(messages: &mut Vec<Value>) {
    for msg in messages.iter_mut() {
        let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) else {
            continue;
        };
        for block in content.iter_mut() {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let Some(obj) = block.as_object_mut() else { continue; };
            let normalized: String = match obj.get("content") {
                None => String::new(),
                Some(Value::String(s)) => continue,  // 已经是 string，不变
                Some(Value::Array(arr)) => {
                    arr.iter()
                        .map(|item| {
                            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                                item.get("text")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("")
                                    .to_string()
                            } else {
                                serde_json::to_string(item).unwrap_or_default()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                Some(other) => serde_json::to_string(other).unwrap_or_default(),
            };
            obj.insert("content".into(), Value::String(normalized));
        }
    }
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_normalize_tool_result 2>&1 | tail -10
```

Expected: `test result: ok. 6 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs
git commit -m "feat(deepseek): implement normalize_tool_result_content"
```
