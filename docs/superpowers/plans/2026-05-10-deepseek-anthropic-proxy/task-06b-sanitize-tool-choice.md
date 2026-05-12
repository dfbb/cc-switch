# T06b: sanitize_tool_choice

> **并行：** 可与 T02 / T03 / T04a-d / T06a / T06c / T08 / T12 / T17 同时执行。前置：T01。

**Goal:** 实现 `sanitize_tool_choice`，白名单保留 type ∈ {none,auto,any,tool}，清理 `disable_parallel_tool_use`，type=tool 缺 name 降级为 auto，非 object / 未知 type 整字段删除。使用 Verdict enum 解决 Rust 借用冲突（持有 tc_obj 借用时不能调 obj.remove）。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs`（追加）

---

- [ ] **Step 1: 写失败测试**

```rust
pub(crate) fn sanitize_tool_choice(body: &mut Value) {
    todo!()
}

#[cfg(test)]
mod tests_tool_choice {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_none_type_kept() {
        let mut body = json!({"tool_choice": {"type": "none"}});
        sanitize_tool_choice(&mut body);
        assert_eq!(body["tool_choice"]["type"], "none");
    }

    #[test]
    fn test_auto_type_kept() {
        let mut body = json!({"tool_choice": {"type": "auto"}});
        sanitize_tool_choice(&mut body);
        assert_eq!(body["tool_choice"]["type"], "auto");
    }

    #[test]
    fn test_any_type_kept() {
        let mut body = json!({"tool_choice": {"type": "any"}});
        sanitize_tool_choice(&mut body);
        assert_eq!(body["tool_choice"]["type"], "any");
    }

    #[test]
    fn test_tool_type_with_name_kept() {
        let mut body = json!({"tool_choice": {"type": "tool", "name": "Bash"}});
        sanitize_tool_choice(&mut body);
        assert_eq!(body["tool_choice"]["type"], "tool");
        assert_eq!(body["tool_choice"]["name"], "Bash");
    }

    #[test]
    fn test_disable_parallel_tool_use_removed() {
        let mut body = json!({"tool_choice": {"type": "auto", "disable_parallel_tool_use": true}});
        sanitize_tool_choice(&mut body);
        assert!(body["tool_choice"].get("disable_parallel_tool_use").is_none());
    }

    #[test]
    fn test_tool_type_missing_name_downgraded_to_auto() {
        let mut body = json!({"tool_choice": {"type": "tool"}});
        sanitize_tool_choice(&mut body);
        assert_eq!(body["tool_choice"]["type"], "auto");
        assert!(body["tool_choice"].get("name").is_none());
    }

    #[test]
    fn test_tool_type_empty_name_downgraded_to_auto() {
        let mut body = json!({"tool_choice": {"type": "tool", "name": ""}});
        sanitize_tool_choice(&mut body);
        assert_eq!(body["tool_choice"]["type"], "auto");
    }

    #[test]
    fn test_unknown_type_removed() {
        let mut body = json!({"tool_choice": {"type": "required"}, "model": "m"});
        sanitize_tool_choice(&mut body);
        assert!(body.get("tool_choice").is_none());
        assert_eq!(body["model"], "m");  // 其余字段不受影响
    }

    #[test]
    fn test_non_object_tool_choice_removed() {
        let mut body = json!({"tool_choice": "auto", "model": "m"});
        sanitize_tool_choice(&mut body);
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn test_no_tool_choice_no_panic() {
        let mut body = json!({"model": "m"});
        sanitize_tool_choice(&mut body);
        assert!(body.get("tool_choice").is_none());
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_tool_choice 2>&1 | tail -15
```

- [ ] **Step 3: 实现（使用 Verdict enum 解决借用冲突）**

```rust
pub(crate) fn sanitize_tool_choice(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else { return; };
    if !obj.contains_key("tool_choice") { return; }

    enum Verdict {
        Keep,
        RemoveNonObject,
        RemoveUnknown(String),
    }

    let verdict = {
        let tc = obj.get_mut("tool_choice").expect("contains_key checked above");
        match tc.as_object_mut() {
            None => Verdict::RemoveNonObject,
            Some(tc_obj) => {
                let kind = tc_obj
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                match kind.as_str() {
                    "none" | "auto" | "any" | "tool" => {
                        tc_obj.remove("disable_parallel_tool_use");
                        if kind == "tool"
                            && !tc_obj
                                .get("name")
                                .and_then(|v| v.as_str())
                                .is_some_and(|s| !s.is_empty())
                        {
                            tc_obj.insert("type".into(), Value::String("auto".into()));
                            tc_obj.remove("name");
                            log::warn!("tool_choice type=tool without name → downgraded to auto");
                        }
                        Verdict::Keep
                    }
                    _ => Verdict::RemoveUnknown(kind),
                }
            }
        }
    };

    match verdict {
        Verdict::Keep => {}
        Verdict::RemoveNonObject => {
            obj.remove("tool_choice");
            log::warn!("removed non-object tool_choice");
        }
        Verdict::RemoveUnknown(kind) => {
            obj.remove("tool_choice");
            log::warn!("removed tool_choice with unknown type: {}", kind);
        }
    }
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::request_sanitizer::tests_tool_choice 2>&1 | tail -10
```

Expected: `test result: ok. 10 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs
git commit -m "feat(deepseek): implement sanitize_tool_choice with Verdict pattern"
```
