# T09: tool_repair — 步骤3（残留清理 + DeleteBlock）

> **并行：** ⚠️ 需等 T08 完成（依赖 `build_plan` 和 `ToolRepairPlan`）。

**Goal:** 实现步骤3 `add_delete_ops`：扫描 messages 中未带 `_dsk_accepted` 标志的 tool_result 块，孤立或已消费的均加入 `RepairOp::DeleteBlock`，填充 `deleted_by_user`；被清空的 user 消息若满足 `user_will_be_removed[u]==false` 则追加 `RepairOp::RemoveEmptyUser`（此计算在 T10 完成，本任务仅写 DeleteBlock 部分并暴露供 T10 使用）。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs`（追加步骤3）

---

- [ ] **Step 1: 写失败测试**

```rust
/// 步骤3：扫描未 accepted 的 tool_result，加入 DeleteBlock plan
pub(crate) fn add_delete_ops(messages: &[Value], plan: &mut ToolRepairPlan) {
    todo!()
}

#[cfg(test)]
mod tests_add_delete_ops {
    use super::*;
    use serde_json::json;

    fn tool_result(id: &str) -> Value {
        json!({"type": "tool_result", "tool_use_id": id, "content": "ok"})
    }

    fn tool_result_accepted(id: &str) -> Value {
        json!({"type": "tool_result", "tool_use_id": id, "content": "ok", "_dsk_accepted": true})
    }

    fn tool_use(id: &str) -> Value {
        json!({"type": "tool_use", "id": id, "name": "bash", "input": {}})
    }

    #[test]
    fn test_orphan_tool_result_deleted() {
        // tool_result with id that has no corresponding assistant tool_use
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result("A"), tool_result("ORPHAN")]}),
        ];
        let mut plan = ToolRepairPlan::default();
        // Simulate A being accepted (already marked)
        plan.accepted_in_place_by_user.entry(1).or_default().insert(0);

        add_delete_ops(&messages, &mut plan);

        let deletes: Vec<_> = plan.ops.iter().filter(|op| matches!(op, RepairOp::DeleteBlock { user_idx: 1, block_idx: 1 })).collect();
        assert_eq!(deletes.len(), 1, "orphan ORPHAN should be deleted");
    }

    #[test]
    fn test_duplicate_tool_result_deleted() {
        // Same tool_use_id appears twice; A accepted in place at block 0, block 1 should be deleted
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result_accepted("A"), tool_result("A")]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.accepted_in_place_by_user.entry(1).or_default().insert(0);

        add_delete_ops(&messages, &mut plan);

        let deletes: Vec<_> = plan.ops.iter().filter(|op| matches!(op, RepairOp::DeleteBlock { user_idx: 1, block_idx: 1 })).collect();
        assert_eq!(deletes.len(), 1, "duplicate A at block 1 should be deleted");
    }

    #[test]
    fn test_late_legitimate_tool_result_deleted() {
        // A's tool_result exists but in wrong position (not adjacent to assistant)
        // After build_plan it's NOT accepted (build_plan chose a different source)
        // Step 3 must delete it
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [{"type": "text", "text": "irrelevant"}]}),
            json!({"role": "user", "content": [tool_result("A")]}),  // idx=2, far away
        ];
        let mut plan = ToolRepairPlan::default();
        // A was synthesized by SplitAndPromote from some other source, so idx=2 block is not accepted

        add_delete_ops(&messages, &mut plan);

        // A has a valid expected_id but it's not accepted → delete it
        let has_delete = plan.ops.iter().any(|op| matches!(op, RepairOp::DeleteBlock { user_idx: 2, block_idx: 0 }));
        assert!(has_delete, "unapproved legitimate id should be deleted");
    }

    #[test]
    fn test_accepted_block_not_deleted() {
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result_accepted("A")]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.accepted_in_place_by_user.entry(1).or_default().insert(0);

        add_delete_ops(&messages, &mut plan);

        assert!(plan.ops.is_empty(), "accepted block should not be deleted");
    }

    #[test]
    fn test_deleted_by_user_populated() {
        let messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result("ORPHAN")]}),
        ];
        let mut plan = ToolRepairPlan::default();
        add_delete_ops(&messages, &mut plan);

        assert!(plan.deleted_by_user.contains_key(&1));
        assert!(plan.deleted_by_user[&1].contains(&0));
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::tool_repair::tests_add_delete_ops 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
pub(crate) fn add_delete_ops(messages: &[Value], plan: &mut ToolRepairPlan) {
    // 先收集所有 expected_ids（assistant tool_use 的 id 集合）
    let mut all_expected_ids: HashSet<String> = HashSet::new();
    for msg in messages {
        if msg.get("role").and_then(|r| r.as_str()) == Some("assistant") {
            if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        if let Some(id) = block.get("id").and_then(|i| i.as_str()) {
                            all_expected_ids.insert(id.to_string());
                        }
                    }
                }
            }
        }
    }

    // 扫描所有未 _dsk_accepted 的 tool_result
    for (user_idx, msg) in messages.iter().enumerate() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("user") {
            continue;
        }
        let Some(content) = msg.get("content").and_then(|v| v.as_array()) else {
            continue;
        };
        let accepted_in_place = plan.accepted_in_place_by_user.get(&user_idx);
        let extracted = plan.extracted_by_user.get(&user_idx);

        for (block_idx, block) in content.iter().enumerate() {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            // 已被 accepted（case(a) 或 extracted）→ 跳过
            let is_accepted_in_place = accepted_in_place.map(|s| s.contains(&block_idx)).unwrap_or(false);
            let is_extracted = extracted.map(|s| s.contains(&block_idx)).unwrap_or(false);
            let is_marker = block.get("_dsk_accepted").and_then(|v| v.as_bool()).unwrap_or(false);
            if is_accepted_in_place || is_extracted || is_marker {
                continue;
            }
            // 未 accepted → 加入 DeleteBlock
            plan.ops.push(RepairOp::DeleteBlock { user_idx, block_idx });
            plan.deleted_by_user.entry(user_idx).or_default().insert(block_idx);
        }
    }
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::tool_repair::tests_add_delete_ops 2>&1 | tail -10
```

Expected: `test result: ok. 5 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs
git commit -m "feat(deepseek): implement tool_repair step 3 - orphan/duplicate cleanup"
```
