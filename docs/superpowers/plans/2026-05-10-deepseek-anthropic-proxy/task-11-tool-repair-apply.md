# T11: tool_repair — 步骤4（三阶段 apply + snapshot_to_current + repair_tool_order 公开 API）

> **并行：** ⚠️ 需等 T10 完成。

**Goal:** 实现 `apply_plan`（三阶段 A→B→C）和 `snapshot_to_current` 维护辅助函数，最终完成公开 API `repair_tool_order`，将步骤1-3-2.5-4 串联调用。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs`（追加 apply + repair_tool_order）

---

- [ ] **Step 1: 写失败测试**

```rust
/// 步骤4：三阶段 apply
pub(crate) fn apply_plan(messages: &mut Vec<Value>, plan: ToolRepairPlan) {
    todo!()
}

#[cfg(test)]
mod tests_apply_plan {
    use super::*;
    use serde_json::json;

    fn tool_use(id: &str) -> Value {
        json!({"type": "tool_use", "id": id, "name": "bash", "input": {}})
    }

    fn tool_result(id: &str) -> Value {
        json!({"type": "tool_result", "tool_use_id": id, "content": "ok"})
    }

    // --- 完整端到端通过 repair_tool_order ---

    #[test]
    fn test_case_a_no_change() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result("A")]}),
        ];
        repair_tool_order(&mut messages);
        assert_eq!(messages.len(), 2);
        assert!(messages[1]["content"][0].get("_dsk_accepted").is_none(), "cleanup should have removed _dsk_accepted");
    }

    #[test]
    fn test_case_b_reorder_fixed() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B")]}),
            json!({"role": "user", "content": [tool_result("B"), tool_result("A")]}),
        ];
        repair_tool_order(&mut messages);
        // After repair: messages[0] = assistant, messages[1] = synthetic user(A, B)
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1]["content"][0]["tool_use_id"], "A");
        assert_eq!(messages[1]["content"][1]["tool_use_id"], "B");
    }

    #[test]
    fn test_case_b_text_mixed_split() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "before"},
                tool_result("A"),
                {"type": "text", "text": "after"}
            ]}),
        ];
        repair_tool_order(&mut messages);
        // synthetic user has just [tool_result A, "before", "after"]
        // original user is removed (paired)
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "assistant");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"][0]["tool_use_id"], "A");
        // remaining text blocks present in paired_remaining
        let texts: Vec<_> = messages[1]["content"]
            .as_array().unwrap()
            .iter()
            .filter(|b| b["type"] == "text")
            .collect();
        assert_eq!(texts.len(), 2);
    }

    #[test]
    fn test_case_c_placeholder_inserted() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B")]}),
            json!({"role": "user", "content": [{"type": "text", "text": "followup"}]}),
        ];
        repair_tool_order(&mut messages);
        // placeholder inserted after assistant, original user merged
        assert_eq!(messages.len(), 2);  // assistant + merged synthetic user
        assert_eq!(messages[1]["content"][0]["tool_use_id"], "A");
        assert_eq!(messages[1]["content"][0]["content"], "[no result]");
        assert_eq!(messages[1]["content"][1]["tool_use_id"], "B");
        assert_eq!(messages[1]["content"][2]["type"], "text");
        assert_eq!(messages[1]["content"][2]["text"], "followup");
    }

    #[test]
    fn test_orphan_block_deleted() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [
                tool_result("A"),
                tool_result("ORPHAN")
            ]}),
        ];
        repair_tool_order(&mut messages);
        let content = messages[1]["content"].as_array().unwrap();
        assert!(content.iter().all(|b| b.get("tool_use_id").map(|id| id != "ORPHAN").unwrap_or(true)));
    }

    #[test]
    fn test_dsk_accepted_cleaned_up() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result("A")]}),
        ];
        repair_tool_order(&mut messages);
        for msg in &messages {
            if let Some(content) = msg["content"].as_array() {
                for block in content {
                    assert!(block.get("_dsk_accepted").is_none(), "_dsk_accepted should be cleaned up");
                }
            }
        }
    }

    #[test]
    fn test_three_phase_ordering_phase_a_before_b() {
        // DeleteBlock + SplitAndPromote in same plan
        // Phase A must complete before Phase B changes message count
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [
                tool_result("B"),   // orphan, will be deleted
                tool_result("A"),   // correct, will be extracted
            ]}),
        ];
        repair_tool_order(&mut messages);
        // After: assistant + synthetic user[A], orphan deleted
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1]["content"][0]["tool_use_id"], "A");
        let no_b = messages[1]["content"]
            .as_array().unwrap()
            .iter()
            .all(|b| b.get("tool_use_id").map(|id| id != "B").unwrap_or(true));
        assert!(no_b, "orphan B should be deleted");
    }

    #[test]
    fn test_multiple_assistants_independent() {
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [tool_result("B"), tool_result("A")]}),
            json!({"role": "assistant", "content": [tool_use("B")]}),
            json!({"role": "user", "content": [{"type": "text", "text": "next"}]}),
        ];
        repair_tool_order(&mut messages);
        // First assistant: synthetic user[A], B is orphan (no expected_id for it from first assistant after B is owned by second)
        // Second assistant: B found somewhere... actually B is at user idx=1 but B was supposed to be for assistant idx=2
        // This is complex - just check no crash and result is valid structure
        // At minimum: no consecutive users, no _dsk_accepted leak
        for msg in &messages {
            if let Some(content) = msg["content"].as_array() {
                for block in content {
                    assert!(block.get("_dsk_accepted").is_none());
                }
            }
        }
        // No consecutive users
        let roles: Vec<_> = messages.iter()
            .map(|m| m["role"].as_str().unwrap_or(""))
            .collect();
        for w in roles.windows(2) {
            assert!(!(w[0] == "user" && w[1] == "user"), "consecutive users found");
        }
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::tool_repair::tests_apply_plan 2>&1 | tail -15
```

- [ ] **Step 3: 实现 `apply_plan` + 辅助函数 + `repair_tool_order`**

```rust
fn after_insert(map: &mut HashMap<usize, usize>, pos: usize) {
    for v in map.values_mut() {
        if *v >= pos {
            *v += 1;
        }
    }
}

fn after_remove(map: &mut HashMap<usize, usize>, pos: usize) {
    map.retain(|_, v| *v != pos);
    for v in map.values_mut() {
        if *v > pos {
            *v -= 1;
        }
    }
}

pub(crate) fn apply_plan(messages: &mut Vec<Value>, mut plan: ToolRepairPlan) {
    // 初始化 snapshot_to_current: identity
    let mut snap_to_cur: HashMap<usize, usize> = (0..messages.len()).map(|i| (i, i)).collect();

    // 阶段 A：DeleteBlock — 按 user_idx 分组，组内 block_idx 降序
    let mut delete_ops: Vec<(usize, usize)> = plan.ops.iter().filter_map(|op| {
        if let RepairOp::DeleteBlock { user_idx, block_idx } = op {
            Some((*user_idx, *block_idx))
        } else {
            None
        }
    }).collect();
    // 同 user 内降序 block_idx，跨 user 任意顺序
    delete_ops.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

    for (user_idx, block_idx) in delete_ops {
        let actual_user = match snap_to_cur.get(&user_idx) {
            Some(&v) => v,
            None => continue,
        };
        if let Some(content) = messages[actual_user].get_mut("content").and_then(|v| v.as_array_mut()) {
            if block_idx < content.len() {
                content.remove(block_idx);
            }
        }
        // 阶段 A 不改变 messages.len()，snap_to_cur 不变
    }

    // 阶段 B：SplitAndPromote / SynthesizePlaceholder — 按 insert_after_assistant_idx 降序
    let mut b_ops: Vec<usize> = plan.ops.iter().enumerate().filter_map(|(i, op)| {
        match op {
            RepairOp::SplitAndPromote { .. } | RepairOp::SynthesizePlaceholder { .. } => Some(i),
            _ => None,
        }
    }).collect();
    b_ops.sort_by_key(|&i| {
        match &plan.ops[i] {
            RepairOp::SplitAndPromote { insert_after_assistant_idx: ia, .. } => usize::MAX - ia,
            RepairOp::SynthesizePlaceholder { insert_after_assistant_idx: ia, .. } => usize::MAX - ia,
            _ => 0,
        }
    });

    for op_idx in b_ops {
        let (insert_after_snap, synthetic_blocks, paired_remaining, paired_source_snap) =
            match &plan.ops[op_idx] {
                RepairOp::SplitAndPromote {
                    insert_after_assistant_idx,
                    synthetic_blocks,
                    paired_remaining_blocks,
                    paired_source_user_idx,
                    ..
                } => (
                    *insert_after_assistant_idx,
                    synthetic_blocks.clone(),
                    paired_remaining_blocks.clone(),
                    *paired_source_user_idx,
                ),
                RepairOp::SynthesizePlaceholder {
                    insert_after_assistant_idx,
                    synthetic_blocks,
                    paired_remaining_blocks,
                    paired_source_user_idx,
                    ..
                } => (
                    *insert_after_assistant_idx,
                    synthetic_blocks.clone(),
                    paired_remaining_blocks.clone(),
                    *paired_source_user_idx,
                ),
                _ => continue,
            };

        // 构造 synthetic user
        let mut content = synthetic_blocks;
        content.extend(paired_remaining);
        // strip _dsk_accepted from synthetic content
        for block in content.iter_mut() {
            if let Some(obj) = block.as_object_mut() {
                obj.remove("_dsk_accepted");
            }
        }
        let synthetic_user = json!({"role": "user", "content": content});

        // 步骤4a：insert
        let actual_a = snap_to_cur[&insert_after_snap];
        let insert_pos = actual_a + 1;
        messages.insert(insert_pos, synthetic_user);
        after_insert(&mut snap_to_cur, insert_pos);

        // 步骤4b：remove paired（条件）
        if let Some(p_snap) = paired_source_snap {
            if let Some(&actual_p) = snap_to_cur.get(&p_snap) {
                messages.remove(actual_p);
                after_remove(&mut snap_to_cur, actual_p);
            }
        }
    }

    // 阶段 C：RemoveEmptyUser — 按 user_idx 降序
    let mut c_ops: Vec<usize> = plan.ops.iter().enumerate().filter_map(|(i, op)| {
        if matches!(op, RepairOp::RemoveEmptyUser { .. }) { Some(i) } else { None }
    }).collect();
    c_ops.sort_by_key(|&i| {
        if let RepairOp::RemoveEmptyUser { user_idx } = &plan.ops[i] {
            usize::MAX - user_idx
        } else {
            0
        }
    });

    for op_idx in c_ops {
        if let RepairOp::RemoveEmptyUser { user_idx } = &plan.ops[op_idx] {
            if let Some(&actual_user) = snap_to_cur.get(user_idx) {
                // 防御性断言：content 应为空
                debug_assert!(
                    messages[actual_user]["content"].as_array().map(|a| a.is_empty()).unwrap_or(true),
                    "RemoveEmptyUser: content should be empty at this point"
                );
                messages.remove(actual_user);
                after_remove(&mut snap_to_cur, actual_user);
            }
        }
    }

    // 清理所有剩余 _dsk_accepted 字段
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

pub fn repair_tool_order(messages: &mut Vec<Value>) {
    // 步骤1 + 步骤2：build_plan（snapshot + case 判定）
    let mut plan = build_plan(messages);
    // 步骤3：残留清理
    let messages_snapshot: Vec<Value> = messages.clone();
    add_delete_ops(&messages_snapshot, &mut plan);
    // 步骤2.5：paired 聚合 + 唯一绑定 + RemoveEmptyUser
    aggregate_paired_remaining(&messages_snapshot, &mut plan);
    // 步骤4：三阶段 apply
    apply_plan(messages, plan);
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::tool_repair 2>&1 | tail -10
```

Expected: `test result: ok. N passed`（所有 tool_repair 测试）

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs
git commit -m "feat(deepseek): implement tool_repair steps 4 + apply_plan (three-phase A/B/C) + repair_tool_order"
```
