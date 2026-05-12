# T06c: output_config 白名单 + max_tokens 兜底 + mcp_servers 删除

> **并行：** 可与 T02 / T03 / T04a-d / T06a / T06b / T08 / T12 / T17 同时执行。前置：T01。

**Goal:** 实现三个简单的 body 级字段处理：`sanitize_output_config`（白名单仅保留 effort）、`apply_max_tokens_fallback`（缺失时补 8192）、`remove_mcp_servers`。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`（追加）

---

- [ ] **Step 1: 写失败测试**

```rust
pub(crate) fn sanitize_output_config(body: &mut Value, unsafe_tool_followup: bool) {
    todo!()
}

pub(crate) fn apply_max_tokens_fallback(body: &mut Value) {
    todo!()
}

pub(crate) fn remove_mcp_servers(body: &mut Value) {
    todo!()
}

#[cfg(test)]
mod tests_output_config_and_tokens {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_effort_kept() {
        let mut body = json!({"output_config": {"effort": "high", "verbosity": "low"}});
        sanitize_output_config(&mut body, false);
        assert_eq!(body["output_config"]["effort"], "high");
        assert!(body["output_config"].get("verbosity").is_none());
    }

    #[test]
    fn test_unknown_subfields_removed() {
        let mut body = json!({"output_config": {"reasoning": "high", "extra": 1}});
        sanitize_output_config(&mut body, false);
        let oc = body.get("output_config");
        assert!(oc.is_none() || oc.unwrap().as_object().unwrap().is_empty());
    }

    #[test]
    fn test_empty_output_config_deleted() {
        let mut body = json!({"output_config": {"verbosity": "low"}});
        sanitize_output_config(&mut body, false);
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn test_unsafe_tool_followup_removes_effort_too() {
        let mut body = json!({"output_config": {"effort": "high"}});
        sanitize_output_config(&mut body, true);
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn test_no_output_config_no_panic() {
        let mut body = json!({"model": "m"});
        sanitize_output_config(&mut body, false);
        assert!(body.get("output_config").is_none());
    }

    #[test]
    fn test_max_tokens_fallback_when_missing() {
        let mut body = json!({"model": "m", "messages": []});
        apply_max_tokens_fallback(&mut body);
        assert_eq!(body["max_tokens"], 8192);
    }

    #[test]
    fn test_max_tokens_not_overridden_when_present() {
        let mut body = json!({"model": "m", "max_tokens": 4096});
        apply_max_tokens_fallback(&mut body);
        assert_eq!(body["max_tokens"], 4096);
    }

    #[test]
    fn test_max_tokens_fallback_when_null() {
        let mut body = json!({"model": "m", "max_tokens": null});
        apply_max_tokens_fallback(&mut body);
        assert_eq!(body["max_tokens"], 8192);
    }

    #[test]
    fn test_mcp_servers_removed() {
        let mut body = json!({"mcp_servers": [{"name": "fs"}], "model": "m"});
        remove_mcp_servers(&mut body);
        assert!(body.get("mcp_servers").is_none());
        assert_eq!(body["model"], "m");
    }

    #[test]
    fn test_no_mcp_servers_no_panic() {
        let mut body = json!({"model": "m"});
        remove_mcp_servers(&mut body);
        assert!(body.get("mcp_servers").is_none());
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_output_config_and_tokens 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
const OUTPUT_CONFIG_ALLOWED: &[&str] = &["effort"];

pub(crate) fn sanitize_output_config(body: &mut Value, unsafe_tool_followup: bool) {
    let Some(obj) = body.as_object_mut() else { return; };
    let Some(oc) = obj.get_mut("output_config").and_then(|v| v.as_object_mut()) else {
        return;
    };
    oc.retain(|k, _| OUTPUT_CONFIG_ALLOWED.contains(&k.as_str()));
    if unsafe_tool_followup {
        oc.remove("effort");
    }
    if oc.is_empty() {
        obj.remove("output_config");
    }
}

pub(crate) fn apply_max_tokens_fallback(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else { return; };
    let needs_fallback = matches!(obj.get("max_tokens"), None | Some(Value::Null));
    if needs_fallback {
        obj.insert("max_tokens".into(), serde_json::json!(8192));
    }
}

pub(crate) fn remove_mcp_servers(body: &mut Value) {
    if let Some(obj) = body.as_object_mut() {
        obj.remove("mcp_servers");
    }
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_output_config_and_tokens 2>&1 | tail -10
```

Expected: `test result: ok. 10 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs
git commit -m "feat(deepseek): implement output_config whitelist, max_tokens fallback, mcp_servers removal"
```
