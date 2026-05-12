# T06a: tools 黑名单过滤

> **并行：** 可与 T02 / T03 / T04a-d / T06b / T06c / T08 / T12 / T17 同时执行。前置：T01。

**Goal:** 实现 `filter_server_tools`，从 `body["tools"]` 数组中移除 Anthropic server tools（仅当 `tool["type"]` 为黑名单值时删除；无 `type` 字段的普通 client tools 保留）。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`（追加）

---

- [ ] **Step 1: 写失败测试**

```rust
pub(crate) fn filter_server_tools(body: &mut Value) {
    todo!()
}

#[cfg(test)]
mod tests_tools_blacklist {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_plain_client_tool_preserved() {
        let mut body = json!({
            "tools": [{"name": "Bash", "description": "run bash", "input_schema": {}}]
        });
        filter_server_tools(&mut body);
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        assert_eq!(body["tools"][0]["name"], "Bash");
    }

    #[test]
    fn test_web_search_type_removed() {
        let mut body = json!({
            "tools": [
                {"type": "web_search_20250305", "name": "web_search"},
                {"name": "Bash", "input_schema": {}}
            ]
        });
        filter_server_tools(&mut body);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "Bash");
    }

    #[test]
    fn test_web_search_name_double_guard_removed() {
        // 无 type 字段但 name=="web_search" 也删
        let mut body = json!({
            "tools": [
                {"name": "web_search", "input_schema": {}},
                {"name": "Bash", "input_schema": {}}
            ]
        });
        filter_server_tools(&mut body);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "Bash");
    }

    #[test]
    fn test_computer_type_removed() {
        let mut body = json!({
            "tools": [{"type": "computer_20250124", "name": "computer"}]
        });
        filter_server_tools(&mut body);
        assert!(body["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_text_editor_type_removed() {
        let mut body = json!({
            "tools": [{"type": "text_editor_20250124", "name": "str_replace_based_edit_tool"}]
        });
        filter_server_tools(&mut body);
        assert!(body["tools"].as_array().unwrap().is_empty());
    }

    #[test]
    fn test_no_tools_field_no_panic() {
        let mut body = json!({"model": "m"});
        filter_server_tools(&mut body);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_web_fetch_type_removed() {
        let mut body = json!({
            "tools": [
                {"type": "web_fetch", "name": "web_fetch"},
                {"name": "mcp_tool", "input_schema": {}}
            ]
        });
        filter_server_tools(&mut body);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "mcp_tool");
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_tools_blacklist 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
pub(crate) fn filter_server_tools(body: &mut Value) {
    let Some(tools) = body.get_mut("tools").and_then(|v| v.as_array_mut()) else {
        return;
    };
    tools.retain(|tool| {
        // 黑名单：type 以下列前缀开头时删除
        let type_val = tool.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if type_val.starts_with("web_search")
            || type_val.starts_with("web_fetch")
            || type_val.starts_with("computer_")
            || type_val.starts_with("text_editor_")
        {
            return false;
        }
        // 双重保险：name=="web_search" 或 "web_fetch" 也删
        let name_val = tool.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if name_val == "web_search" || name_val == "web_fetch" {
            return false;
        }
        true
    });
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_tools_blacklist 2>&1 | tail -10
```

Expected: `test result: ok. 7 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs
git commit -m "feat(deepseek): implement filter_server_tools blacklist"
```
