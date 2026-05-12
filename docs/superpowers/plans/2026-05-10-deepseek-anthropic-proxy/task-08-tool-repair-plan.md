# T08: tool_repair — 步骤1(snapshot) + 步骤2(plan 构造)

> **并行：** 可与 T02-T06c / T12 / T17 同时执行。前置：T01。

**Goal:** 实现 `RepairOp` 枚举、snapshot 建表（步骤1）和 plan 构造逻辑（步骤2）：case(a) 不动 + case(b) SplitAndPromote + case(c) SynthesizePlaceholder。此任务仅构造 plan 不 apply，并给已就位的 tool_result 打 `_dsk_accepted`。

**Files:**
- Modify: `src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs`

---

- [ ] **Step 1: 定义数据结构和 stub**

```rust
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub(crate) enum RepairOp {
    SplitAndPromote {
        source_user_idx: usize,
        blocks_to_extract: Vec<usize>,
        /// 按 expected_ids 顺序：命中→抽出的 tool_result；缺失→placeholder [no result]
        /// case(b) 部分缺失时内联 placeholder 混排；case(c) 全缺失用 SynthesizePlaceholder
        synthetic_blocks: Vec<Value>,
        insert_after_assistant_idx: usize,
        paired_remaining_blocks: Vec<Value>,
        paired_source_user_idx: Option<usize>,
    },
    SynthesizePlaceholder {
        insert_after_assistant_idx: usize,
        /// 全部为占位 [no result]（仅 case(c) 全缺失）
        synthetic_blocks: Vec<Value>,
        candidate_source_user_idx: Option<usize>,
        paired_remaining_blocks: Vec<Value>,
        paired_source_user_idx: Option<usize>,
    },
    DeleteBlock {
        user_idx: usize,
        block_idx: usize,
    },
    RemoveEmptyUser {
        user_idx: usize,
    },
}

#[derive(Debug, Default)]
pub(crate) struct ToolRepairPlan {
    pub ops: Vec<RepairOp>,
    /// source_user_idx → 从该 user 抽取的 block_idx 集合
    pub extracted_by_user: HashMap<usize, HashSet<usize>>,
    /// user_idx → DeleteBlock 的 block_idx 集合
    pub deleted_by_user: HashMap<usize, HashSet<usize>>,
    /// user_idx → case(a) 标记就位的 tool_result block_idx（未进入 extracted）
    pub accepted_in_place_by_user: HashMap<usize, HashSet<usize>>,
    /// user_idx → original_content 副本（snapshot，供 apply 阶段读取）
    pub original_content: HashMap<usize, Vec<Value>>,
}

/// 步骤1+步骤2：构造 plan（不 mutate messages）
pub(crate) fn build_plan(messages: &mut Vec<Value>) -> ToolRepairPlan {
    todo!()
}

/// 公开 helper：为 test-hooks 暴露 plan 内容以便断言
#[cfg(feature = "test-hooks")]
pub fn inspect_plan(messages: &mut Vec<Value>) -> Vec<String> {
    let plan = build_plan(messages);
    plan.ops.iter().map(|op| format!("{:?}", op)).collect()
}

pub fn repair_tool_order(messages: &mut Vec<Value>) {
    todo!()  // 由 T09-T11 完成
}

#[cfg(test)]
mod tests_build_plan {
    use super::*;
    use serde_json::json;

    fn tool_use(id: &str) -> Value {
        json!({"type": "tool_use", "id": id, "name": "bash", "input": {}})
    }

    fn tool_result(id: &str) -> Value {
        json!({"type": "tool_result", "tool_use_id": id, "content": "ok"})
    }

    #[test]
    fn test_case_a_no_op() {
        // 紧随 user 前 N 块按顺序匹配 → case(a) 不动
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B")]}),
            json!({"role": "user", "content": [tool_result("A"), tool_result("B")]}),
        ];
        let plan = build_plan(&mut messages);
        // plan 中无任何 op
        assert!(plan.ops.is_empty(), "case(a) should produce no ops");
        // A, B 都被标 _dsk_accepted
        assert_eq!(messages[1]["content"][0]["_dsk_accepted"], true);
        assert_eq!(messages[1]["content"][1]["_dsk_accepted"], true);
    }

    #[test]
    fn test_case_a_order_mismatch_becomes_b() {
        // 顺序不匹配 → 进入 case(b)
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B")]}),
            json!({"role": "user", "content": [tool_result("B"), tool_result("A")]}),
        ];
        let plan = build_plan(&mut messages);
        let has_split = plan.ops.iter().any(|op| matches!(op, RepairOp::SplitAndPromote { .. }));
        assert!(has_split, "order mismatch should produce SplitAndPromote");
    }

    #[test]
    fn test_case_b_tool_result_in_text_mixed_user() {
        // tool_result 夹在 text 中 → case(b)
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A")]}),
            json!({"role": "user", "content": [
                {"type": "text", "text": "before"},
                tool_result("A"),
                {"type": "text", "text": "after"}
            ]}),
        ];
        let plan = build_plan(&mut messages);
        let split = plan.ops.iter().find(|op| matches!(op, RepairOp::SplitAndPromote { .. }));
        assert!(split.is_some(), "should have SplitAndPromote");
        if let Some(RepairOp::SplitAndPromote { blocks_to_extract, source_user_idx, .. }) = split {
            assert_eq!(*source_user_idx, 1);
            // block_idx=1 (the tool_result) should be extracted
            assert!(blocks_to_extract.contains(&1));
        }
    }

    #[test]
    fn test_case_c_full_missing_produces_synthesize_placeholder() {
        // 无任何匹配 tool_result → case(c)
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B")]}),
            json!({"role": "user", "content": [{"type": "text", "text": "something else"}]}),
        ];
        let plan = build_plan(&mut messages);
        let synth = plan.ops.iter().find(|op| matches!(op, RepairOp::SynthesizePlaceholder { .. }));
        assert!(synth.is_some(), "all missing → SynthesizePlaceholder");
        if let Some(RepairOp::SynthesizePlaceholder { synthetic_blocks, insert_after_assistant_idx, .. }) = synth {
            assert_eq!(*insert_after_assistant_idx, 0);
            assert_eq!(synthetic_blocks.len(), 2);
            assert_eq!(synthetic_blocks[0]["tool_use_id"], "A");
            assert_eq!(synthetic_blocks[1]["tool_use_id"], "B");
        }
    }

    #[test]
    fn test_case_b_partial_hit_placeholder_inline() {
        // A 存在，B 缺失 → case(b) SplitAndPromote，B placeholder 在 synthetic_blocks 内联
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B"), tool_use("C")]}),
            json!({"role": "user", "content": [tool_result("A"), tool_result("C")]}),
        ];
        let plan = build_plan(&mut messages);
        // 必须是 SplitAndPromote，不是 SynthesizePlaceholder
        assert!(!plan.ops.iter().any(|op| matches!(op, RepairOp::SynthesizePlaceholder { .. })));
        let split = plan.ops.iter().find(|op| matches!(op, RepairOp::SplitAndPromote { .. }));
        assert!(split.is_some());
        if let Some(RepairOp::SplitAndPromote { synthetic_blocks, .. }) = split {
            assert_eq!(synthetic_blocks.len(), 3);
            assert_eq!(synthetic_blocks[0]["tool_use_id"], "A");  // 抽出
            assert_eq!(synthetic_blocks[1]["tool_use_id"], "B");  // placeholder
            assert_eq!(synthetic_blocks[1]["content"], "[no result]");
            assert_eq!(synthetic_blocks[2]["tool_use_id"], "C");  // 抽出
        }
    }

    #[test]
    fn test_single_source_user_selection() {
        // user_x 含 A，user_y 含 B+C → 选含最多匹配的 user_y (idx=3)
        let mut messages = vec![
            json!({"role": "assistant", "content": [tool_use("A"), tool_use("B"), tool_use("C")]}),
            json!({"role": "user", "content": [tool_result("A")]}),              // idx=1, 1 match
            json!({"role": "assistant", "content": [{"type": "text", "text": "ok"}]}),
            json!({"role": "user", "content": [tool_result("B"), tool_result("C")]}), // idx=3, 2 matches
        ];
        let plan = build_plan(&mut messages);
        let split = plan.ops.iter().find(|op| matches!(op, RepairOp::SplitAndPromote { .. }));
        assert!(split.is_some());
        if let Some(RepairOp::SplitAndPromote { source_user_idx, .. }) = split {
            // user_y（idx=3）含 2 个匹配，应被选中
            assert_eq!(*source_user_idx, 3, "should select user with most matches");
        }
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::tool_repair::tests_build_plan 2>&1 | tail -15
```

Expected: FAILED（todo!() panic）

- [ ] **Step 3: 实现 `build_plan`**

```rust
pub(crate) fn build_plan(messages: &mut Vec<Value>) -> ToolRepairPlan {
    let mut plan = ToolRepairPlan::default();

    // 步骤1：snapshot — 记录 original_content + 建立索引表
    for (idx, msg) in messages.iter().enumerate() {
        if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
            plan.original_content.insert(idx, content.clone());
        }
    }

    // 助手 tool_use 索引表：assistant_idx → Vec<(block_idx, tool_use_id)>
    let mut assistant_tool_uses: Vec<(usize, Vec<(usize, String)>)> = Vec::new();
    // user tool_result 索引表：Vec<(user_idx, block_idx, tool_use_id)>
    let mut user_tool_results: Vec<(usize, usize, String)> = Vec::new();

    for (msg_idx, msg) in messages.iter().enumerate() {
        let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if let Some(content) = msg.get("content").and_then(|v| v.as_array()) {
            if role == "assistant" {
                let uses: Vec<(usize, String)> = content
                    .iter()
                    .enumerate()
                    .filter_map(|(bi, b)| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            b.get("id").and_then(|i| i.as_str()).map(|id| (bi, id.to_string()))
                        } else {
                            None
                        }
                    })
                    .collect();
                if !uses.is_empty() {
                    assistant_tool_uses.push((msg_idx, uses));
                }
            } else if role == "user" {
                for (bi, block) in content.iter().enumerate() {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        if let Some(id) = block.get("tool_use_id").and_then(|i| i.as_str()) {
                            user_tool_results.push((msg_idx, bi, id.to_string()));
                        }
                    }
                }
            }
        }
    }

    // 步骤2：按 assistant message 分组判定
    for (assistant_idx, expected_ids_with_blocks) in &assistant_tool_uses {
        let expected_ids: Vec<String> = expected_ids_with_blocks.iter().map(|(_, id)| id.clone()).collect();
        if expected_ids.is_empty() {
            continue;
        }

        // 找紧随 assistant 之后的 user 消息
        let next_user_idx = (assistant_idx + 1..messages.len()).find(|&i| {
            messages[i].get("role").and_then(|r| r.as_str()) == Some("user")
        });

        // case(a) 检查
        let is_case_a = if let Some(nu) = next_user_idx {
            if let Some(content) = messages[nu].get("content").and_then(|v| v.as_array()) {
                let n = expected_ids.len();
                content.len() >= n
                    && content[..n].iter().zip(expected_ids.iter()).all(|(block, eid)| {
                        block.get("type").and_then(|t| t.as_str()) == Some("tool_result")
                            && block.get("tool_use_id").and_then(|i| i.as_str()) == Some(eid.as_str())
                    })
            } else {
                false
            }
        } else {
            false
        };

        if is_case_a {
            // 标记 _dsk_accepted
            let nu = next_user_idx.unwrap();
            let n = expected_ids.len();
            if let Some(content) = messages[nu].get_mut("content").and_then(|v| v.as_array_mut()) {
                for block in content[..n].iter_mut() {
                    if let Some(obj) = block.as_object_mut() {
                        obj.insert("_dsk_accepted".into(), Value::Bool(true));
                    }
                }
            }
            let set = plan.accepted_in_place_by_user.entry(nu).or_default();
            for bi in 0..n {
                set.insert(bi);
            }
            continue;
        }

        // 找所有下游 user 中各 expected_id 的匹配 tool_result
        // key: user_idx → Vec<(block_idx, expected_id_index)>
        let mut user_matches: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for (user_idx, block_idx, tool_use_id) in &user_tool_results {
            if *user_idx <= *assistant_idx {
                continue;  // 只看下游
            }
            if let Some(eid_pos) = expected_ids.iter().position(|e| e == tool_use_id) {
                user_matches.entry(*user_idx).or_default().push((*block_idx, eid_pos));
            }
        }

        if user_matches.is_empty() {
            // case(c)：全部缺失
            let synthetic_blocks: Vec<Value> = expected_ids
                .iter()
                .map(|id| {
                    let mut b = json!({"type": "tool_result", "tool_use_id": id, "content": "[no result]", "is_error": false});
                    b.as_object_mut().unwrap().insert("_dsk_accepted".into(), Value::Bool(true));
                    b
                })
                .collect();
            plan.ops.push(RepairOp::SynthesizePlaceholder {
                insert_after_assistant_idx: *assistant_idx,
                synthetic_blocks,
                candidate_source_user_idx: next_user_idx,
                paired_remaining_blocks: Vec::new(),
                paired_source_user_idx: None,
            });
        } else {
            // case(b)：选单一 source_user（最多匹配；并列取最近 assistant）
            let source_user_idx = user_matches
                .iter()
                .max_by_key(|(&uidx, matches)| (matches.len(), usize::MAX - uidx))
                .map(|(&uidx, _)| uidx)
                .unwrap();

            let source_matches = &user_matches[&source_user_idx];
            let matched_eids: HashSet<usize> = source_matches.iter().map(|(_, ei)| *ei).collect();

            // 构造 synthetic_blocks 按 expected_ids 顺序
            let mut blocks_to_extract: Vec<usize> = Vec::new();
            let synthetic_blocks: Vec<Value> = expected_ids
                .iter()
                .enumerate()
                .map(|(ei, id)| {
                    if let Some(&(block_idx, _)) = source_matches.iter().find(|(_, e)| *e == ei) {
                        // 抽出
                        let block = plan.original_content[&source_user_idx][block_idx].clone();
                        blocks_to_extract.push(block_idx);
                        let mut b = block;
                        b.as_object_mut().unwrap().insert("_dsk_accepted".into(), Value::Bool(true));
                        // 实际从 messages 中标记
                        if let Some(content) = messages[source_user_idx].get_mut("content").and_then(|v| v.as_array_mut()) {
                            if let Some(orig) = content.get_mut(block_idx) {
                                orig.as_object_mut().unwrap().insert("_dsk_accepted".into(), Value::Bool(true));
                            }
                        }
                        b
                    } else {
                        // placeholder
                        json!({"type": "tool_result", "tool_use_id": id, "content": "[no result]", "is_error": false, "_dsk_accepted": true})
                    }
                })
                .collect();

            plan.extracted_by_user
                .entry(source_user_idx)
                .or_default()
                .extend(blocks_to_extract.iter().copied());

            plan.ops.push(RepairOp::SplitAndPromote {
                source_user_idx,
                blocks_to_extract,
                synthetic_blocks,
                insert_after_assistant_idx: *assistant_idx,
                paired_remaining_blocks: Vec::new(),
                paired_source_user_idx: None,
            });
        }
    }

    plan
}
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test deepseek_anthropic::tool_repair::tests_build_plan 2>&1 | tail -10
```

Expected: `test result: ok. 7 passed`

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs
git commit -m "feat(deepseek): implement tool_repair steps 1+2 (snapshot + plan construction)"
```
