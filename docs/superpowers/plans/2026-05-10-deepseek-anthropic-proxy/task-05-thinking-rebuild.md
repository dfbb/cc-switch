# T05: thinking 字段重建 + unsafe_tool_followup 检测

> **并行：** ⚠️ 需等 T02（依赖 `is_reasoner_target`）完成后才能开始。前置：T01 + T02。

**Goal:** 实现 `rebuild_thinking_field`，包含 `detect_tool_history`、`detect_replayable_thinking_before_tool_use` 两个辅助检测函数，以及最终写回 `body["thinking"]` 的逻辑。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`（追加）

---

- [ ] **Step 1: 写失败测试**

```rust
use crate::proxy::providers::deepseek_anthropic::model_mapping::is_reasoner_target;

fn detect_tool_history(body: &Value) -> bool {
    todo!()
}

fn detect_replayable_thinking_before_tool_use(body: &Value) -> bool {
    todo!()
}

/// 返回 effective_thinking_enabled
pub(crate) fn rebuild_thinking_field(body: &mut Value, target_model: &str) -> bool {
    todo!()
}

#[cfg(test)]
mod tests_thinking_rebuild {
    use super::*;
    use serde_json::json;

    fn make_body_with_tool_history(has_thinking: bool) -> Value {
        json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {
                    "role": "assistant",
                    "content": if has_thinking {
                        json!([
                            {"type": "thinking", "thinking": "chain"},
                            {"type": "tool_use", "id": "t1", "name": "bash", "input": {}}
                        ])
                    } else {
                        json!([
                            {"type": "tool_use", "id": "t1", "name": "bash", "input": {}}
                        ])
                    }
                },
                {
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]
                },
                {
                    "role": "user",
                    "content": [{"type": "text", "text": "next turn"}]
                }
            ]
        })
    }

    #[test]
    fn test_detect_tool_history_true_when_tool_result_present() {
        let body = make_body_with_tool_history(false);
        assert!(detect_tool_history(&body));
    }

    #[test]
    fn test_detect_tool_history_false_when_no_tool_result() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]},
                {"role": "assistant", "content": [{"type": "text", "text": "hello"}]}
            ]
        });
        assert!(!detect_tool_history(&body));
    }

    #[test]
    fn test_detect_replayable_thinking_true_when_thinking_before_tool_use() {
        let body = make_body_with_tool_history(true);
        assert!(detect_replayable_thinking_before_tool_use(&body));
    }

    #[test]
    fn test_detect_replayable_thinking_false_when_no_thinking() {
        let body = make_body_with_tool_history(false);
        assert!(!detect_replayable_thinking_before_tool_use(&body));
    }

    #[test]
    fn test_pro_no_tool_history_default_enabled() {
        let mut body = json!({"model": "deepseek-v4-pro", "messages": []});
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-pro");
        assert!(enabled);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn test_pro_unsafe_tool_followup_forced_disabled() {
        let mut body = make_body_with_tool_history(false);
        body["thinking"] = json!(null);  // 未指定
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-pro");
        assert!(!enabled);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_pro_explicit_disabled_respected() {
        let mut body = json!({
            "model": "deepseek-v4-pro",
            "messages": [],
            "thinking": {"type": "disabled"}
        });
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-pro");
        assert!(!enabled);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_flash_no_client_intent_default_disabled() {
        let mut body = json!({"model": "deepseek-v4-flash", "messages": []});
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-flash");
        assert!(!enabled);
        assert_eq!(body["thinking"]["type"], "disabled");
    }

    #[test]
    fn test_flash_client_explicit_enabled_respected() {
        let mut body = json!({
            "model": "deepseek-v4-flash",
            "messages": [],
            "thinking": {"type": "enabled"}
        });
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-flash");
        assert!(enabled);
        assert_eq!(body["thinking"]["type"], "enabled");
    }

    #[test]
    fn test_unknown_thinking_type_falls_back_to_target_default() {
        // unknown type → 回退到 target 默认（flash=false）
        let mut body = json!({
            "model": "deepseek-v4-flash",
            "messages": [],
            "thinking": {"type": "future_type"}
        });
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-flash");
        assert!(!enabled);
    }

    #[test]
    fn test_pro_with_replayable_thinking_stays_enabled() {
        let mut body = make_body_with_tool_history(true);
        let enabled = rebuild_thinking_field(&mut body, "deepseek-v4-pro");
        assert!(enabled);
        assert_eq!(body["thinking"]["type"], "enabled");
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_thinking_rebuild 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
fn detect_tool_history(body: &Value) -> bool {
    let Some(messages) = body.get("messages").and_then(|m| m.as_array()) else {
        return false;
    };
    messages.iter().any(|msg| {
        msg.get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
            })
            .unwrap_or(false)
    })
}

fn detect_replayable_thinking_before_tool_use(body: &Value) -> bool {
    let Some(messages) = body.get("messages").and_then(|m| m.as_array()) else {
        return false;
    };
    // 在每条 assistant 消息中，若有 tool_use 则检查同 content 中是否有先于 tool_use 的 thinking 块
    for msg in messages {
        if msg.get("role").and_then(|r| r.as_str()) != Some("assistant") {
            continue;
        }
        let Some(content) = msg.get("content").and_then(|c| c.as_array()) else {
            continue;
        };
        let has_tool_use = content.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"));
        if !has_tool_use {
            continue;
        }
        let has_thinking = content.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("thinking"));
        if has_thinking {
            return true;
        }
    }
    false
}

pub(crate) fn rebuild_thinking_field(body: &mut Value, target_model: &str) -> bool {
    let obj = body.as_object_mut().expect("body must be object");

    let original_thinking = obj.remove("thinking");
    let client_intent: Option<bool> = original_thinking
        .as_ref()
        .and_then(|t| t.get("type"))
        .and_then(|s| s.as_str())
        .and_then(|s| match s {
            "enabled" => Some(true),
            "disabled" => Some(false),
            _ => {
                log::warn!("unknown thinking.type='{}', falling back to target default", s);
                None
            }
        });

    let target_default = is_reasoner_target(target_model);
    let intended = client_intent.unwrap_or(target_default);

    let has_tool_history = detect_tool_history(body);
    let has_replayable = detect_replayable_thinking_before_tool_use(body);
    let unsafe_tool_followup = has_tool_history && !has_replayable;

    let effective = intended && !unsafe_tool_followup;

    if effective {
        // 保留客户端的 budget_tokens（若有），否则只写 type=enabled
        let budget_tokens = original_thinking
            .as_ref()
            .and_then(|t| t.get("budget_tokens"))
            .cloned();
        let mut thinking_obj = serde_json::Map::new();
        thinking_obj.insert("type".into(), serde_json::Value::String("enabled".into()));
        if let Some(bt) = budget_tokens {
            thinking_obj.insert("budget_tokens".into(), bt);
        }
        obj.insert("thinking".into(), serde_json::Value::Object(thinking_obj));
    } else {
        obj.insert(
            "thinking".into(),
            serde_json::json!({"type": "disabled"}),
        );
    }

    effective
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_thinking_rebuild 2>&1 | tail -10
```

Expected: `test result: ok. 10 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs
git commit -m "feat(deepseek): implement thinking rebuild and unsafe_tool_followup detection"
```
