# T10: tool_repair — 步骤2.5（paired_remaining 聚合 + 唯一绑定 + RemoveEmptyUser）

> **并行：** ⚠️ 需等 T09 完成。

**Goal:** 实现 `aggregate_paired_remaining`：全局合并 `extracted_by_user ∪ deleted_by_user ∪ accepted_in_place_by_user`，计算每个 source user 的 `final_remaining_blocks`；将 remaining 唯一绑定到「升序最后一个」作用于该 source user 的 op；处理冲突降级（user_will_be_removed 时把 accepted_in_place 并入 deleted）；追加 `RemoveEmptyUser` 给被 DeleteBlock 清空且无 paired 接管的 user。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs`（追加步骤2.5）

---

- [ ] **Step 1: 写失败测试**

```rust
pub(crate) fn aggregate_paired_remaining(messages: &[Value], plan: &mut ToolRepairPlan) {
    todo!()
}

#[cfg(test)]
mod tests_aggregate {
    use super::*;
    use serde_json::json;

    fn make_split_op(source: usize, assistant: usize) -> RepairOp {
        RepairOp::SplitAndPromote {
            source_user_idx: source,
            blocks_to_extract: vec![1],
            synthetic_blocks: vec![json!({"type": "tool_result", "tool_use_id": "A", "_dsk_accepted": true})],
            insert_after_assistant_idx: assistant,
            paired_remaining_blocks: Vec::new(),
            paired_source_user_idx: None,
        }
    }

    #[test]
    fn test_remaining_text_bound_to_last_op() {
        let messages = vec![
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "A", "name": "b", "input": {}}]}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "before"},
                {"type": "tool_result", "tool_use_id": "A", "content": "ok", "_dsk_accepted": true},
                {"type": "text", "text": "after"}
            ]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.original_content.insert(1, messages[1]["content"].as_array().unwrap().clone());
        plan.extracted_by_user.entry(1).or_default().insert(1);
        plan.ops.push(make_split_op(1, 0));

        aggregate_paired_remaining(&messages, &mut plan);

        if let RepairOp::SplitAndPromote { ref paired_remaining_blocks, ref paired_source_user_idx, .. } = plan.ops[0] {
            assert_eq!(*paired_source_user_idx, Some(1));
            assert_eq!(paired_remaining_blocks.len(), 2);  // "before" and "after"
        } else {
            panic!("expected SplitAndPromote");
        }
    }

    #[test]
    fn test_two_ops_same_source_last_gets_remaining() {
        // Two assistants, same source user
        // assistant_idx=0 → op at insert_after=0
        // assistant_idx=2 → op at insert_after=2
        // Remaining text should go to last (insert_after=2 is "last" in ascending order)
        let messages = vec![
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "A", "name": "b", "input": {}}]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "A", "content": "ok", "_dsk_accepted": true},
                {"type": "tool_result", "tool_use_id": "B", "content": "ok", "_dsk_accepted": true},
                {"type": "text", "text": "remaining"}
            ]}),
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "B", "name": "b", "input": {}}]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.original_content.insert(1, messages[1]["content"].as_array().unwrap().clone());
        plan.extracted_by_user.entry(1).or_default().extend([0, 1]);
        // op for A (insert_after=0)
        plan.ops.push(RepairOp::SplitAndPromote {
            source_user_idx: 1,
            blocks_to_extract: vec![0],
            synthetic_blocks: vec![json!({"type": "tool_result", "tool_use_id": "A"})],
            insert_after_assistant_idx: 0,
            paired_remaining_blocks: Vec::new(),
            paired_source_user_idx: None,
        });
        // op for B (insert_after=2)
        plan.ops.push(RepairOp::SplitAndPromote {
            source_user_idx: 1,
            blocks_to_extract: vec![1],
            synthetic_blocks: vec![json!({"type": "tool_result", "tool_use_id": "B"})],
            insert_after_assistant_idx: 2,
            paired_remaining_blocks: Vec::new(),
            paired_source_user_idx: None,
        });

        aggregate_paired_remaining(&messages, &mut plan);

        // op insert_after=2 (last in ascending) should carry remaining + source remove
        if let RepairOp::SplitAndPromote { insert_after_assistant_idx: 2, ref paired_remaining_blocks, ref paired_source_user_idx, .. } = plan.ops[1] {
            assert_eq!(*paired_source_user_idx, Some(1));
            assert_eq!(paired_remaining_blocks.len(), 1);
            assert_eq!(paired_remaining_blocks[0]["text"], "remaining");
        } else {
            panic!("expected last op to carry remaining");
        }

        // First op should have empty paired
        if let RepairOp::SplitAndPromote { insert_after_assistant_idx: 0, ref paired_source_user_idx, .. } = plan.ops[0] {
            assert_eq!(*paired_source_user_idx, None);
        } else {
            panic!("first op should not carry remaining");
        }
    }

    #[test]
    fn test_remove_empty_user_added_when_all_blocks_deleted() {
        let messages = vec![
            json!({"role": "assistant", "content": [{"type": "tool_use", "id": "A", "name": "b", "input": {}}]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "ORPHAN", "content": "ok"}
            ]}),
        ];
        let mut plan = ToolRepairPlan::default();
        plan.original_content.insert(1, messages[1]["content"].as_array().unwrap().clone());
        plan.deleted_by_user.entry(1).or_default().insert(0);
        plan.ops.push(RepairOp::DeleteBlock { user_idx: 1, block_idx: 0 });

        aggregate_paired_remaining(&messages, &mut plan);

        let has_remove = plan.ops.iter().any(|op| matches!(op, RepairOp::RemoveEmptyUser { user_idx: 1 }));
        assert!(has_remove, "fully-cleared user should get RemoveEmptyUser");
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::tool_repair::tests_aggregate 2>&1 | tail -15
```

- [ ] **Step 3: 实现**

```rust
pub(crate) fn aggregate_paired_remaining(messages: &[Value], plan: &mut ToolRepairPlan) {
    // 确定 user_will_be_removed[u]
    let mut user_will_be_removed: HashSet<usize> = HashSet::new();
    for op in &plan.ops {
        match op {
            RepairOp::SplitAndPromote { source_user_idx, .. } => {
                if !plan.extracted_by_user.get(source_user_idx).map(|s| s.is_empty()).unwrap_or(true) {
                    user_will_be_removed.insert(*source_user_idx);
                }
            }
            RepairOp::SynthesizePlaceholder { candidate_source_user_idx: Some(u), .. } => {
                user_will_be_removed.insert(*u);
            }
            _ => {}
        }
    }

    // 冲突降级：accepted_in_place 并入 deleted 若 user_will_be_removed
    let conflict_users: Vec<usize> = plan
        .accepted_in_place_by_user
        .keys()
        .filter(|u| user_will_be_removed.contains(u))
        .copied()
        .collect();
    for u in conflict_users {
        if let Some(accepted) = plan.accepted_in_place_by_user.remove(&u) {
            let deleted = plan.deleted_by_user.entry(u).or_default();
            for bi in &accepted {
                deleted.insert(*bi);
                plan.ops.push(RepairOp::DeleteBlock { user_idx: u, block_idx: *bi });
            }
        }
    }

    // 计算每个 source user 的 final_remaining_blocks
    let mut final_remaining: HashMap<usize, Vec<Value>> = HashMap::new();
    for (&user_idx, original) in &plan.original_content {
        let extracted = plan.extracted_by_user.get(&user_idx);
        let deleted = plan.deleted_by_user.get(&user_idx);
        let accepted = plan.accepted_in_place_by_user.get(&user_idx);

        let remaining: Vec<Value> = original
            .iter()
            .enumerate()
            .filter_map(|(bi, block)| {
                let in_extracted = extracted.map(|s| s.contains(&bi)).unwrap_or(false);
                let in_deleted = deleted.map(|s| s.contains(&bi)).unwrap_or(false);
                let in_accepted = accepted.map(|s| s.contains(&bi)).unwrap_or(false);
                if in_extracted || in_deleted || in_accepted {
                    None
                } else {
                    Some(block.clone())
                }
            })
            .collect();

        if user_will_be_removed.contains(&user_idx) && !remaining.is_empty() {
            final_remaining.insert(user_idx, remaining);
        }
    }

    // 唯一绑定到「升序最后一个」op
    // 收集每个 source user → 其所有 op 的 (index_in_ops, insert_after_assistant_idx)
    let mut source_to_op_indices: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
    for (op_idx, op) in plan.ops.iter().enumerate() {
        match op {
            RepairOp::SplitAndPromote { source_user_idx, insert_after_assistant_idx, .. } => {
                source_to_op_indices
                    .entry(*source_user_idx)
                    .or_default()
                    .push((op_idx, *insert_after_assistant_idx));
            }
            RepairOp::SynthesizePlaceholder { candidate_source_user_idx: Some(u), insert_after_assistant_idx, .. } => {
                source_to_op_indices
                    .entry(*u)
                    .or_default()
                    .push((op_idx, *insert_after_assistant_idx));
            }
            _ => {}
        }
    }

    for (source_user, mut op_indices) in source_to_op_indices {
        // 升序最后一个 = 最大 insert_after_assistant_idx
        op_indices.sort_by_key(|&(_, ia)| ia);
        let (last_op_idx, _) = *op_indices.last().unwrap();
        let remaining = final_remaining.remove(&source_user).unwrap_or_default();
        match &mut plan.ops[last_op_idx] {
            RepairOp::SplitAndPromote { ref mut paired_remaining_blocks, ref mut paired_source_user_idx, .. } => {
                *paired_remaining_blocks = remaining;
                *paired_source_user_idx = Some(source_user);
            }
            RepairOp::SynthesizePlaceholder { ref mut paired_remaining_blocks, ref mut paired_source_user_idx, .. } => {
                *paired_remaining_blocks = remaining;
                *paired_source_user_idx = Some(source_user);
            }
            _ => {}
        }
    }

    // RemoveEmptyUser：被 DeleteBlock 清空且不被 paired 接管的 user
    for (&user_idx, deleted_set) in &plan.deleted_by_user {
        if user_will_be_removed.contains(&user_idx) {
            continue;  // 已被 paired remove 接管
        }
        let original_len = plan.original_content.get(&user_idx).map(|v| v.len()).unwrap_or(0);
        let total_removed = deleted_set.len()
            + plan.accepted_in_place_by_user.get(&user_idx).map(|s| s.len()).unwrap_or(0);
        if total_removed >= original_len {
            plan.ops.push(RepairOp::RemoveEmptyUser { user_idx });
        }
    }
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::tool_repair::tests_aggregate 2>&1 | tail -10
```

Expected: `test result: ok. 3 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs
git commit -m "feat(deepseek): implement tool_repair step 2.5 - paired aggregation and RemoveEmptyUser"
```
