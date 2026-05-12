# T04d: filter_context_management_edits

> **并行：** 可与 T02 / T03 / T04a-c / T06a-c / T08 / T12 / T17 同时执行。前置：T01。

**Goal:** 实现 `filter_context_management_edits`，删除 body 顶层 `context_management.edits` 数组中 `type` 以 `clear_thinking_` 开头的条目；若 edits 变空则级联删除 edits → context_management。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`（追加函数和测试）

---

- [ ] **Step 1: 写失败测试**

```rust
pub(crate) fn filter_context_management_edits(body: &mut Value) {
    todo!()
}

#[cfg(test)]
mod tests_context_management {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_clear_thinking_edit_removed() {
        let mut body = json!({
            "context_management": {
                "edits": [
                    {"type": "clear_thinking_blocks"},
                    {"type": "keep_this"}
                ]
            }
        });
        filter_context_management_edits(&mut body);
        let edits = body["context_management"]["edits"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["type"], "keep_this");
    }

    #[test]
    fn test_edits_all_removed_field_deleted() {
        let mut body = json!({
            "context_management": {
                "edits": [{"type": "clear_thinking_history"}]
            }
        });
        filter_context_management_edits(&mut body);
        assert!(body["context_management"].get("edits").is_none());
    }

    #[test]
    fn test_context_management_empty_object_deleted() {
        let mut body = json!({
            "context_management": {
                "edits": [{"type": "clear_thinking_blocks"}]
            }
        });
        filter_context_management_edits(&mut body);
        assert!(body.get("context_management").is_none());
    }

    #[test]
    fn test_context_management_other_fields_kept() {
        let mut body = json!({
            "context_management": {
                "edits": [{"type": "clear_thinking_blocks"}],
                "other_field": "value"
            }
        });
        filter_context_management_edits(&mut body);
        // other_field 保留，context_management 仍存在
        assert_eq!(body["context_management"]["other_field"], "value");
        assert!(body["context_management"].get("edits").is_none());
    }

    #[test]
    fn test_no_context_management_no_panic() {
        let mut body = json!({"model": "m", "messages": []});
        filter_context_management_edits(&mut body);
        // 不 panic，body 不变
        assert_eq!(body["model"], "m");
    }

    #[test]
    fn test_multiple_clear_thinking_variants_all_removed() {
        let mut body = json!({
            "context_management": {
                "edits": [
                    {"type": "clear_thinking_blocks"},
                    {"type": "clear_thinking_history"},
                    {"type": "clear_thinking_foo_bar"},
                    {"type": "normal_edit"}
                ]
            }
        });
        filter_context_management_edits(&mut body);
        let edits = body["context_management"]["edits"].as_array().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0]["type"], "normal_edit");
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_context_management 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
pub(crate) fn filter_context_management_edits(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else { return; };
    let Some(cm) = obj.get_mut("context_management").and_then(|v| v.as_object_mut()) else {
        return;
    };

    if let Some(edits) = cm.get_mut("edits").and_then(|v| v.as_array_mut()) {
        edits.retain(|e| {
            e.get("type")
                .and_then(|t| t.as_str())
                .map(|t| !t.starts_with("clear_thinking_"))
                .unwrap_or(true)
        });
    }

    // 若 edits 变空则删除
    let edits_empty = cm
        .get("edits")
        .and_then(|v| v.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(false);
    if edits_empty {
        cm.remove("edits");
    }

    // 若 context_management 变空对象则整体删除
    if cm.is_empty() {
        obj.remove("context_management");
    }
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_context_management 2>&1 | tail -10
```

Expected: `test result: ok. 6 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs
git commit -m "feat(deepseek): implement filter_context_management_edits"
```
