# T02: model_mapping.rs

> **并行：** 可与 T03 / T04a-d / T06a-c / T08 / T12 / T17 同时执行。前置：T01。

**Goal:** 实现 `map_claude_to_deepseek` 与 `is_reasoner_target` 两个纯函数，所有后续逻辑依赖这两个函数来决定目标模型名和是否为推理模式。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/model_mapping.rs`

---

- [ ] **Step 1: 写失败测试**

将 `model_mapping.rs` 内容替换为：

```rust
pub fn map_claude_to_deepseek(claude_model: &str) -> &'static str {
    todo!()
}

pub fn is_reasoner_target(deepseek_model: &str) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opus_maps_to_pro() {
        assert_eq!(map_claude_to_deepseek("claude-opus-4-7"), "deepseek-v4-pro");
    }

    #[test]
    fn test_sonnet_maps_to_flash() {
        assert_eq!(map_claude_to_deepseek("claude-sonnet-4-6"), "deepseek-v4-flash");
    }

    #[test]
    fn test_haiku_maps_to_flash() {
        assert_eq!(map_claude_to_deepseek("claude-haiku-3-5"), "deepseek-v4-flash");
    }

    #[test]
    fn test_unknown_maps_to_flash() {
        assert_eq!(map_claude_to_deepseek("some-unknown-model"), "deepseek-v4-flash");
    }

    #[test]
    fn test_case_insensitive_opus() {
        assert_eq!(map_claude_to_deepseek("CLAUDE-OPUS-4"), "deepseek-v4-pro");
    }

    #[test]
    fn test_is_reasoner_target_pro() {
        assert!(is_reasoner_target("deepseek-v4-pro"));
    }

    #[test]
    fn test_is_reasoner_target_reasoner() {
        assert!(is_reasoner_target("deepseek-reasoner"));
    }

    #[test]
    fn test_is_reasoner_target_flash() {
        assert!(!is_reasoner_target("deepseek-v4-flash"));
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::model_mapping 2>&1 | tail -20
```

Expected: FAILED（todo!() panic）

- [ ] **Step 3: 实现**

```rust
pub fn map_claude_to_deepseek(claude_model: &str) -> &'static str {
    let lower = claude_model.to_ascii_lowercase();
    if lower.contains("opus")   { return "deepseek-v4-pro";   }
    if lower.contains("sonnet") { return "deepseek-v4-flash"; }
    if lower.contains("haiku")  { return "deepseek-v4-flash"; }
    "deepseek-v4-flash"
}

pub fn is_reasoner_target(deepseek_model: &str) -> bool {
    deepseek_model.contains("pro") || deepseek_model.contains("reasoner")
}

#[cfg(test)]
mod tests {
    // ... 保持 Step 1 中的测试不变
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::model_mapping 2>&1 | tail -10
```

Expected: `test result: ok. 8 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/model_mapping.rs
git commit -m "feat(deepseek): implement model_mapping"
```
