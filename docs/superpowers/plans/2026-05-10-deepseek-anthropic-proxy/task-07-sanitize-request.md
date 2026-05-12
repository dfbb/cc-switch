# T07: sanitize_request 编排

> **并行：** ⚠️ 需等 T04a + T04b + T04c + T04d + T05 + T06a + T06b + T06c 全部完成后才能开始。

**Goal:** 实现 `sanitize_request` 主函数与 `SanitizeResult` 结构体，按 ①-⑩ 顺序调用各子函数，并在 request body 末尾清理 `_dsk_accepted` 临时字段（由 T08-T11 的 tool_repair 写入）。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`（追加 SanitizeResult + sanitize_request）

---

- [ ] **Step 1: 写失败测试**

```rust
use crate::proxy::providers::deepseek_anthropic::model_mapping::map_claude_to_deepseek;
use crate::proxy::providers::deepseek_anthropic::tool_repair::repair_tool_order;

pub struct SanitizeResult {
    pub fake_model: String,
    pub target_model: String,
    pub effective_thinking_enabled: bool,
}

pub fn sanitize_request(body: &mut Value) -> SanitizeResult {
    todo!()
}

#[cfg(test)]
mod tests_sanitize_request {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_model_mapping_applied() {
        let mut body = json!({
            "model": "claude-opus-4-7",
            "messages": [],
            "max_tokens": 1024
        });
        let result = sanitize_request(&mut body);
        assert_eq!(result.fake_model, "claude-opus-4-7");
        assert_eq!(result.target_model, "deepseek-v4-pro");
        assert_eq!(body["model"], "deepseek-v4-pro");
    }

    #[test]
    fn test_max_tokens_fallback_applied() {
        let mut body = json!({
            "model": "claude-sonnet-4-6",
            "messages": []
        });
        sanitize_request(&mut body);
        assert_eq!(body["max_tokens"], 8192);
    }

    #[test]
    fn test_mcp_servers_removed() {
        let mut body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [],
            "mcp_servers": [{"name": "fs"}]
        });
        sanitize_request(&mut body);
        assert!(body.get("mcp_servers").is_none());
    }

    #[test]
    fn test_server_tools_removed_client_tools_kept() {
        let mut body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [],
            "tools": [
                {"type": "web_search_20250305", "name": "web_search"},
                {"name": "Bash", "input_schema": {}}
            ]
        });
        sanitize_request(&mut body);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "Bash");
    }

    #[test]
    fn test_flash_thinking_disabled() {
        let mut body = json!({
            "model": "claude-sonnet-4-6",
            "messages": []
        });
        let result = sanitize_request(&mut body);
        assert!(!result.effective_thinking_enabled);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_stream_field_preserved() {
        let mut body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [],
            "stream": false
        });
        sanitize_request(&mut body);
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn test_dsk_accepted_cleaned_up() {
        // 模拟 tool_repair 留下的 _dsk_accepted 字段
        let mut body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [{
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok", "_dsk_accepted": true}]
            }]
        });
        sanitize_request(&mut body);
        let block = &body["messages"][0]["content"][0];
        assert!(block.get("_dsk_accepted").is_none());
    }

    #[test]
    fn test_empty_content_fallback_applied_after_stripping() {
        let mut body = json!({
            "model": "claude-sonnet-4-6",
            "messages": [{
                "role": "user",
                "content": [{"type": "image", "source": {}}]
            }]
        });
        sanitize_request(&mut body);
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_sanitize_request 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
pub struct SanitizeResult {
    pub fake_model: String,
    pub target_model: String,
    pub effective_thinking_enabled: bool,
}

pub fn sanitize_request(body: &mut Value) -> SanitizeResult {
    // ① 模型名映射
    let fake_model = body
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let target_model = map_claude_to_deepseek(&fake_model).to_string();
    if let Some(obj) = body.as_object_mut() {
        obj.insert("model".into(), Value::String(target_model.clone()));
    }

    // ② tools 黑名单
    filter_server_tools(body);

    // ③ thinking 字段重建（计算 effective_thinking_enabled）
    let effective_thinking_enabled = rebuild_thinking_field(body, &target_model);

    // ④ output_config 白名单（需要 unsafe_tool_followup，用 effective 反推）
    let unsafe_tool_followup = {
        let ite = effective_thinking_enabled;
        let is_pro_or_reasoner = crate::proxy::providers::deepseek_anthropic::model_mapping::is_reasoner_target(&target_model);
        // 只有 pro 且 intended=true 但 effective=false 时才是 unsafe_tool_followup
        // 简化：通过 detect_tool_history 重新计算（已在 rebuild_thinking_field 内调用，此处 body 未变）
        detect_tool_history(body) && !detect_replayable_thinking_before_tool_use(body)
    };
    sanitize_output_config(body, unsafe_tool_followup);
    remove_mcp_servers(body);

    // ⑤ messages 净化流水线
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        strip_unsupported_attachments(messages);
        sanitize_thinking_blocks(messages, effective_thinking_enabled);
        normalize_tool_result_content(messages);
        strip_reasoning_content(messages);
        // 空 content 兜底
        for msg in messages.iter_mut() {
            if let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                if content.is_empty() {
                    content.push(serde_json::json!({"type": "text", "text": "(empty)"}));
                }
            }
        }
    }

    // ⑥ context_management.edits 过滤
    filter_context_management_edits(body);

    // ⑦ tool 顺序修复（plan-then-apply；写入临时 _dsk_accepted 字段）
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        repair_tool_order(messages);
    }

    // ⑧ max_tokens 兜底
    apply_max_tokens_fallback(body);

    // ⑨ tool_choice 白名单
    sanitize_tool_choice(body);

    // ⑩ stream 字段保留客户端原值（不操作）

    // 清理 _dsk_accepted 临时字段（tool_repair 写入）
    if let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for msg in messages.iter_mut() {
            if let Some(content) = msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                for block in content.iter_mut() {
                    if let Some(obj) = block.as_object_mut() {
                        obj.remove("_dsk_accepted");
                    }
                }
            }
        }
    }

    SanitizeResult {
        fake_model,
        target_model,
        effective_thinking_enabled,
    }
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_sanitize_request 2>&1 | tail -10
```

Expected: `test result: ok. 8 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs
git commit -m "feat(deepseek): implement sanitize_request orchestrator"
```
