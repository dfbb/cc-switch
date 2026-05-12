# DeepSeek Anthropic 兼容代理层设计

**日期：** 2026-05-10（v4.10，十一轮评审整改：8 + 7 + 6 + 8 + 7 + 5 + 6 + 5 + 6 + 5 + 5 = 68 条）
**目标使用场景：** **DeepSeek Pro（Reasoner 推理）优先**，同时支持 Flash（非推理）
**集成方式：** 扩展现有 Claude 提供商，新增 `deepseek_anthropic` api_format

---

## 设计目标与约束

**目标：** 让 Claude Code v2.1.126+ 在不报错的前提下使用 DeepSeek Pro/Flash 后端，同时尽量保留：
- Pro 推理输出在 Claude Code 中渲染为 thinking UI
- MCP function tools 可用（DeepSeek Anthropic 兼容端点支持 `tool_choice` 的 none/auto/any/tool 全部 4 个 type；代理对该字段做白名单保留 + 子字段清理，详见 ⑨）
- 非流式请求（如 `/cost`）正常透传

**核心约束：**
- Claude Code v2.1.126+ 本地白名单：模型名必须是 `claude-*` 系列（已实测 `claude-sonnet-4-6`，`claude-opus-4-7` 待实测）
- DeepSeek Reasoner 要求历史中 tool_use 之前必须有可回放的 thinking 块；否则会拒绝
- DeepSeek Anthropic 端点 thinking **默认 enabled**，Flash 模式必须显式 disable

---

## 架构决策

**新增 `api_format = "deepseek_anthropic"`，归类为 passthrough（不进入 transform 列表）。**

数据流（修订）：

```
Claude Code (claude-sonnet-4-6 / claude-opus-4-7, x-api-key)
    │
    ▼
cc-switch forwarder
    │ resolve_claude_api_format → "deepseek_anthropic"
    │ claude_api_format_needs_transform → false (passthrough)
    │ sanitize_request(mapped_body):
    │   ① 模型名映射 + 保存 fake_model
    │   ② tools 黑名单（删 server tools，保留 client tools）
    │   ③ thinking 字段白名单重建（unsafe_tool_followup 检测）
    │   ④ output_config 智能处理
    │   ⑤ messages 净化（attachment / thinking 历史过滤 / tool_result / reasoning_content）
    │   ⑥ context_management.edits 过滤
    │   ⑦ split-and-promote tool 顺序修复（按 assistant 分组、按 expected_ids 顺序匹配）
    │   ⑧ max_tokens 兜底
    │   ⑨ tool_choice 白名单保留（type ∈ {none,auto,any,tool}；清理 disable_parallel_tool_use；type=tool 缺 name 降级为 auto）
    │   ⑩ stream 字段保留客户端原值
    │ ordered_headers 构造时跳过 anthropic-beta 等
    ▼
DeepSeek API /anthropic/v1/messages
    │ SSE 或非流式响应（model: deepseek-v4-pro/flash）
    ▼
cc-switch response_processor
    │ 流式：wrap_sse_stream + 状态机 transform_native_sse_block_event
    │   - 改写 message_start 的嵌套 message.model
    │   - 按 effective_thinking_enabled 删除 thinking 整组事件 / 仅删 redacted_thinking
    │   - 索引重映射保证下游 index 连续
    │ 非流式：JSON 顶层 model 字段重写
    ▼
Claude Code（模型名校验通过；推理 UI 正常渲染）
```

---

## 改动详情

### 1. 后端 Rust

#### 新文件：`src-tauri/src/proxy/providers/deepseek_anthropic/mod.rs`

模块结构（按职责拆分）：
```
deepseek_anthropic/
├── mod.rs                  // 公开 API + 重导出
├── model_mapping.rs        // Claude 名 ↔ DeepSeek 名
├── request_sanitizer.rs    // sanitize_request + 子函数
├── tool_repair.rs          // split-and-promote tool 顺序修复
├── sse_state.rs            // SSE 状态机（吸收自 free-claude-code）
├── sse_stream.rs           // wrap_sse_stream + patch_sse_event
└── response_patch.rs       // 非流式响应 patch（评审 #6 修正：补回）
```

公开 API：
```rust
pub use model_mapping::map_claude_to_deepseek;
pub use request_sanitizer::{sanitize_request, SanitizeResult};
pub use sse_stream::{patch_sse_event, wrap_sse_stream};
pub use response_patch::patch_non_streaming_response;
```

> **针对评审 #6：** 单事件函数 `patch_sse_event(event, &mut state, fake_model, effective_thinking_enabled) -> Vec<String>` 与流包装 `wrap_sse_stream<S>(stream, fake_model, effective_thinking_enabled) -> impl Stream<Item = Result<Bytes, std::io::Error>>` 完全分离；`wrap_sse_stream` 内部按 `\n\n` 切分缓冲、调 `patch_sse_event`。模块重导出包含两者。

---

#### `model_mapping.rs`

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
```

---

#### `request_sanitizer.rs`（核心，吸收 free-claude-code 3 项设计）

```rust
pub struct SanitizeResult {
    pub fake_model: String,            // 客户端原始 model（供响应 patch）
    pub target_model: String,          // 实际发给 DeepSeek 的 model
    pub effective_thinking_enabled: bool,  // SSE 状态机需要
}

pub fn sanitize_request(body: &mut Value) -> SanitizeResult { ... }
```

**处理顺序：**

**① 模型名映射**
- 保存 `body["model"]` 字符串值为 `fake_model`
- `target_model = map_claude_to_deepseek(&fake_model)`
- 写回 `body["model"] = target_model`

**② tools 黑名单过滤**（评审 #1 修正）

> **关键修正：** Anthropic 普通 client tools 没有 `type` 字段（结构是 `{name, description, input_schema}`），server tools 才有 `type: "web_search_*"` 等。**改用黑名单：**

- 仅当 `tool["type"]` 存在且为以下值之一时删除：
  - `"web_search"`, `"web_search_*"`（任何带版本号的）
  - `"web_fetch"`, `"web_fetch_*"`
  - `"computer_*"`, `"text_editor_*"`（其他 Anthropic 内置 server tools）
- 否则保留（包括 `{name, description, input_schema}` 普通工具，含 MCP 转发的 function 工具）
- 同时检查 `tool["name"]` 是否为 `"web_search"` / `"web_fetch"` 双重保险（free-claude-code 同款）

**③ thinking 字段白名单重建**（吸收 free-claude-code 设计；Pro 默认启用）

```rust
let original_thinking = body.as_object_mut().unwrap().remove("thinking");

// 客户端意图三态：Some(true)=显式 enabled, Some(false)=显式 disabled, None=未指定/未知值
// v4.3 修正：只识别 "enabled" / "disabled"，未知值（含将来新增类型）不静默降级，回退到 target_model 默认
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

// 默认值由 target_model 决定（评审 #2 修正：Pro 默认启用）
let target_default = is_reasoner_target(&target_model);  // pro=true, flash=false

// client 显式优先，否则按 target_model 默认
let intended_thinking_enabled = client_intent.unwrap_or(target_default);

// unsafe_tool_followup 检测（free-claude-code 核心设计）
let has_tool_history = detect_tool_history(body);
let has_replayable_thinking = detect_replayable_thinking_before_tool_use(body);
let unsafe_tool_followup = has_tool_history && !has_replayable_thinking;

let effective_thinking_enabled = intended_thinking_enabled && !unsafe_tool_followup;
```

| target | client 意图 | unsafe_tool_followup | effective_thinking_enabled | body["thinking"] 写回 |
|--------|-----------|---------------------|---------------------------|--------------------|
| pro | None（未指定） | false | **true**（默认启用） | `{"type":"enabled", "budget_tokens":N?}` |
| pro | Some(true) | false | true | `{"type":"enabled", "budget_tokens":N?}` |
| pro | Some(false) | * | false | `{"type":"disabled"}` |
| pro | * | true | false | `{"type":"disabled"}`（unsafe_tool_followup 降级） |
| flash | None | * | **false**（默认禁用） | `{"type":"disabled"}` |
| flash | Some(true) | false | true | `{"type":"enabled"}`（用户主动要求） |
| flash | Some(false) | * | false | `{"type":"disabled"}` |

> **针对评审 #2：** Pro 默认 thinking enabled（与 DeepSeek Anthropic 端点默认对齐），仅在客户端显式 disable 或 unsafe_tool_followup 时禁用。
>
> **针对评审 #8：** Flash 模式必须显式 `{"type":"disabled"}`，不能省略 thinking 字段（端点默认 enabled）。

**④ output_config 白名单（评审 #6 修正）**

> **修正：** DeepSeek Anthropic 文档仅支持 `output_config.effort`。整对象透传若 Claude Code 未来加未知子字段会触发 400/422。改用白名单。

```rust
const OUTPUT_CONFIG_ALLOWED: &[&str] = &["effort"];
```

处理逻辑：
- 仅保留 `output_config[k]` where k ∈ `OUTPUT_CONFIG_ALLOWED`
- 若 `unsafe_tool_followup == true`：再从 output_config 删除 `effort`（避免下游误进推理）
- 若清理后 output_config 为空对象，整体删除

`mcp_servers` 顶层字段一律静默删除。

**⑤ messages 净化（多步流水线，按 free-claude-code 顺序）**

```
strip_unsupported_attachments  ← image/document → 占位（仅当全空时）
   ↓
sanitize_thinking_blocks       ← 按 effective_thinking_enabled 过滤 thinking/redacted_thinking
   ↓
normalize_tool_result_content  ← content 数组序列化为字符串
   ↓
strip_reasoning_content        ← 删除 assistant.reasoning_content 字段
```

**`strip_unsupported_attachments` 细则**（吸收 free-claude-code；v4.12 评审 #1 全量覆盖 DeepSeek "Not Supported" 列表）：

依据 DeepSeek 官方 Anthropic 兼容文档 Message Fields 表，content 数组中以下 type **全部 Not Supported**，必须在 sanitize 阶段统一剔除：

| type | 顶层处理 | tool_result.content 内嵌处理 |
|------|---------|------------------------------|
| `image` | 移除 | 移除 |
| `document` | 移除 | 移除 |
| `search_result` | 移除 | 移除 |
| `server_tool_use` | 移除 | 移除 |
| `web_search_tool_result` | 移除 | 移除 |
| `code_execution_tool_result` | 移除 | 移除 |
| `mcp_tool_use` | 移除 | 移除 |
| `mcp_tool_result` | 移除 | 移除 |
| `container_upload` | 移除 | 移除 |

实现：

```rust
const UNSUPPORTED_BLOCK_TYPES: &[&str] = &[
    "image",
    "document",
    "search_result",
    "server_tool_use",
    "web_search_tool_result",
    "code_execution_tool_result",
    "mcp_tool_use",
    "mcp_tool_result",
    "container_upload",
];

fn is_unsupported_block(v: &Value) -> bool {
    v.get("type")
        .and_then(|t| t.as_str())
        .is_some_and(|t| UNSUPPORTED_BLOCK_TYPES.contains(&t))
}
```

- 顶层不支持块 → 移除，**不**放占位（除非整条消息 content 因移除而为空，才整体放 `[attachment omitted]` 占位 text，避免触发空 content 兜底再级联其它分支）
- `tool_result.content` 数组内嵌不支持块 → 移除；若内嵌列表全空，放 `[attachment omitted]` 占位
- 占位文字：`"[attachment omitted: DeepSeek does not support image, document, search_result, server_tool_use, web_search_tool_result, code_execution_tool_result, mcp_tool_use, mcp_tool_result, container_upload]"`（一条统一占位即可，无需为每种 type 单独占位）

> **不混淆 `mcp_tool_*` 与请求顶层 `mcp_servers`**：后者是 Simple Fields 中的 `mcp_servers` 数组（DeepSeek 标 Ignored），由请求顶层字段过滤负责（见 ⑤ messages 净化外的字段处理）；前者是 content block，由本细则处理。

**`sanitize_thinking_blocks` 细则**（按 effective_thinking_enabled）：

| effective_thinking_enabled | thinking 块 | redacted_thinking 块 |
|---------------------------|-------------|---------------------|
| true | **保留**（Reasoner 需要历史 thinking） | 删除 |
| false | 删除 | 删除 |

仅处理 `role == "assistant"` 的 content；user/system 不动。

**`normalize_tool_result_content`**：
- `tool_result.content` 为字符串 → 不动
- 为数组 → 拼接：text 块取 `text` 字段，其它 dict → JSON 序列化，按 `\n` 连接
- 为 dict → JSON 序列化
- 为 None → 空串

**`strip_reasoning_content`**：删除消息顶层的 `reasoning_content` 字段。

> **决议（v4.11 评审 #1，依据 DeepSeek 官方 Anthropic 兼容文档）：** Anthropic 兼容端点（`/anthropic`）通过 `content[]` 数组返回 `{"type":"thinking", ...}` 块（compatibility 表 "Message Fields → array, type = thinking" 为 **Supported**，输入与输出对称），**不**在顶层 `reasoning_content` 字段返回 CoT。"工具调用循环必须回传 reasoning"是 native（OpenAI-compat）端点规则；在 Anthropic 兼容端点上等价规则是「上一轮 assistant 的 `thinking` 块必须保留进 messages 中下一轮」，已由 `sanitize_thinking_blocks` 在 `effective_thinking_enabled=true` 时实现（保留 thinking、删 redacted_thinking）。
>
> 因此：
> - **锁定删除策略**：`strip_reasoning_content` 仅作为防御性清理（处理客户端误从 native 端点带来的顶层 `reasoning_content`），保持当前实现。
> - **不**做 `reasoning_content → 合成 thinking 块` 的转换：会与 Anthropic 兼容端点真正返回的 `thinking` 块叠加，造成历史 thinking 重复或与 SSE 块索引重映射冲突。
> - 单测断言：(1) 删除顶层 `reasoning_content` 字段；(2) 同时存在 `content` 中 `thinking` 块时不被影响；(3) `effective_thinking_enabled=true` 下原 `thinking` 块原样保留进 sanitized messages。
>
> **若**实测 Pro + MCP 多轮工具循环出现 unsafe_tool_followup 高降级率：先核查 SSE thinking 块解码 / `sanitize_thinking_blocks` 误删，**而非**回退到合成 thinking 转换路径。

**空 content 兜底（v4.6 评审 #4 统一）**：过滤后若消息 `content` 为空数组，**必须**替换为非空 text 块 `[{"type":"text","text":"(empty)"}]`。**不**使用空字符串 `""` 或 `text:""`，部分服务对空 content 会返 400。`patch_non_streaming_response` 中的兜底亦应使用同一占位（保持单元测试断言一致）。

**⑥ context_management.edits 过滤**
- 删 `edits` 中 `type` 以 `clear_thinking_` 开头的条目
- 若 `edits` 变空数组则从 context_management 删 edits 字段
- 若 context_management 变空对象则整体删除

**⑦ Tool 顺序修复（split-and-promote，评审 #1 修正）**

> **再修正：** 上一版「保守不搬运」漏掉了「tool_result 存在但不与 tool_use 相邻」的常见乱序场景，仍会触发 422。改为以**块级别 split** 方式处理三种情况：

> **针对 v4.7 评审 #1（索引稳定性）；v4.14 评审 #3 修正：** 步骤 2 / 2.5 / 3 都会插入、删除、移动 message/block，初始 `(message_idx, block_idx)` 索引会立刻失效。本节采用 **plan-then-apply** 模式：
>
> - **步骤 1-3 阶段只构造操作计划 / 写 in-place 标志，不改变 messages 的 Vec 形状**。所有引用基于初始 snapshot 索引。
> - **步骤 4 统一应用 plan，采用三阶段 A→B→C 排序**（详见步骤 4 / `tool_repair.rs` 摘要）：
>   - **阶段 A — DeleteBlock**：按 `user_idx` 分组，组内 `block_idx` 降序逐块 remove；不影响 message 数量
>   - **阶段 B — SplitAndPromote / SynthesizePlaceholder**：按 `insert_after_assistant_idx` 降序应用，每 op 单点 `insert` + 条件单点 `paired remove`；每次 mutate 后维护 `snapshot_to_current: HashMap<usize, usize>` 用于把 snapshot user_idx 翻译为当前实际位置
>   - **阶段 C — RemoveEmptyUser**：按 `user_idx` 降序，通过 `snapshot_to_current` 翻译后 `messages.remove`
> - 涉及 message 整体插入 / 删除时，**不允许中途读取 snapshot 之外的索引**：插入 synthetic user 的位置以「target assistant 在初始 snapshot 中的 `message_idx + 1`」表达，运行时由 `snapshot_to_current` 翻译；阶段 B 内严禁区间替换，避免误删中间消息（v4.9 #3）。
> - block 标 `_dsk_accepted` 是 in-place 字段写入，不动 Vec 形状，可在步骤 2 即时执行（不进入 plan）。
>
> 不再使用「按 message_idx 倒序 + block_idx 倒序统一应用」这一单序方案——它无法表达「同一 mutate 步骤里既要在 message 层面 insert 又要在 block 层面 remove」的依赖。三阶段排序通过把 message 形状变化与 block 内删除分离，彻底避免「正序处理多个 assistant 时索引漂移」与「block 删除被 message insert 推移」两类 bug。

**步骤 1 — 索引建表（snapshot）：**
- 扫描所有 assistant 消息，记录 `(message_idx, block_idx, tool_use_id)`；同一 assistant 内多个 tool_use 按 block_idx 排序得到 `expected_ids`
- 扫描所有 user 消息，记录 `(message_idx, block_idx, tool_use_id)` for tool_result blocks
- **此阶段只读，不修改 messages**

**步骤 2 — 按 assistant message 分组判定（构造 plan）：**

> 步骤 2 仅构造一份 `plan: Vec<RepairOp>`，例如 `RepairOp::SplitAndPromote { source_user_idx, blocks_to_extract: Vec<block_idx>, synthetic_blocks: Vec<Value>, insert_after_assistant_idx }` / `RepairOp::SynthesizePlaceholder { ... }`。**不**直接 mutate messages。同时给已确认的「就位」tool_result block 即时打 `_dsk_accepted: true`（in-place，不动 Vec 形状，安全）。

对每条 assistant 消息，**收集其中所有 `tool_use` 块的 ids**，按 content 出现顺序记为 `expected_ids = [id1, id2, ...]`。检查紧随其后的 user 消息（如有）：

- 取该 user 消息**前 N 块**（N = `expected_ids.len()`），若它们都是 `tool_result` 且 `tool_use_id` 序列**按顺序**与 `expected_ids` 完全相等 → **case (a) 不动**（已正确，包括「同一 assistant 多 tool_use → 同一 user 多 tool_result 顺序匹配」的常规形态）。
- 否则进入分类：

| 情况 | 处理 |
|------|------|
| (a) 紧随 user 消息前 N 块按顺序连续匹配 `expected_ids` | 不动（已正确） |
| (b) 部分 / 全部匹配的 tool_result 存在于 messages 中，但顺序、位置不满足 (a) | **split-and-promote（v4.14 评审 #1：限定单一 source user）**：选定**单一** `source_user_idx`（启发：含**最多**匹配 tool_result 的 user；并列时取**离 assistant 最近**的下游 user）。从该 source user 抽出其中匹配 expected_ids 的 tool_result 块；其它 user 中的「迟到合法」tool_result 由步骤 3 DeleteBlock 清理（v4.6 评审 #3）；source user 内未被抽出的 expected_id 视为「该 expected_id 在本次 case (b) 范围内未命中」→ 在合成 user 中以 placeholder 占位（**注意**：本场景仍是 SplitAndPromote，**不是** SynthesizePlaceholder——见下方 case (c) 边界）。按 expected_ids 顺序合成单条新 user 消息 `{"role":"user","content":[<tool_result_or_placeholder_1>, ...]}` 插入到 assistant 之后；source user 保留剩余非匹配块由步骤 2.5 final_remaining 处理。 |
| (c) **全部** expected_id 在 messages 中**完全无**匹配 tool_result | **SynthesizePlaceholder**：合成单条新 user 消息，content 全部为占位块 `{"type":"tool_result","tool_use_id":"<missing_id>","content":"[no result]","is_error":false}`，按 expected_ids 顺序排列；该消息整体只插入一次。**严格区分**：只要至少一个 expected_id 在 messages 中存在可抽取的 tool_result（即使在远端 user），即走 case (b) SplitAndPromote 路径（其余缺失 expected_id 在 synthetic_blocks 内 placeholder 占位混排）；只有当 messages 中**没有任何**对应 tool_result 时才进入 case (c) SynthesizePlaceholder。 |

**步骤 3 — 重复 / 孤立 tool_result 清理（v4.5 评审 #1 修正；v4.10 评审 #2：步骤顺序调整为 2 → 3 → 2.5 → 4）：**

> **背景（v4.5 评审 #1 修正旧逻辑 bug）：** 旧步骤 3 用 `consumed_tool_result_ids: HashSet<String>` + 扫描所有 user 消息的策略，会把 case (a) 已正确就位的 tool_result、case (b) 新合成消息中的 tool_result 也按「id 已 consumed」误删。改为**按 block 身份**而非 id 跟踪「保留集合」。

> **针对 v4.10 评审 #2：** v4.9 把这一步放在步骤 2.5 之后，但 2.5 计算 `final_remaining = original \ extracted ∪ deleted` 需要 `deleted_by_user`，而 DeleteBlock 是步骤 3 的产出。按 v4.9 顺序实现，2.5 看到的 deleted_by_user 是空集 → 残留的孤立/重复 tool_result 会被并入 paired_remaining_blocks 重新塞回 synthetic user，到步骤 3 时它们已不在「未 accepted 的源残留」里（因已被纳入聚合路径）。**修复：把残留扫描提前到步骤 2.5 之前**，确保 deleted_by_user 在聚合阶段已就绪。

实现策略：
- 步骤 2 执行过程中，**对每个最终保留下来的 tool_result block 在 block 上挂一个临时 `_dsk_accepted: true` 字段**（serde_json::Value 是 `serde_json::Map<String, Value>` 可以直接 `obj.insert("_dsk_accepted".into(), Value::Bool(true))`），序列化前统一清掉。
  - case (a) 整段不动 → 紧随 user 前 N 块全部标 `_dsk_accepted`
  - case (b) 抽出后放入新合成 user → 抽出的 tool_result block 标 `_dsk_accepted`
  - case (c) 新合成的占位块 → 直接构造时即带 `_dsk_accepted`
- **扫描"未带 `_dsk_accepted` 标志"的 tool_result 残留块**（即原始 messages 中没有被步骤 2 主动处理的 tool_result）：
  - 若 `tool_use_id` 在 expected_ids 全集中**无对应** assistant tool_use → 加入 `RepairOp::DeleteBlock`（孤立）
  - 若 `tool_use_id` 在全集中存在 → **加入 DeleteBlock**（v4.6 评审 #3：要么在步骤 2 提升，要么删除；不在原地保留以免再次造成非相邻 tool_result → 422）
- 这些 DeleteBlock 写入 `deleted_by_user: HashMap<usize, HashSet<usize>>`，供步骤 2.5 全局聚合
- 若该 user 消息 content 因 DeleteBlock 全部清空 **且 `user_will_be_removed[u] == false`**（即同 v4.12 评审 #2 引入的等价前置信号：`extracted_by_user[u]` 为空 AND 无任何 SynthesizePlaceholder.candidate_source_user_idx == Some(u)，因此后续 paired 绑定不会把整条 remove 责任写到任何 op 上）→ 在步骤 2.5 预计算结束时为该 user 追加一条 `RepairOp::RemoveEmptyUser { user_idx: u }`，由步骤 4 阶段 C 独立 remove 整条并更新 `snapshot_to_current`；若 `user_will_be_removed[u] == true`，则**不**追加 RemoveEmptyUser（整条 remove 由 paired 4b 完成；同一 user 不能被阶段 B 与阶段 C 双重 remove，否则阶段 C 查 `snapshot_to_current[&u]` 时会因键已被 `after_remove` 删除而 panic）
- **序列化前清除所有 `_dsk_accepted` 临时字段**（在 sanitize_request 末尾统一遍历 messages 一次）

> **针对 v4.6 评审 #2：** 不要使用 `HashSet<*const Value>`：messages 是 `Vec<Value>`，步骤 2 会插入 / 删除 / 移动 Value，raw pointer 会因 Vec realloc 或 move 失效，引发 use-after-free 风险。`_dsk_accepted` 临时字段方案随 Value 一起移动，天然稳定；性能成本可忽略（仅 tool_result 块增加一个 bool 字段）。
>
> **针对 v4.6 评审 #3：** 「迟到的合法 tool_result 原地保留」会留下非相邻 tool_result → DeepSeek 仍 422。把这种情况视为前置索引/提升逻辑 bug：到步骤 3 还存在的未 accepted tool_result（且 expected_ids 全集中有对应）一律视为「步骤 2 应已抽取但未抽取」的残留，**删除**而非原地保留。这强制步骤 2 的提升逻辑覆盖所有合法 case，调试时露馅而不是悄悄留下 422 隐患。

**步骤 2.5 — 连续 user 消息合并 + paired 聚合绑定（v4.4 评审 #5 新增；v4.10 评审 #2 顺序调整后执行）：**

> **背景：** Anthropic Messages API 严格要求 `user` / `assistant` 角色交替；DeepSeek Anthropic 端点同样校验。split-and-promote 抽出 tool_result（或 case (c) 合成 placeholder）后，原 user 消息可能保留 text/image 块，与新合成的 user(tool_result) 形成 `assistant → user(tool_result) → user(text)` 连续 user 消息，会触发 422。

处理（v4.5 #2 + v4.7 #3 + v4.8 #1/#2 + v4.9 #1/#2/#3 + v4.10 #2/#3 修正）：

**预计算阶段（步骤 3/2.5 之间，仅基于 snapshot；v4.10 #2 已确保 deleted_by_user 在此阶段完整）：**
- 维护 `extracted_by_user: HashMap<usize, HashSet<usize>>`：source user 到「本轮所有 SplitAndPromote 从该 user 抽取的 block_idx 集合」（合并多个 op 的 `blocks_to_extract`）
- 维护 `deleted_by_user: HashMap<usize, HashSet<usize>>`：source user 到「步骤 3 标记 DeleteBlock 的 block_idx 集合」（v4.10 #2：步骤 3 已先于本步执行）
- 维护 `accepted_in_place_by_user: HashMap<usize, HashSet<usize>>`：source user 到「带 `_dsk_accepted=true` 但**未**在 `extracted_by_user[u]` 中的 block_idx 集合」——即 case (a) 标记保持原位的 tool_result block（步骤 2 case (b) 抽出的 block 同样带 `_dsk_accepted`，但已计入 extracted，需从此集合排除）
- **冲突降级（v4.11 评审 #2 / v4.12 评审 #2 修正前置时序）**：v4.11 写「检查 `paired_source_user_idx == Some(u)`」是错的——本步骤运行时 paired 绑定尚未发生（步骤 2 默认 `paired_*=None`，绑定在本预计算之后），条件永远不会成立。改为基于「**该 user 是否会被后续 paired 绑定 remove**」的等价判定信号：
  - 定义 `user_will_be_removed[u] = true` 当且仅当满足以下任一：
    - `extracted_by_user[u]` 非空（存在至少一条 SplitAndPromote.source_user_idx == u → paired 绑定时该 user 的「升序最后一个」op 必然写 `paired_source_user_idx = Some(u)`）
    - 存在至少一条 SynthesizePlaceholder 的 `candidate_source_user_idx == Some(u)`（同理，paired 绑定时该 candidate user 升序最后一个 op 会承担 remove）
  - 若 `accepted_in_place_by_user[u]` 非空 **且** `user_will_be_removed[u] == true` → 将 `accepted_in_place_by_user[u]` 整体并入 `deleted_by_user[u]` 并清空 `accepted_in_place_by_user[u]`，同时为每个降级 block 追加一条 `RepairOp::DeleteBlock`（保证 plan 与状态一致；apply 时该 user 整条已被 paired remove，DeleteBlock 路径会跳过）
- 对每个 source user 计算 `final_remaining_blocks_for_user: Vec<Value>` = `original_content` 按出现顺序保留**未在 `extracted_by_user[u] ∪ deleted_by_user[u] ∪ accepted_in_place_by_user[u]` 中**的 block 副本（深拷贝，apply 阶段不再读源）。即排除：① 已抽到 synthetic user 的 tool_result（extracted）、② 步骤 3 删除的孤立/重复 tool_result（deleted）、③ case (a) 标记保留原位的 tool_result（accepted_in_place）。降级后 accepted_in_place 已被并入 deleted，故只有「纯 case (a) 单作用、无 paired remove」的 source user 才会留下非空 accepted_in_place 集，那种情形 final_remaining 也不会被任何 op 引用，安全

**绑定规则（v4.10 评审 #3 时间线分流）：**

`final_remaining_blocks_for_user` 不能整体绑到「升序第一个」synthetic user，否则多 assistant 共享 source user + 残余 text 时会把 text 前移，破坏语义时间线。改为按 block 类型分流：

- **tool_result 残余** → 罕见且通常已被步骤 3 DeleteBlock 吸收（v4.6 评审 #3 强制要求未抽取的合法 tool_result 必须删除）；case (a) 标记保留原位的 tool_result 通过 `accepted_in_place_by_user` 显式从 final_remaining 排除集合扣除（前述预计算阶段定义），**不进入 final_remaining**；若 case (a) 与 case (b) 在同一 source user 共存（user 将被整条 remove），accepted_in_place 已通过冲突降级并入 deleted。所以实践中 final_remaining 仅含 text/image/document 等非 tool_result 残余
- **非 tool_result 残余（text / image / document）** → 绑到「该 source user 在 plan 中**升序最后一个**」 synthetic user 的 paired_remaining_blocks 上（按 `insert_after_assistant_idx` 升序选最后一个）。理由：原 user 的 text/image 在语义上「位于该 user 内 tool_result 块之后」，应紧贴最后一个被消费该 user 的 assistant 之下游，最贴近原始时间线
- **删除 source user 的责任**：仅由「升序最后一个」op 承担（即 `paired_source_user_idx = Some(source_idx)` 仅写入该 op）；其它作用于同一 source user 的 op 保持 `paired_*=空/None`

> **特殊形态：多 assistant 共享 source user 且 final_remaining 含 tool_result**：若按 case (a) 定义某些 tool_result 本应留原位，但 source user 又被另一个 assistant 通过 SplitAndPromote 整体清空（paired_source_user_idx 写入 → remove 整条），原位保留语义会被打破。这种形态视为前置 case (a) 判定与 case (b) split 冲突的 spec 边界 case，**保守降级**：若同一 source user 同时进入「至少一条 case (a) 保留」和「至少一条 SplitAndPromote 抽取」，把 case (a) 的 tool_result 也加入 deleted_by_user 并写一条额外 DeleteBlock（视同未 accepted），强制走 case (b) 路径之后整条 remove。单测必须覆盖此形态。

**Op 字段：**
- `RepairOp::SplitAndPromote` 与 `RepairOp::SynthesizePlaceholder` 都带 `paired_remaining_blocks: Vec<Value>` 与 `paired_source_user_idx: Option<usize>`（统一字段名替换 v4.8 的 `paired_remaining_user_idx`，明确含义为「该 op 是否承担删除某条 source user 的责任」）
- 仅当该 op 是「source user 的**升序最后一个** synthetic user」时 `paired_remaining_blocks` 非空、`paired_source_user_idx = Some(source_idx)`；否则均为空 / None（v4.10 评审 #3：从 v4.9 的「升序第一个」改为「升序最后一个」以保持时间线）
- case (c) 的 placeholder：若 `candidate_source_user_idx == Some(source)` 且本 op 是该 source 升序最后一个 → 同样绑定 `paired_remaining_blocks` + `paired_source_user_idx`，避免「placeholder + 原下游 user(text)」连续 user

**Apply 阶段顺序（步骤 4 详述）：**
- 拼成最终 synthetic user：`[synthetic_blocks..., paired_remaining_blocks...]`
- 按倒序 `insert_after_assistant_idx` 应用，每 op 走 insert + 条件 remove 两步独立 vec 操作
- **绝不**用 `messages[a+1..=p]` 区间替换：tool_result 来源 user 不一定紧邻 assistant，中间可能存在用户在多轮对话中插入的独立 user/assistant/system 消息，区间替换会误删这些无关历史

**不做全局连续 user 扫描**：用户原本独立的两条 user 消息（多轮工具循环之间的中间用户输入）保持独立。仅 case (b)/(c) 路径会产生 synthetic user；case (a) 不动消息结构。

> **针对 v4.9 评审 #1：** v4.8 让 `SynthesizePlaceholder` 只带 `insert_after_assistant_idx + synthetic_blocks`，case (c) 全缺失 + assistant 后原本紧跟 user(text) → 插入 placeholder user 后形成 placeholder→text 连续 user，与 v4.4 评审 #5 修复目标矛盾。**修复：placeholder op 同样 capture `paired_remaining_blocks` + `paired_source_user_idx`**，与 SplitAndPromote 走同一原子合并路径。
>
> **针对 v4.9 评审 #2 / v4.10 评审 #3：** v4.8 让 `SplitAndPromote` 各自计算 `paired_remaining_blocks = original \ blocks_to_extract`，但同一 source user 可能被多个 assistant 各自抽走部分 tool_result。逐 op 计算会让 op_A 把 op_B 已经抽走的 tool_result 当作 remaining 误并入 synthetic_A。v4.9 改为先按 source user 全局聚合 extracted∪deleted，再唯一绑定到该 source user 的第一个 synthetic user，但「升序第一个」会把残余 text 前移到该 user 之前的 assistant 后，破坏时间线。**v4.10 修复：改为唯一绑定到「升序最后一个」 synthetic user，使残余 text/image 紧贴最后一次消费该 user 的 assistant 下游，最贴近原始时间线；并补保守降级规则避免 case (a)/case (b) 在同一 source user 共存导致的语义冲突。**
>
> **针对 v4.9 评审 #3：** v4.8 写「`messages[insert_after_assistant_idx + 1 ..= paired_remaining_user_idx]` 范围替换」隐式假设两者之间只有 paired user 一条；实际多轮对话中 paired user 可以远离 assistant，中间消息会被无差别覆盖。**修复：apply 拆为「insert 合成 user」+「remove paired user」两个独立 vec 操作，范围替换语义彻底删除。**

**步骤 4 — 统一应用 plan + 清理（v4.7 #1 + v4.8 #2 + v4.9 #1/#2/#3 + v4.10 #1/#3）：**

- 收集前述 step 2 / 3 / 2.5 累积的 `plan: Vec<RepairOp>` 与预计算的 `final_remaining_blocks_for_user`
- **按 source user 聚合（v4.8 #2 / v4.9 #2 / v4.10 #3 唯一绑定）**：每个 source user 的 `final_remaining_blocks` 唯一绑定到「该 user 在 plan 中升序**最后一个** synthetic user」（按 `insert_after_assistant_idx` 升序选最后一个，v4.10 #3 时间线修正）；其它作用于同一 source user 的 SplitAndPromote/SynthesizePlaceholder 的 `paired_remaining_blocks` 设为 `Vec::new()`、`paired_source_user_idx` 设为 `None`
- **初始化 `snapshot_to_current: HashMap<usize, usize>`，键为 0..messages.len()，值与键相等（identity）。** 每次对 messages 做 `insert` / `remove` 都同步维护此 map；所有 op 内对 snapshot 索引的引用统一通过 `snapshot_to_current` 查询当前实际位置（v4.10 #1：单 op 内的 `p_snap+1` 推算无法覆盖前序倒序 op 的 insert/remove 副作用，必须维护跨 op 的可变 idx 表）
- **三阶段总排序（v4.12 评审 #3）**：plan 中存在三类 op，各自字段不同（DeleteBlock 没有 `insert_after_assistant_idx`、RemoveEmptyUser 也没有），不能用单一 key 倒序。改为分阶段执行，每阶段内部用各自的天然 key 排序：

  **阶段 A — DeleteBlock（intra-message，块级）：**
  - 按 `user_idx` 分组；每组内按 `block_idx` **降序** 排序
  - 跳过条件不变（被 extracted/paired 接管的跳过）
  - 应用：`actual_user = snapshot_to_current[&user_idx]`；`messages[actual_user].content.remove(block_idx)`
  - 阶段 A 不修改 `messages` 长度，`snapshot_to_current` **保持不变**
  - 同一 user 内多块 DeleteBlock 必须 `block_idx` 降序，以避免删除靠前 block 后导致后续 block_idx 漂移
  - 跨 user 的 DeleteBlock 可任意顺序（互不影响）

  **阶段 B — 消息级 SplitAndPromote / SynthesizePlaceholder：**
  - 按 `insert_after_assistant_idx` **降序** 排序
  - 每 op 走 4a insert + 条件 4b remove；每次 mutate 后立即更新 `snapshot_to_current`
  - 倒序保证未来 op 的 `insert_after_assistant_idx` 严格小于当前已处理位置（其插入位置 `actual_a + 1` 不会向上推动当前已固化的 paired remove 位置）
  - paired remove 的 user 通过 `snapshot_to_current[&p_snap]` 翻译，远离 assistant 也只精确删一条

  **阶段 C — RemoveEmptyUser（消息级删除空壳）：**
  - 仅运行于步骤 2.5 预计算阶段已追加的 RemoveEmptyUser op（其 user 已在 paired 接管之外，且阶段 A 之后 content 实际为空）
  - 按 `user_idx` **降序** 排序（虽然通过 `snapshot_to_current` 翻译，但降序额外保险，避免 map 维护中的 corner case）
  - 应用：`actual_user = snapshot_to_current[&user_idx]`；`messages.remove(actual_user)`；`after_remove(&mut map, actual_user)`
  - 防御性断言：进入阶段 C 时 `messages[actual_user].content` 应为空数组（若非空说明步骤 2.5 预计算追加规则与阶段 A 删除集合不一致，是 bug）

```rust
// 维护 snapshot_to_current 的辅助闭包
fn after_insert(map: &mut HashMap<usize, usize>, pos: usize) {
    // 凡 current >= pos 的条目都向后挪 1（不包括刚插入的 synthetic，本身无 snapshot key）
    for v in map.values_mut() {
        if *v >= pos { *v += 1; }
    }
}
fn after_remove(map: &mut HashMap<usize, usize>, pos: usize) {
    // 先删除 value == pos 的 snapshot 键（被移除的 snapshot 不再存在）
    map.retain(|_, v| *v != pos);
    for v in map.values_mut() {
        if *v > pos { *v -= 1; }
    }
}
```

- **op 应用细则（按上述三阶段顺序执行）：**
  - **阶段 A** — `DeleteBlock { user_idx, block_idx }`：取 `actual_user = snapshot_to_current[&user_idx]`；仅当该 block 既未在任何 SplitAndPromote 的 `extracted_by_user[user_idx]` 中、也未被任何 paired 合并通过整条 remove 路径处理时，才单独 mutate（直接修改 `messages[actual_user].content`，从中移除 `block_idx` 对应 block）。否则已通过聚合消化（要么将被阶段 B 抽走、要么将被阶段 B 的 remove paired user 整条删除），**跳过**。DeleteBlock 仅删 block 不删 message，不更新 `snapshot_to_current`
  - **阶段 B** — `SplitAndPromote { source_user_idx, insert_after_assistant_idx, synthetic_blocks, paired_remaining_blocks, paired_source_user_idx }`：
    - 拼成单条 synthetic user：`{"role":"user","content": [synthetic_blocks..., paired_remaining_blocks...]}`
    - **步骤 4a — insert：** 取 `actual_a = snapshot_to_current[&insert_after_assistant_idx]`；执行 `messages.insert(actual_a + 1, synthetic_user)`；调用 `after_insert(&mut map, actual_a + 1)`
    - **步骤 4b — remove paired（条件）：** 若 `paired_source_user_idx == Some(p_snap)`：取 `actual_p = snapshot_to_current[&p_snap]`；执行 `messages.remove(actual_p)`；调用 `after_remove(&mut map, actual_p)`
    - **绝不**用 `messages[a+1..=p]` 区间替换：tool_result 来源 user 不一定紧邻 assistant，中间可能有用户在多轮中插入的独立 user/assistant/system，区间替换会误删（v4.9 #3）
  - **阶段 B** — `SynthesizePlaceholder { insert_after_assistant_idx, synthetic_blocks, candidate_source_user_idx, paired_remaining_blocks, paired_source_user_idx }`：与 SplitAndPromote 相同的 4a + 4b 流程（v4.9 #1：placeholder 同样支持 paired 合并；v4.10 #4：candidate_source_user_idx 仅参与步骤 2.5 聚合分类，apply 阶段忽略）
  - **阶段 C** — `RemoveEmptyUser { user_idx }`（步骤 2.5 预计算阶段追加）：取 `actual_user = snapshot_to_current[&user_idx]`；执行 `messages.remove(actual_user)`；调用 `after_remove(&mut map, actual_user)`
- 应用完成后，**遍历整个 messages 一次清除所有 `_dsk_accepted` 临时字段**

**关键不变量（v4.10 加固）：**
- snapshot 索引 → 实际索引转换通过 `snapshot_to_current: HashMap<usize, usize>` 跨 op 维护；每次 `messages.insert` / `messages.remove` 立即同步更新该 map。**禁止**用「`p_snap + 1` 仅考虑当前 op 自身 insert」的局部推算（v4.10 #1：前序倒序 op 的 insert/remove 也会推动 paired_source_user_idx 与 insert_after_assistant_idx 在当前状态下的实际位置）
- 倒序处理保证后续 op 的 insert 不影响本 op **未来要查询的** snapshot 索引（后续 op 的 `insert_after_assistant_idx` 严格小于当前；其插入/删除位置严格在当前已处理位置之下）；但本 op 自身需要从已被前序倒序 op 修改过的 messages 中读取实际位置，因此 snapshot_to_current map 是必需的
- 同一 source user 的 `final_remaining_blocks` **唯一**写入一次，由「升序最后一个 synthetic user」承担（v4.10 #3）；该 source user 的删除责任也由同一 op 承担
- 区间替换（`messages[a..b] = vec![x]` 或 `splice(a..b, ...)`）禁用，全部走 `insert(p, x)` + `remove(p')` 的单点操作

> **针对 v4.8 评审 #2 / v4.9 #2 / v4.10 #3：** v4.7 「先 block 抽取/清理，再 message 整体插入/删除/合并」让同一 source user 多 op 互相干扰；v4.8 改为「按 source user 聚合 + snapshot 一次性预计算」，但若每个 SplitAndPromote 各自带 `paired_remaining_blocks` 仍会重复并入。v4.9 在聚合基础上加「唯一绑定」（升序第一个），v4.10 修正为「升序最后一个」以保持 text/image 残余的语义时间线。
>
> **针对 v4.9 评审 #3：** 区间替换隐式假设 a..=p 之间只有 paired user 一条；实际多轮对话中 paired user 可以远离 assistant（中间存在独立的用户输入或工具循环），区间替换会无差别删掉这些消息。改为「insert + 单独 remove paired」两步，paired_source_user_idx 即使在多轮中远离也只点删一条。
>
> **针对 v4.10 评审 #1：** v4.9 写「snapshot p_snap → actual_p 转换仅考虑本 op 的 4a insert（≥ insert_after+1 则 +1），倒序循环保证后续 op 不影响本 op 索引」——前半句错（前序倒序 op 也插入/删除过其它位置），后半句反向（错把「后续不影响」当作「前序也不影响」）。前序倒序 op 的 `insert_after_assistant_idx` 较大，但其插入位置 `prior_actual_a + 1` 与 paired remove 位置可能出现在当前 op 的 paired_source_user_idx 之前（snapshot 维度），从而把当前 paired_source_user_idx 的实际位置整体推后或拉前。改为统一通过 `snapshot_to_current` 查实际位置，单 op 推算彻底禁用。

**不做：**
- 不重排已正确顺序的消息
- 不做全局连续 user 合并（仅步骤 2.5 的「synthetic user ↔ 配对剩余原 user」一对一合并；用户原本独立的两条 user 消息保持独立）
- 不修改 system / 顶层非 tool_use 块

**单测覆盖：** (a)/(b)/(c) 各两条用例 + 多个 tool_result 在同一 user 消息混合 text 的 split 场景

**⑧ max_tokens 兜底**
- 若 `body["max_tokens"]` 为 null 或缺失，设为 `8192`

**⑨ tool_choice 处理（v4.4 评审 #1；v4.11 评审 #2 修正：白名单保留）**

> **背景与依据：** DeepSeek 官方 Anthropic 兼容文档（[anthropic_api.md → Tool Fields → tool_choice 表](https://api-docs.deepseek.com/guides/anthropic_api)）：
>
> | Value | Support Status |
> |-------|----------------|
> | none | Fully Supported |
> | auto | Supported (`disable_parallel_tool_use` ignored) |
> | any | Supported (`disable_parallel_tool_use` ignored) |
> | tool | Supported (`disable_parallel_tool_use` ignored) |
>
> 即 `tool_choice` 的 4 种 `type` 值在 Anthropic 兼容端点 **全部受支持**（含 Reasoner/Pro）。v4.4-v4.10 「无条件删除」会改变客户端语义——尤其 `tool_choice: {"type":"none"}`（强制不调用工具）和 `{"type":"tool","name":...}`（强制指定工具）。

```rust
// 白名单保留 type；清理已知忽略的子字段，避免传递错误信号
// v4.14 评审 #1：原版同时持有 tc_obj 借用与 obj.remove 触发 E0499 借用冲突；
// 改为「内层作用域产出 owned Verdict，借用结束后再 mutate obj」。
fn sanitize_tool_choice(body: &mut Value) {
    let Some(obj) = body.as_object_mut() else { return; };
    if !obj.contains_key("tool_choice") { return; }

    enum Verdict {
        Keep,
        RemoveNonObject,
        RemoveUnknown(String), // owned kind，避开 &str 借用 tc_obj
    }

    // 内层作用域：tc_obj 借用在此结束
    let verdict = {
        let tc = obj.get_mut("tool_choice").expect("contains_key checked above");
        match tc.as_object_mut() {
            None => Verdict::RemoveNonObject, // 非 object 形态（极罕见）
            Some(tc_obj) => {
                // 立刻把 type 复制为 owned String，断开对 tc_obj 的 &str 借用
                let kind = tc_obj
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                match kind.as_str() {
                    "none" | "auto" | "any" | "tool" => {
                        // 文档明确：disable_parallel_tool_use 在 auto/any/tool 下被 ignored；
                        // 删除以减少上游解析负担，且避免 future 行为漂移
                        tc_obj.remove("disable_parallel_tool_use");
                        // "tool" 必须带 name；缺失则降级为 auto（保留意图：工具调用允许）
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
    }; // ← tc_obj / tc 借用在此结束，下面才能再次可变借用 obj

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

适用于 Pro 与 Flash 两个目标，无目标差异化（依据同一份 Anthropic 兼容表）。

> **回归实测要求（v4.11 #2）：** 若实测 Pro 端真的对某种 `tool_choice` 形态返 400，必须先把可复现请求体记录到本文档，再考虑追加更窄的删除分支；不再以「Pro 一律不支持」为由全删。

**⑩ stream 字段保留客户端原值**（不强制 true，用例支持非流式）

---

#### `tool_repair.rs`

实现上述 ⑦ 的完整 split-and-promote 流程（v4.7 评审 #1 升级为 plan-then-apply；v4.8 评审 #1/#2 把 split + insert + merge 合并为单一原子 op）：

1. **步骤 1 索引建表（snapshot 阶段，只读）**：扫描 assistant tool_use 与 user tool_result 的 `(message_idx, block_idx, tool_use_id)` 表，**不修改 messages**；同时为每个 user 消息保留 `original_content` 副本（snapshot），供 apply 阶段读取
2. **步骤 2 三类处理（构造 plan，不带 paired 字段；v4.14 评审 #2 严格边界）**：按 assistant message 分组、按 expected_ids 顺序匹配紧随 user 消息前 N 块；case (a) 不动；**case (b)（部分 / 全部命中，包括「部分命中 + 部分缺失」）** 构造 `RepairOp::SplitAndPromote { source_user_idx（v4.14 评审 #1：单一 source，启发为最多匹配的下游 user，并列取最近）, blocks_to_extract, synthetic_blocks（按 expected_ids 顺序合成；命中的 expected_id 放抽出的 tool_result，缺失的 expected_id 在同一 synthetic_blocks 内以 placeholder `[no result]` 占位混排）, insert_after_assistant_idx, ..默认 paired_*=空/None }`；**case (c)（全部 expected_id 在 messages 中无任何匹配 tool_result）** 构造 `RepairOp::SynthesizePlaceholder { insert_after_assistant_idx, synthetic_blocks（全部为占位）, candidate_source_user_idx=紧随 assistant 的下游 user idx（若存在）, ..默认 paired_*=空/None }`（v4.10 #4：candidate_source_user_idx 是步骤 2 的产出，供步骤 2.5 判定 placeholder 应参与哪个 source user 的聚合）。**严格不变量**：只要至少一个 expected_id 在 messages 任意 user 中能找到匹配 tool_result，就走 SplitAndPromote（其余缺失项内联 placeholder）；SynthesizePlaceholder 仅用于「全部缺失」。同时给已确认就位的 tool_result block in-place 写 `_dsk_accepted: true`（不动 Vec 形状）
3. **步骤 3 残留清理（构造 plan，先于步骤 2.5 执行；v4.10 #2 顺序）**：仅扫描未带 `_dsk_accepted` 的 tool_result 块；孤立 id / expected id 已 accepted 消费的均加入 `RepairOp::DeleteBlock`（v4.5 评审 #1）；**不**原地保留迟到合法 tool_result（v4.6 评审 #3）；这些 DeleteBlock 写入 `deleted_by_user`，供步骤 2.5 全局 final_remaining 计算时吸收，故 apply 时通常被聚合消化跳过
4. **步骤 2.5 全局 paired_remaining 聚合 + 唯一绑定（v4.9 #1/#2 + v4.10 #2/#3；在步骤 3 之后执行）**：先按 source user 全局合并 `extracted_by_user` ∪ `deleted_by_user` ∪ `accepted_in_place_by_user`（v4.10 #2 已确保 deleted_by_user 在此阶段完整）→ 一次性计算每个 user 的 `final_remaining_blocks`；再按 `insert_after_assistant_idx` 升序选**最后一个**作用于该 source user 的 op（含 SplitAndPromote 和 SynthesizePlaceholder），把 `paired_remaining_blocks` + `paired_source_user_idx = Some(source)` 唯一写入；后续（实际位于 plan 倒序中位置更小）作用于同一 source user 的 op 保持 `Vec::new()` / `None`
5. **步骤 4 三阶段应用 plan + 清理临时字段（v4.12 评审 #3 修正）**：见 sanitize_request 步骤 4 详述。**单序遍历已废弃**，改为三阶段：
   - **阶段 A — DeleteBlock**：按 `user_idx` 分组，组内 `block_idx` 降序逐块 remove；不修改 `snapshot_to_current`
   - **阶段 B — SplitAndPromote / SynthesizePlaceholder**：按 `insert_after_assistant_idx` 降序；每 op 走 `insert(actual_a + 1, synthetic_user)` + 条件 `remove(actual_paired_p)`，每次 mutate 后立即更新 `snapshot_to_current`，绝不区间替换
   - **阶段 C — RemoveEmptyUser**：按 `user_idx` 降序；每 op 通过 `snapshot_to_current[&user_idx]` 翻译为实际位置 `messages.remove`；防御性断言进入阶段 C 时该 user content 为空
   - 最后遍历整个 messages 清理 `_dsk_accepted`

**关键不变量：** 步骤 1-3 只读 / 构造 plan / 写 in-place 字段；步骤 2.5 全局聚合 + 唯一绑定 + RemoveEmptyUser 追加（基于 `user_will_be_removed[u]` 等价信号）；步骤 4 三阶段 A→B→C，单点 insert + 单点 remove。每个 source user 的 final_remaining 只写入一次；同一 user 不会被阶段 B paired remove 与阶段 C RemoveEmptyUser 双重处理；区间替换被禁用以避免误删中间消息（v4.9 #3）。

提供单元测试钩子。

公开 API：

```rust
pub fn repair_tool_order(messages: &mut Vec<Value>);

#[derive(Debug)]
enum RepairOp {
    SplitAndPromote {
        source_user_idx: usize,
        blocks_to_extract: Vec<usize>,             // snapshot block 索引
        synthetic_blocks: Vec<Value>,              // 抽出 tool_result（含 case (b) 部分缺失时的 placeholder 占位混排；case (c) 全缺失走 SynthesizePlaceholder）
        insert_after_assistant_idx: usize,
        // v4.9 #1/#2 + v4.10 #3/#4：步骤 2 写入 source_user_idx；步骤 2.5 全局聚合后写入 paired_*；
        // 仅升序最后一个 op 的 paired_remaining_blocks 非空、paired_source_user_idx = Some
        paired_remaining_blocks: Vec<Value>,       // snapshot 预计算的 final_remaining
        paired_source_user_idx: Option<usize>,     // 该 op 是否承担删除某条 source user 的责任
    },
    SynthesizePlaceholder {
        insert_after_assistant_idx: usize,
        synthetic_blocks: Vec<Value>,              // 全部为占位 [no result]（case (c) 全缺失）
        // v4.10 #4：步骤 2 写入「紧随 assistant 的下游 user idx，若存在」，供步骤 2.5 判定聚合归属
        candidate_source_user_idx: Option<usize>,
        // v4.9 #1：placeholder 同样支持配对合并，避免与下游 user(text) 形成连续 user
        // 步骤 2.5 决定该 placeholder 是否承担 candidate user 的删除（v4.10 #3：仅 candidate 升序最后一个 op 承担）
        paired_remaining_blocks: Vec<Value>,
        paired_source_user_idx: Option<usize>,
    },
    DeleteBlock { user_idx: usize, block_idx: usize },
    // v4.11 评审 #3：DeleteBlock 累计清空 user content 且无 paired 接管时，由步骤 2.5 预计算阶段追加
    RemoveEmptyUser { user_idx: usize },
    // v4.8 #1：Merge op 已删除，merge 语义并入 SplitAndPromote/SynthesizePlaceholder.paired_remaining_blocks
}
```

> **针对 v4.6 评审 #6：** v4.4 摘要只写「步骤 3 孤立 tool_result 清理」远不能覆盖 v4.4-v4.5-v4.6-v4.7 累积的逻辑（重复清理 / accepted 标记 / 配对合并 / 临时字段清理 / plan-then-apply 倒序），读者按简化版实现会漏掉 80% 的逻辑。

> **针对评审 v4.3 #1：** 「保守版」描述已废弃；本模块按 split-and-promote 实现，不做"含 text 不移动"的保守跳过，也不做"多个 tool_result 同属一条 user 消息不拆分"的保守跳过 —— 拆分恰恰是 split-and-promote 的核心能力。

---

#### `sse_state.rs`（吸收 free-claude-code `native_sse_block_policy.py`）

完整实现块索引重映射 + thinking 策略状态机：

```rust
#[derive(Default)]
pub struct SseBlockPolicyState {
    pub next_index: usize,
    pub by_upstream: HashMap<usize, UpstreamBlockState>,
    pub dropped_indexes: HashSet<usize>,
    pub pending_suppressed_stops: HashSet<usize>,
    pub message_stopped: bool,
    pub bypass: bool,  // v4.7 评审 #2：结构性错误后单向开关，参见错误处理策略
}

pub struct UpstreamBlockState {
    pub block_type: String,
    pub down_index: usize,
    pub open: bool,
    pub last_start_block: Option<Value>,
}

pub fn transform_native_sse_block_event(
    event: &str,
    state: &mut SseBlockPolicyState,
    fake_model: &str,
    thinking_enabled: bool,
) -> Vec<String>;  // 评审 #3：空 Vec=丢弃；多元素=「合成事件 + 转发事件」
```

> **针对评审 #3：** 返回 `Vec<String>` 是必需的。例如规则 2 要先合成 `content_block_stop`、再转发改写后的 `content_block_start`，单返回值无法承载。`Vec::new()` 表示 drop。

**核心规则（移植 free-claude-code）：**

1. **`message_start` 事件：** 改写嵌套 `payload.message.model = fake_model`，返回 `vec![改写后事件]`
2. **`content_block_start`：**
   - 若 block.type 应丢弃 → 记录 upstream_index 到 `dropped_indexes`，返回 `vec![]`
   - 否则若已有其它块开着，先**合成** `content_block_stop` 关闭旧块并标记 pending_suppressed_stops
   - 分配新下游 index，回写 payload["index"]
   - **返回 `vec![合成的 stop, 改写后的 start]`**（多元素）
3. **`content_block_delta`：**
   - upstream_index 在 dropped_indexes → `vec![]`
   - delta_type 应丢弃 → `vec![]`
   - upstream 段已关闭但又来 delta → 重新分配新 index、合成新 `content_block_start`，返回 `vec![合成 start, 改写后 delta]`
   - 无对应 start 的孤儿 delta → 合成 start，返回 `vec![合成 start, 改写后 delta]`
   - 否则按 by_upstream 映射改写 index，返回 `vec![改写后 delta]`
4. **`content_block_stop`：**
   - dropped_indexes / pending_suppressed_stops → `vec![]`
   - open 段 → 改写 index、置 open=false，返回 `vec![改写后 stop]`
   - 已 closed 段 → `vec![]`（去重）
5. **其它（`message_delta`、`message_stop`、`ping`、unknown）：** 原样返回 `vec![event.to_string()]`

**block 丢弃判定（评审 #4 修正）：**

```rust
fn should_drop_block_type(block_type: &str, thinking_enabled: bool) -> bool {
    // redacted_thinking：始终删（DeepSeek 不识别加密块；与历史块过滤策略一致）
    if block_type.starts_with("redacted_thinking") {
        return true;
    }
    // thinking：仅在 thinking 关闭时删（保留 Pro 推理 UI）
    !thinking_enabled && block_type.contains("thinking")
}
```

> **针对评审 #4：** 上版代码 `return !thinking_enabled` 与文字「始终删」相冲。统一为「`redacted_thinking` 始终删；普通 `thinking` 视开关」。

---

> **针对 v4.4 评审 #3（事件分隔符）：** `patch_sse_event` 的 `Vec<String>` 元素**不带** `\n\n`，仅保留事件正文（与切分时剥离对称）；`wrap_sse_stream` 在 yield 时**必须**用 `format!("{}\n\n", e)` 显式补回分隔符。这是契约一部分：单元测试 `patch_sse_event` 时直接对比 `"event: ...\ndata: {...}"`（无尾部 \n\n），流测试时校验 yield bytes 末尾必为 `\n\n`。直接 yield `Vec<String>` 元素会让客户端 SSE 解析器（按 `\n\n` 分块）无法识别事件边界。

##### 错误处理策略（v4.5 评审 #3 明确 + v4.7 评审 #2 升级 bypass_mode）

**单一原则：永不终止流；遇到结构性错误后切入 bypass_mode 全流原样透传。** patch 失败 = 当前事件按原样转发 + warn 日志；后续是否仍走 patch 视错误类型决定（见下表）。

> **日志宏（v4.6 评审 #1）：** 全文表格中 `warn!(...)` / `debug!(...)` 是 `log::warn!` / `log::debug!` 的简写（项目仅依赖 `log = "0.4"`，**未引入 `tracing`**）。具体写法：`log::warn!("malformed event line: {}", &raw_event)`，使用 `{}` 格式化而非 `tracing` 的结构化字段语法。

> **bypass_mode（v4.7 评审 #2 新增）：** `SseBlockPolicyState` 增加字段 `bypass: bool`（默认 false）。一旦置 true，后续所有事件**直接原样透传，不读取也不更新 state**，杜绝「raw index 与 patched index 混杂」。`bypass` 触发后**不可恢复**（流级单向开关）。

| 故障类型 | 处理 | 后续策略 | 日志级别 |
|---------|------|---------|---------|
| 事件 `event: <type>\n` 行缺失 / 格式异常 | 整段 raw 透传 | 进入 bypass_mode | `warn!("malformed event line")` |
| 事件 `data: ` 行 JSON 反序列化失败 | 整段 raw 透传 | 进入 bypass_mode | `warn!("json parse error: {}", e)` |
| `content_block_start.payload.index` / `block.type` 字段缺失 | 整段 raw 透传 | **进入 bypass_mode**（避免后续 delta/stop 索引重映射错乱） | `warn!("missing index/type")` |
| message_start 嵌套 `payload.message.model` 字段缺失或非字符串 | 整段 raw 透传，state 不更新 | 不进入 bypass（仅丢失伪装名，state 仍可正确处理后续 block） | `warn!("model field missing")` |
| 上游传来 unknown 事件类型 | 原样透传 | 不进入 bypass（已规则 5 默认） | 不打 warn |
| 流本身 IO 错误（chunk read 失败） | 沿用 upstream 错误向下游返回 `Result<Bytes, std::io::Error>` 的 Err 分支 | 流终止 | upstream 既有日志 |

**不**做：
- 不向客户端注入合成 error 事件
- 不抛 `io::Error::new(InvalidData, _)` 终止流（这一点修正了 v4.2 评审 #3 中的措辞）
- bypass_mode 一旦置 true 不可恢复（即使后续事件解析正常）

> **针对 v4.7 评审 #2 / v4.8 评审 #3：** 旧策略「结构性错误原样透传 + state 不更新」会让客户端在同一流内看到「patched index 0,1 → raw index 5（malformed start）→ patched/raw 混杂的 delta/stop」，SSE 解析器会因 index 不连续而拒绝或显示错乱。结构性错误（影响 state 一致性的）必须切入 bypass，**剩余流全部 raw 透传，止损后续损坏**。**注意：bypass 不能回滚已发出的 patched 事件**——若 malformed event 出现在已 patch 过若干事件之后，客户端仍会看到「前半 patched + 后半 raw」的混合时间线；这是结构性错误的固有代价，bypass 仅保证「错误点之后不再恶化」，不保证「客户端整流一致」。非结构性错误（如 model 字段缺失，仅伪装名缺失但 index/state 不影响）仍透传不进 bypass。

#### `sse_state.rs` 状态机字段补充（v4.7 评审 #2）

```rust
#[derive(Default)]
pub struct SseBlockPolicyState {
    pub next_index: usize,
    pub by_upstream: HashMap<usize, UpstreamBlockState>,
    pub dropped_indexes: HashSet<usize>,
    pub pending_suppressed_stops: HashSet<usize>,
    pub message_stopped: bool,
    pub bypass: bool,  // v4.7 评审 #2：结构性错误后单向开关
}
```

`transform_native_sse_block_event` 入口处优先判定：

```rust
if state.bypass {
    return vec![event.to_string()];  // 原样透传，不读不写其它字段
}
```

#### `sse_stream.rs`

```rust
pub fn patch_sse_event(
    event: &str,
    state: &mut SseBlockPolicyState,
    fake_model: &str,
    thinking_enabled: bool,
) -> Vec<String> {
    transform_native_sse_block_event(event, state, fake_model, thinking_enabled)
}

pub fn wrap_sse_stream<S>(
    upstream: S,
    fake_model: String,
    thinking_enabled: bool,
) -> impl Stream<Item = Result<Bytes, std::io::Error>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static
{
    // 内部状态：
    //   buffer: Vec<u8>           按 \n\n 切分缓冲
    //   state: SseBlockPolicyState
    // 处理步骤：
    //   1. 收集 chunk → 拼到 buffer
    //   2. 找到 \n\n → 切出完整 event 字符串（**剥掉**末尾 \n\n，仅保留事件正文）
    //   3. 调 patch_sse_event(event, &mut state, &fake_model, thinking_enabled) → Vec<String>
    //      - 返回值约定：每个元素是**不带尾部 \n\n** 的事件正文（与输入对称）
    //   4. 输出时 yield Bytes::from(format!("{}\n\n", e)) for e in vec
    //      - 空 Vec → 跳过；多元素 → 每个元素独立 yield，分隔符各自补回
    //      - 即使 patch 不改写事件，也必须用 format!() 重新拼接 \n\n，保证客户端收到完整 SSE block
    //   5. 末尾余量留在 buffer
}
```

> **针对评审 #6：** `patch_sse_event` 只处理完整事件；`wrap_sse_stream` 独占缓冲与切分职责。
>
> **针对 v4.2 评审 #3：** 错误类型与 `create_logged_passthrough_stream` 对齐为 `std::io::Error`（passthrough 流的既定类型），仅用于**上游 IO 错误透传**，不用于 patch 内部异常。patch 内部异常一律按上文「错误处理策略」永远透传 + warn，不构造任何 `io::Error`，避免与「事件原样透传」原则冲突（v4.5 评审 #3 修正）。
>
> **针对 v4.4 评审 #3（事件分隔符）：** `patch_sse_event` 的 `Vec<String>` 元素**不带** `\n\n`，仅保留事件正文（与切分时剥离对称）；`wrap_sse_stream` 在 yield 时**必须**用 `format!("{}\n\n", e)` 显式补回分隔符。这是契约一部分：单元测试 `patch_sse_event` 时直接对比 `"event: ...\ndata: {...}"`（无尾部 \n\n），流测试时校验 yield bytes 末尾必为 `\n\n`。直接 yield `Vec<String>` 元素会让客户端 SSE 解析器（按 `\n\n` 分块）无法识别事件边界。

---

#### `response_patch.rs`（评审 #5 修正）

```rust
pub fn patch_non_streaming_response(
    body: &mut Value,
    fake_model: &str,
    effective_thinking_enabled: bool,  // v4.3 评审 #5：与 SSE 状态机镜像
) {
    if let Some(obj) = body.as_object_mut() {
        // 顶层 model 字段
        if obj.contains_key("model") {
            obj.insert("model".into(), Value::String(fake_model.into()));
        }
        // content 数组：与 SSE 状态机完全镜像 —— redacted_thinking 始终 drop；普通 thinking 视开关
        if let Some(content) = obj.get_mut("content").and_then(|v| v.as_array_mut()) {
            content.retain(|block| {
                let Some(t) = block.get("type").and_then(|s| s.as_str()) else {
                    return true;
                };
                if t.starts_with("redacted_thinking") {
                    return false;  // v4.5 评审 #4：与 SSE 一致，整块丢弃
                }
                if t == "thinking" && !effective_thinking_enabled {
                    return false;  // SSE 状态机一致：thinking 关闭时整块丢弃
                }
                true
            });
            // 兜底：如果上一步把 content 清空，补一个非空 text 块（v4.6 评审 #4 与 sanitize 兜底统一）
            if content.is_empty() {
                content.push(json!({"type":"text","text":"(empty)"}));
            }
        }
    }
}
```

> **针对评审 #5（v4.2）：** Anthropic 非流式 Messages 响应 `model` 在顶层；`message_start.message.model` 是 SSE 嵌套结构。两条路径分开。
>
> **针对评审 #5（v4.3）：** 非流式响应必须按 `effective_thinking_enabled` 镜像 SSE 状态机的 thinking 块过滤策略：thinking 关闭时整块删除（避免 Flash 模式下泄漏推理输出），thinking 启用时保留。
>
> **针对评审 #4（v4.5）：** redacted_thinking 在 SSE 路径始终 drop（见 `should_drop_block_type`），非流式路径之前是「改写为占位 text」与之不一致。改为**两路径都 drop**，保持流式/非流式客户端可见行为完全一致；content 清空兜底由空 text 块负责。

---

#### 修改：`src-tauri/src/proxy/providers/claude.rs`（评审 #9）

`get_claude_api_format()` 第 36-42 行（meta.apiFormat 路径）：
```rust
"deepseek_anthropic" => "deepseek_anthropic",
```

第 51-56 行（settings_config.api_format 兼容路径）：同上加分支。

`claude_api_format_needs_transform()` 第 78-83 行：**不**加 deepseek_anthropic（保持返回 false → 走 passthrough）。

---

#### 修改：`src-tauri/src/proxy/providers/mod.rs`

```rust
pub mod deepseek_anthropic;
```

---

#### 修改：`src-tauri/src/proxy/forwarder.rs`（评审 #3、#4 修正）

**A. 在 `forward()` 内对 `mapped_body`（已存在的可变克隆）做 sanitize：**

```rust
// 现有代码：let mut mapped_body = body.clone();  或类似
let deepseek_sanitize_result = if matches!(
    resolved_claude_api_format.as_deref(),
    Some("deepseek_anthropic")
) {
    Some(deepseek_anthropic::sanitize_request(&mut mapped_body))
} else {
    None
};
```

> 不在 `body: &Value` 上 mutate；mapped_body 是早已存在的可变克隆。

**B. 构造 `ordered_headers` 时跳过 deepseek 不需要的头**（不在原始 headers 上 remove）：

```rust
const DEEPSEEK_HEADER_BLACKLIST: &[&str] = &[
    "anthropic-beta",
    "anthropic-dangerous-direct-browser-access",
];

let header_blacklist: &[&str] = if matches!(
    resolved_claude_api_format.as_deref(),
    Some("deepseek_anthropic")
) {
    DEEPSEEK_HEADER_BLACKLIST
} else {
    &[]
};

// 在 ordered_headers 构建循环中：
for (k, v) in headers.iter() {
    let name = k.as_str().to_ascii_lowercase();
    if header_blacklist.iter().any(|b| *b == name) {
        continue;
    }
    ordered_headers.push((k.clone(), v.clone()));
}
```

**C. 私有 `forward()` 返回签名扩展（评审 #4）：**

当前：
```rust
async fn forward(...) -> Result<(ProxyResponse, Option<String>), ProxyError>
```

改为：
```rust
async fn forward(...) -> Result<(ProxyResponse, Option<String>, Option<DeepseekContext>), ProxyError>

pub struct DeepseekContext {
    pub fake_model: String,
    /// = `SanitizeResult.effective_thinking_enabled`，由 sanitize_request 计算后**直接传递**：
    /// 来源 = (target_model 默认: pro=true / flash=false) 覆盖 (客户端显式 thinking) 再覆盖 (unsafe_tool_followup 强制 false)。
    /// 下游 SSE 状态机据此决定是否丢弃 thinking 块。
    pub effective_thinking_enabled: bool,
}
```

> **针对 v4.2 评审 #5：** 字段名与 `SanitizeResult.effective_thinking_enabled` 完全一致，避免命名漂移导致 SSE 端误用「客户端原始 thinking」而非「实际生效值」。

或更简单：把 `Option<DeepseekContext>` 改为塞进 `RequestContext`。建议后者，避免函数签名扩散。

**D. `forward_with_retry` 所有 success/retry/failover 分支统一打包到 `ForwardResult`：**

```rust
pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    pub claude_api_format: Option<String>,
    pub deepseek_context: Option<DeepseekContext>,  // 新增
}
```

每个 `Ok((response, claude_api_format)) =>` 分支补上 `deepseek_context`。

**E. `process_response` 签名扩展 + handler 调用点串联（v4.3 评审 #3 端到端伪代码）：**

> **背景：** v4.2 仅说明 `handle_streaming/non_streaming` 接收 `deepseek_context`，但实际 cc-switch 现有调用链是 `handle_claude_messages` → `process_response`（`handlers.rs:186`）→ 内部分发到 `handle_streaming` / `handle_non_streaming`。需要把 `deepseek_context` 沿这条链路传完。

修改 `src-tauri/src/proxy/response_processor.rs::process_response` 签名（v4.6 评审 #5 对齐现有类型）：

```rust
// 当前（response_processor.rs:333-344）：
pub async fn process_response(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
) -> Result<Response, ProxyError> {
    if is_sse_response(&response) {
        Ok(handle_streaming(response, ctx, state, parser_config).await)
    } else {
        handle_non_streaming(response, ctx, state, parser_config).await
    }
}

// 改为：
pub async fn process_response(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    deepseek_context: Option<&DeepseekContext>,  // v4.3 评审 #3：端到端透传
) -> Result<Response, ProxyError> {
    if is_sse_response(&response) {
        // handle_streaming 返回 Response（不带 Result），用 Ok(...) 包装
        Ok(handle_streaming(response, ctx, state, parser_config, deepseek_context).await)
    } else {
        handle_non_streaming(response, ctx, state, parser_config, deepseek_context).await
    }
}
```

> **针对 v4.6 评审 #5：** 现有签名是 `&UsageParserConfig`（不是 `ParserConfig`）；现有 `handle_streaming` 返回 `Response`（不带 `Result`），`handle_non_streaming` 返回 `Result<Response, ProxyError>`。`process_response` 自身返回 `Result<Response, ProxyError>`，所以流式分支必须 `Ok(handle_streaming(...).await)` 包装。前几版伪代码漏掉这两点，按那版实现会编译失败。

修改 `src-tauri/src/proxy/handlers.rs::handle_claude_messages`（`handlers.rs:186` 调用点）：

```rust
// 当前：
let response = result.response;
// ...
process_response(response, &ctx, &state, &CLAUDE_PARSER_CONFIG).await

// 改为：
let response = result.response;
let deepseek_context = result.deepseek_context.as_ref();  // v4.3 评审 #3
// ...
if needs_transform {
    return handle_claude_transform(response, &ctx, &state, &body, is_stream, &api_format).await;
    // 注：deepseek_anthropic 走 passthrough，不进入此分支；transform 路径无需感知 deepseek_context
}
process_response(response, &ctx, &state, &CLAUDE_PARSER_CONFIG, deepseek_context).await
```

**其它 process_response 调用点**（OpenAI / Codex / Gemini，`handlers.rs:489/543/597/662`）：传 `None`，因为非 Claude api_format 不会产出 `DeepseekContext`：

```rust
process_response(response, &ctx, &state, &OPENAI_PARSER_CONFIG, None).await
```

> **不变量：** 只有当 `result.claude_api_format == Some("deepseek_anthropic")` 时 `result.deepseek_context` 才会是 `Some(_)`；其它情况一律 `None`，签名扩展不引入运行时分支风险。

---

#### 修改：`src-tauri/src/proxy/response_processor.rs`（评审 #1 关键路径）

`handle_streaming` 接受 `deepseek_context: Option<&DeepseekContext>`，**在 `create_logged_passthrough_stream` 之后**追加 wrapper：

```rust
let logged_stream = create_logged_passthrough_stream(...);

let final_stream: BoxStream<'static, _> = if let Some(ctx) = deepseek_context {
    Box::pin(deepseek_anthropic::wrap_sse_stream(
        logged_stream,
        ctx.fake_model.clone(),
        ctx.effective_thinking_enabled,
    ))
} else {
    Box::pin(logged_stream)
};
```

> **针对 v4.2 评审 #1（wrapper 位置）：** wrapper **必须**放在 `create_logged_passthrough_stream` 之后（即更靠近客户端的一端）。理由：
> - **usage collector / 请求日志看到的是 DeepSeek 上游的真实事件**（`model: deepseek-v4-pro/flash`、原始 thinking 块、原始 index），便于成本核算与上游问题定位。
> - **客户端看到的是改写后的伪装事件**（`model: claude-opus-4-7`、按 effective_thinking_enabled 过滤、index 连续）。
> - 若颠倒顺序（先 patch 后日志），日志将记录改写后产物，丢失上游真相，且 usage 字段语义被破坏。

`handle_non_streaming` 的修改顺序**必须**为：① 解析 upstream JSON → ② 走既有 usage 解析与 `spawn_log_usage`（基于 upstream `model: deepseek-v4-pro/flash`）→ ③ 才对同一 JSON 调 `patch_non_streaming_response` → ④ 重新序列化 → ⑤ `strip_entity_headers_for_rebuilt_body` → ⑥ 回写客户端：

```rust
let (mut response_headers, status, body_bytes) =
    read_decoded_body(response, ctx.tag, body_timeout).await?;
strip_hop_by_hop_response_headers(&mut response_headers);

// ① 解析 upstream JSON
let mut json_value = serde_json::from_slice::<Value>(&body_bytes).ok();

// ② usage 日志（基于 upstream model 名，与流式语义一致）
// v4.8 评审 #5：必须保留现有 handle_non_streaming 的 3 个分支，否则非 JSON / 解析失败响应会丢日志
if let Some(ref json) = json_value {
    if let Some(usage) = (parser_config.response_parser)(json) {
        // 分支 1：JSON 解析成功 + usage parser 成功
        let model = usage.model.clone()
            .or_else(|| json.get("model").and_then(|m| m.as_str()).map(String::from))
            .unwrap_or_else(|| ctx.request_model.clone());
        spawn_log_usage(state, ctx, usage, &model, &ctx.request_model, status.as_u16(), false);
    } else {
        // 分支 2：JSON 解析成功但 usage parser 返回 None（既有 response_processor.rs:280-298）
        let model = json.get("model").and_then(|m| m.as_str())
            .unwrap_or(&ctx.request_model).to_string();
        spawn_log_usage(state, ctx, TokenUsage::default(), &model, &ctx.request_model, status.as_u16(), false);
        log::debug!("[{}] 未能解析 usage 信息，跳过记录", parser_config.app_type_str);
    }
} else {
    // 分支 3：非 JSON 响应（既有 response_processor.rs:300-315）
    log::debug!("[{}] <<< 响应 (非 JSON): {} bytes", ctx.tag, body_bytes.len());
    spawn_log_usage(
        state, ctx, TokenUsage::default(),
        &ctx.request_model, &ctx.request_model, status.as_u16(), false,
    );
}

// ③④⑤ patch + 序列化 + 剥离实体头
let final_body_bytes = if let (Some(ctx_dsk), Some(mut json)) = (deepseek_context, json_value) {
    deepseek_anthropic::patch_non_streaming_response(
        &mut json,
        &ctx_dsk.fake_model,
        ctx_dsk.effective_thinking_enabled,
    );
    let bytes: Bytes = serde_json::to_vec(&json)?.into();
    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    bytes
} else {
    body_bytes
};

// ⑥ 回写客户端
build_response(response_headers, status, final_body_bytes)
```

> **针对 v4.4 评审 #2（usage 与 patch 顺序）：** 与流式路径语义对齐 —— **usage / 日志看到 upstream 真实事件，客户端看到 patch 后伪装**。颠倒顺序会让非流式日志记录 `claude-opus-4-7` 而非 `deepseek-v4-pro`，破坏跨路径成本核算的一致性。`response_processor.rs:259-279` 既有 usage 解析路径**保持不动**，patch 块**插入到该路径之后**。

---

### 2. 前端 TypeScript

#### 修改：`src/types.ts`（评审 #9）

两处联合类型都加 `deepseek_anthropic`：

```ts
// ProviderMeta.apiFormat
apiFormat?:
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "gemini_native"
  | "deepseek_anthropic";

// ClaudeApiFormat
export type ClaudeApiFormat =
  | "anthropic"
  | "openai_chat"
  | "openai_responses"
  | "gemini_native"
  | "deepseek_anthropic";
```

#### 修改：`src/config/claudeProviderPresets.ts`（评审 #2 修正 + v4.11 评审 #3）

> **改用 `ANTHROPIC_API_KEY` 与 `apiKeyField: "ANTHROPIC_API_KEY"`**（DeepSeek 文档标注的 fully supported 头）

> **类型扩展（v4.11 评审 #3）：** 该文件顶部 `ProviderPreset` interface 的 `apiFormat` 字段是**独立联合类型**（与 `src/types.ts` 不共享），原仅含 `"anthropic" | "openai_chat" | "openai_responses" | "gemini_native"`。新增 preset 使用 `apiFormat: "deepseek_anthropic"` 会触发 TS 编译错误。**必须同步在该联合中追加 `"deepseek_anthropic"`**：
>
> ```ts
> apiFormat?:
>   | "anthropic"
>   | "openai_chat"
>   | "openai_responses"
>   | "gemini_native"
>   | "deepseek_anthropic";
> ```

```ts
{
  name: "DeepSeek (Claude Disguise · Flash)",
  websiteUrl: "https://platform.deepseek.com",
  apiKeyUrl: "https://platform.deepseek.com/api_keys",
  apiKeyField: "ANTHROPIC_API_KEY",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
      ANTHROPIC_API_KEY: "",
      ANTHROPIC_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-sonnet-4-6",
    },
  },
  apiFormat: "deepseek_anthropic",
  category: "cn_official",
  modelsUrl: "https://api.deepseek.com/models",
  endpointCandidates: ["https://api.deepseek.com/anthropic"],
  icon: "deepseek",
  iconColor: "#1E88E5",
},
{
  name: "DeepSeek (Claude Disguise · Pro)",
  websiteUrl: "https://platform.deepseek.com",
  apiKeyUrl: "https://platform.deepseek.com/api_keys",
  apiKeyField: "ANTHROPIC_API_KEY",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
      ANTHROPIC_API_KEY: "",
      ANTHROPIC_MODEL: "claude-opus-4-7",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-4-7",
    },
  },
  apiFormat: "deepseek_anthropic",
  category: "cn_official",
  modelsUrl: "https://api.deepseek.com/models",
  endpointCandidates: ["https://api.deepseek.com/anthropic"],
  icon: "deepseek",
  iconColor: "#1E88E5",
},
```

> 现有 `name: "DeepSeek"` 预设保留（走原生 deepseek-v4-* 名，不走 deepseek_anthropic 路径）。

#### 修改：`src/components/providers/forms/ClaudeFormFields.tsx`（v4.11 评审 #3）

API Format 下拉选择器在 `src/components/providers/forms/ClaudeFormFields.tsx:525-548` 硬编码 4 个 `<SelectItem>`（anthropic / openai_chat / openai_responses / gemini_native），缺 `deepseek_anthropic`。**必须新增第 5 个选项**：

```tsx
<SelectItem value="deepseek_anthropic">
  {t("providerForm.apiFormatDeepseekAnthropic", {
    defaultValue: "DeepSeek (Anthropic Compatibility)",
  })}
</SelectItem>
```

理由：(1) 编辑已有 deepseek_anthropic preset 时下拉框值不在选项列表 → Radix Select 显示空白触发器，UX 缺失；(2) 用户手动新建 provider 时无法选到该格式。新选项的禁用/可见规则与既有 4 项一致（受 `disabled` / `apiFormat !== "anthropic"` 等条件影响的代码路径见 192/456/465 行无需新增分支）。

#### 修改：`src/i18n/locales/{zh,en,ja}.json`

新增 i18n key：
- 1 个 key 说明自动过滤行为
- 1 个 key `providerForm.apiFormatDeepseekAnthropic`：zh="DeepSeek（Anthropic 兼容）"、en="DeepSeek (Anthropic Compatibility)"、ja="DeepSeek（Anthropic 互換）"

---

### 3. /v1/models 伪造路径（评审 #5 修正）

> **修正：** 实测 `src-tauri/src/proxy/server.rs:280-296` 现有路由仅有 `/v1/messages`、`/claude/v1/messages`、`/claude-desktop/v1/models`、`/claude-desktop/v1/messages`。**没有 `/v1/models`**。Claude Code 通过 `ANTHROPIC_BASE_URL` 调 `${BASE}/v1/models` 时会 404。

**改动 A — 新增路由：** `src-tauri/src/proxy/server.rs` `build_router()` 中新增（与现有 messages 路由对称）：

```rust
.route("/v1/models", get(handlers::handle_claude_models))
.route("/claude/v1/models", get(handlers::handle_claude_models))
```

**改动 B — 新增 handler：** `src-tauri/src/proxy/handlers.rs` 添加 `handle_claude_models`（v4.8 评审 #4：与 desktop handler 共用 `select_models_endpoint_provider` + `build_deepseek_disguised_models_payload`）：

```rust
pub async fn handle_claude_models(
    State(state): State<ProxyState>,
    _headers: HeaderMap,
) -> Response {
    // v4.4 评审 #6 / v4.5 评审 #5 / v4.8 评审 #4：通过共用 helper 选 provider，
    // 错误映射 4 分支与 RequestContext::new (handler_context.rs:133-139) / handle_claude_desktop_models 完全一致
    let provider = match select_models_endpoint_provider(&state, "claude").await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };

    if get_claude_api_format(&provider) == "deepseek_anthropic" {
        return Json(build_deepseek_disguised_models_payload(&provider)).into_response();
    }

    // 其它 api_format：返回 404，与改动前路由不存在时的客户端体验一致（不引入新行为）
    StatusCode::NOT_FOUND.into_response()
}
```

> **针对 v4.2 评审 #4（非 deepseek 行为）：** 改动前 `/v1/models` 路由根本不存在，请求会被 axum 直接 404 —— 这就是「现有行为」。原文「透传上游 /v1/models（保持现有行为）」措辞错误：从未存在透传逻辑，写入会**新增**一条上游调用路径，扩大改动面、引入新失败模式。改为**仅对 `deepseek_anthropic` 伪造，其它 api_format 返 404**，严格等价于路由不存在时的客户端体验。

**改动 C — 修改既有 `handle_claude_desktop_models`：** 当激活 provider 为 `deepseek_anthropic` 时，**复用 `handle_claude_models` 内部相同的伪装列表构造逻辑**返回；其它 api_format 维持既有桌面端行为不变（即继续走 `claude_desktop_config::model_list_response(provider)`）。

> **针对 v4.7 评审 #5：** 仅修改 `/v1/models` 而不动 `handle_claude_desktop_models`，会导致 Claude Desktop 独立网关命名空间（`/claude-desktop/v1/models`）在 deepseek_anthropic 下返回 `model_list_response` 计算的 `proxy_model_routes`（来源是 desktop 配置文件，与我们伪装的 `ANTHROPIC_*_MODEL` 不一致），客户端将看到对不上的模型 id。必须同步分支。

伪代码（仅展示新增分支与共用 helper，其余字段、auth 均沿用现有实现 `src-tauri/src/proxy/handlers.rs:92-106`）：

```rust
// v4.8 评审 #4 + v4.9 评审 #4：两条 models endpoint 共用 provider 选择 helper，错误映射保持一致
async fn select_models_endpoint_provider(
    state: &ProxyState,
    app_type_str: &'static str,
) -> Result<Provider, ProxyError> {
    let providers = state
        .provider_router
        .select_providers(app_type_str)
        .await
        .map_err(|e| match e {
            crate::error::AppError::AllProvidersCircuitOpen => ProxyError::AllProvidersCircuitOpen,
            crate::error::AppError::NoProvidersConfigured => ProxyError::NoProvidersConfigured,
            other => ProxyError::DatabaseError(other.to_string()),
        })?;
    providers.into_iter().next().ok_or(ProxyError::NoAvailableProvider)
}

pub async fn handle_claude_desktop_models(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Value>, ProxyError> {
    validate_claude_desktop_gateway_auth(&state, &headers)?;
    let provider = select_models_endpoint_provider(&state, "claude-desktop").await?;

    // v4.7 评审 #5 新增：deepseek_anthropic 走与 /v1/models 同款伪装构造
    if get_claude_api_format(&provider) == "deepseek_anthropic" {
        return Ok(Json(build_deepseek_disguised_models_payload(&provider)));
    }

    // 既有 path：从 desktop 配置 proxy_model_routes 渲染
    let response = crate::claude_desktop_config::model_list_response(&provider)
        .map_err(|e| ProxyError::ConfigError(e.to_string()))?;
    Ok(Json(response))
}

// handle_claude_models 同样调用 select_models_endpoint_provider("claude")，
// 不再各自拷贝 4 分支错误映射；保证两条 endpoint 在熔断 / 无 provider / DB 错误时返回相同 ProxyError

// 抽出 handle_claude_models 中 deepseek_anthropic 分支为共用函数（同一来源、同一去重顺序）
fn build_deepseek_disguised_models_payload(provider: &Provider) -> Value {
    const MODEL_ENV_KEYS: &[&str] = &[
        "ANTHROPIC_MODEL",
        "ANTHROPIC_DEFAULT_OPUS_MODEL",
        "ANTHROPIC_DEFAULT_SONNET_MODEL",
        "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    ];
    // v4.12 评审 #4：repo 中不存在 `read_provider_env` helper；按现有代码模式
    // （见 src-tauri/src/proxy/providers/gemini.rs:126 / model_mapper.rs:19）内联读取
    // settings_config["env"] 下的字符串字段
    let env_map = provider
        .settings_config
        .get("env")
        .and_then(|v| v.as_object());
    let mut disguised: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for key in MODEL_ENV_KEYS {
        let v = env_map
            .and_then(|m| m.get(*key))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        if let Some(s) = v {
            if seen.insert(s.to_string()) {
                disguised.push(s.to_string());
            }
        }
    }
    if disguised.is_empty() {
        disguised.push("claude-sonnet-4-6".to_string());
    }
    let data: Vec<Value> = disguised
        .into_iter()
        .map(|name| json!({ "type": "model", "id": name, "display_name": name }))
        .collect();
    json!({"data": data})
}
```

> **针对 v4.8 评审 #4：** v4.7 把 `handle_claude_models` 改为 4 分支错误映射（AllProvidersCircuitOpen / NoProvidersConfigured / DatabaseError / NoAvailableProvider），但 `handle_claude_desktop_models` 伪代码仍是单分支 `.map_err(|e| ProxyError::DatabaseError(e.to_string()))`。两条 endpoint 在同一种底层错误下返回不同 ProxyError 是语义 bug。改为共用 `select_models_endpoint_provider(state, app_type_str)` helper。

注意事项：
- 保持 `validate_claude_desktop_gateway_auth` 在 deepseek 分支**之前**调用，桌面端独立 token 校验不能因伪装而绕过。
- `handle_claude_models`（改动 B）需要重构为调用同一 `build_deepseek_disguised_models_payload`，保证两条 endpoint 输出严格一致；不要在两处分别拷贝构造逻辑。
- 其它 api_format 在两条 endpoint 上行为不同，须区分（v4.11 评审 #4）：
  - `/v1/models` 与 `/claude/v1/models`（`handle_claude_models`）：非 `deepseek_anthropic` → **404**。理由见 v4.2 评审 #4，改动前路由不存在，等价于 axum 直接 404，避免新增上游透传路径。
  - `/claude-desktop/v1/models`（`handle_claude_desktop_models`）：非 `deepseek_anthropic` → 维持既有 `claude_desktop_config::model_list_response(provider)` 路径（即 `proxy_model_routes` 渲染），零行为变更。

---

## 不改动的部分

- `transform.rs` / `transform_responses.rs` / `streaming.rs`：deepseek_anthropic 走 passthrough，完全不涉及
- `forwarder.rs` 重试 / failover 逻辑
- 既有 `name: "DeepSeek"` 预设（保留原生路径）

---

## 测试要点

**单元测试：**

`model_mapping`：
- opus → v4-pro；sonnet → v4-flash；haiku → v4-flash；未知 → v4-flash
- is_reasoner_target("deepseek-v4-pro") = true

`request_sanitizer`：
- **tools 黑名单**：保留 `{name:"Bash", input_schema:{...}}` 等普通工具；删除 `{type:"web_search_20250305"}` 与 `{name:"web_search"}`
- **thinking 字段重建**：
  - target=pro + client thinking enabled + 无 tool history → `{type:enabled, budget_tokens:N?}`
  - target=pro + tool_history 但无 replayable thinking → `{type:disabled}`（unsafe_tool_followup 降级）
  - target=flash + client 未指定 → **`{type:disabled}` 显式**（评审 #8）
- **output_config 白名单**：仅保留 `effort` 子字段，未知子字段（如 `verbosity`、`reasoning`）一律删除；`unsafe_tool_followup=true` 时 `effort` 也删除；清理后空对象整体删
- **历史块过滤**：effective_thinking_enabled=true 时保留 thinking、删 redacted_thinking；false 时两者都删
- **unsupported content blocks（v4.13 评审 #1，与正文 `strip_unsupported_attachments` 全表对齐）**：覆盖 DeepSeek 官方 Anthropic 兼容文档列出的 9 种 Not Supported type：`image` / `document` / `search_result` / `server_tool_use` / `web_search_tool_result` / `code_execution_tool_result` / `mcp_tool_use` / `mcp_tool_result` / `container_upload`。测试至少需覆盖：
  - **顶层 content 数组**：每种 type 各构造一条 user/assistant 消息含该块（混合或单独），断言过滤后该 type 不再出现；若整条消息因此 content 为空，应替换为统一占位 text（不串到 `(empty)` 兜底分支）
  - **tool_result.content 内嵌**：构造 `{"type":"tool_result","content":[{"type":"image",...},{"type":"text","text":"ok"}]}` 等内嵌不支持块的形态，断言内嵌不支持块被剔除；若内嵌列表全空（如 `[{"type":"image",...}]` → []）则放 `[attachment omitted]` 占位
  - **`mcp_tool_*` 与请求顶层 `mcp_servers` 不混淆**：分别测试两条路径，断言 `messages[].content[].type=="mcp_tool_use"` 由本规则剔除、请求顶层 `mcp_servers` 由独立字段过滤剔除（互不干扰、覆盖独立断言）
- **tool_result.content**：数组 → 字符串拼接
- **reasoning_content**：assistant 消息顶层删除
- **空 content**：替换为 `[{"type":"text","text":"(empty)"}]`（v4.6 评审 #4 统一占位）
- **mcp_servers**：删除
- **stream 字段**：保留客户端原值
- **tool_choice 处理（v4.4 评审 #1；v4.11 评审 #2 改为白名单）**：依据 DeepSeek 官方 Anthropic 兼容文档，`tool_choice` 的 `type ∈ {none, auto, any, tool}` 全部受支持（含 Reasoner/Pro），不再无条件删除：
  - `type ∈ {none, auto, any, tool}` → 保留 `type`（与 `name` for tool）；删除已知被 ignored 的 `disable_parallel_tool_use` 子字段
  - `type == "tool"` 但 `name` 缺失/空 → 降级为 `type: "auto"`，并删 `name`
  - `type` 为其它未知值 / 非 object 形态 → 整字段删除并 warn
  - body 不含 `tool_choice` → 两种 target 都不引入
  - **目标无差异化**：Pro 与 Flash 走同一份白名单逻辑

`tool_repair`（split-and-promote）：
- **case (a) 顺序匹配**：assistant 含 [tool_use A, tool_use B] + 紧随 user 含 [tool_result A, tool_result B] → 不动
- **case (a) 顺序不匹配**：assistant 含 [tool_use A, tool_use B] + 紧随 user 含 [tool_result B, tool_result A] → 进入 (b) 拆分重排
- **case (b) 单 tool_use 非首块**：tool_result 在 user 消息中夹在 text 之间 → split 抽出，合成新 user 消息插入到 assistant 之后
- **case (b) 多 tool_use 跨消息（v4.14 评审 #1：单一 source 不变量）**：assistant 含 `[tool_use A, tool_use B, tool_use C]`，匹配 tool_result 分散在多条 user 消息中（如 user_x 含 A、user_y 含 B+C）→ **不**跨多 source 抽取；按启发选定**单一** `source_user_idx`（含最多匹配的下游 user，并列取离 assistant 最近）。断言：(1) 仅从 source user 中抽出其命中的 tool_result；(2) 其它 user 中的「迟到合法」匹配 tool_result 由步骤 3 `DeleteBlock` 删除（不留在原位）；(3) source user 内未命中的 expected_id 在合成 synthetic_blocks 中以 placeholder `[no result]` 占位，按 expected_ids 顺序与抽出块混排；(4) 整体仍生成单条 synthetic user 紧随 assistant 插入
- **case (b) 剩余 text 合并（v4.4 评审 #5）**：原 user 消息抽出 tool_result 后保留 text 块，与新合成 user(tool_result) 相邻 → 步骤 2.5 合并为单条 user 消息，content 顺序 `[tool_results..., 原 text 块...]`
- **case (c) 完全缺失（v4.13 评审 #2 拆分）**：assistant 含 `[tool_use A, tool_use B]`，messages 中**全部** expected_ids 均无对应 tool_result（连下游 user 也无）→ 单条 SynthesizePlaceholder 合成 `[{"type":"tool_result","tool_use_id":"A","content":"[no result]","is_error":false}, {"type":"tool_result","tool_use_id":"B","content":"[no result]","is_error":false}]`，按 expected_ids 顺序排列，整体只插一次。**断言**：本场景下不存在「已抽取 tool_result」，placeholder 列表纯由合成块构成
- **case (b) 部分命中 + 部分缺失（v4.13 评审 #2 新增；v4.14 评审 #2 严格命名修正；最易出错的混排路径）**：assistant 含 `[tool_use A, tool_use B, tool_use C]`，A 与 C 的 tool_result 同时存在于**被选中的同一个 `source_user_idx`** 中（满足单一 source 不变量；若 A、C 跨多个 user，按 v4.14 评审 #1 启发选定单一 source，未命中的 expected_id 在 synthetic_blocks 中以 placeholder 占位）、B 在 messages 中**完全无**匹配 tool_result → **本场景整体走 case (b) `SplitAndPromote`，不是 case (c)**（case (c) 严格保留给「expected_ids 全部缺失」）。`synthetic_blocks` 内部直接内联构造 B 的 placeholder block `{"type":"tool_result","tool_use_id":"B","content":"[no result]","is_error":false}`，与抽出的 A、C tool_result 按 expected_ids 顺序混排；生成**单条** synthetic user 紧随 assistant 插入，content 顺序严格为 `[<tool_result A 抽出>, <placeholder B 内联>, <tool_result C 抽出>]`（按 expected_ids = [A, B, C] 顺序）。**断言**：(1) plan 中只产生 1 个 `RepairOp::SplitAndPromote`，**不**产生任何 `RepairOp::SynthesizePlaceholder`；(2) synthetic user 只插入一次；(3) content 顺序严格按 expected_ids，验证 placeholder 与抽出 block 的相对位置正确；(4) 选中 source user 的 `paired_remaining` 与 remove 路径仍按"升序最后一个"绑定规则处理
- **孤立 tool_result（步骤 3）**：tool_use_id 在表中无对应 → 直接删 block；user 消息因此清空则整条丢弃
- **重复 tool_result（v4.4 评审 #4）**：同一 tool_use_id 多个 tool_result block → 仅保留首次（按 (a)/(b) 优先级），后续重复全删；含同一 user 消息内重复与跨 user 消息重复两类
- **含 text 的 user 消息**：抽出 tool_result 后保留剩余 text 块（不再"保守不移动"）
- **`_dsk_accepted` 清理（v4.6 评审 #2 / v4.7 #1 / v4.10 #5 不变量）**：步骤 2 在判定 case (a)/(b)/(c) 时给确认保留的 tool_result block in-place 标 `_dsk_accepted=true`（步骤 1 仅 snapshot 只读、不写任何字段）；步骤 4 应用 plan 后必须遍历所有剩余 user 消息所有 block，清除该字段；测试断言最终 messages 任一 JSON 路径下不存在 `_dsk_accepted` key
- **多 assistant 正/逆序处理（v4.7 评审 #1 不变量）**：单次请求含 ≥2 个 `assistant(tool_use)` + 各自配对的下游 user(tool_result) → 步骤 1/2 按 message_idx 升序遍历构造 plan；步骤 4 倒序应用插入/删除（`insert_after_assistant_idx` 大者先动）。测试构造 3 个 assistant 块的 case，断言：每个 assistant 后紧邻一条合成 user(tool_result) 且 expected_ids 顺序正确；任一插入不破坏其它 assistant 与其合成 user 的相对位置
- **结构改写后无连续 user（v4.4 #5 / v4.7 #3 / v4.9 #1 不变量）**：步骤 2.5 对每个 plan 操作产生的合成 user 与其 `paired_source_user_idx` 配对合并；测试 case：原 user 抽出 tool_result 后剩 text 块、其前已合成新 user → 应合并为 `[tool_results..., 原 text 块...]` 一条 user，断言最终 messages 不存在两条相邻 user
- **未 accepted 合法 id 被删除（v4.6 评审 #3 不变量）**：构造 case：assistant 含 `tool_use A`；user(t-1) 含 `tool_result A` 但**位置非紧邻 assistant**（被一条无关 user 阻隔），且步骤 2 无法将其纳入 case (a)/(b)（例如 expected_ids 已通过其它来源补齐）→ 该 `tool_result A` 标记为「迟到合法」，**步骤 3 必须删除**（不原地保留）；断言最终 messages 不含该 block，且不出现非相邻 tool_result 触发 422 的形态
- **plan 三阶段应用（v4.12 评审 #4 替换 v4.7 单序倒序假设）**：构造一个 plan 同时含三类 op，例如：
  - `DeleteBlock { user_idx: 1, block_idx: 0 }` 与 `DeleteBlock { user_idx: 1, block_idx: 2 }`（同 user 两块）
  - `DeleteBlock { user_idx: 3, block_idx: 1 }`（另一 user）
  - `SplitAndPromote { insert_after_assistant_idx: 4, ... }` 与 `SynthesizePlaceholder { insert_after_assistant_idx: 6, ... }`
  - `RemoveEmptyUser { user_idx: 5 }`（user 5 被两条 DeleteBlock 清空且不被 paired 接管）

  直接调用 `apply_plan(messages, plan)`，断言：
  - **阶段 A 先于阶段 B**：DeleteBlock 全部完成后 `messages.len()` 不变（仅 content 内 block 减少），随后阶段 B 才插入/删除消息
  - **同一 user 内 DeleteBlock 按 block_idx 降序**：通过 hook 计数验证 user_idx=1 上的 `(block_idx=2)` 在 `(block_idx=0)` 之前被处理；否则正序删 block_idx=0 后剩余 block 整体左移、再删 block_idx=2 实际删到原 block_idx=3，行为错误
  - **阶段 B 内部按 `insert_after_assistant_idx` 降序**：`insert_after_assistant_idx=6` 的 op 先于 `=4` 的 op
  - **阶段 C 最后执行**：RemoveEmptyUser 在所有阶段 B op 完成之后才 mutate；通过 hook 计数验证调用顺序
  - **结果与基线一致**：与「逐操作正序应用 + 手动修正每步后所有 idx」的等价基线对比 messages 字面相等
  - **不再断言**「单一 message_idx 降序」（v4.7 假设已废弃；DeleteBlock/RemoveEmptyUser 没有 `insert_after_assistant_idx` 字段）
- **SynthesizePlaceholder + 下游 user(text) 合并（v4.9 评审 #1 不变量）**：构造 case：`assistant(tool_use A) → user(text "follow up question")`，A 在表中无 tool_result（case (c) 全缺失）→ 断言生成的 placeholder user `[no result]` 与下游 user(text) 合并为单条 `{"role":"user","content":[<placeholder tool_result>, {"type":"text","text":"follow up question"}]}`；最终 messages 不出现两条相邻 user
- **同一 source user 多个 SplitAndPromote（v4.9 评审 #2 + v4.10 评审 #3 / v4.11 评审 #4 不变量）**：构造 case：`assistant_1(tool_use A) → assistant_2(tool_use B) → user(text X, tool_result A, text Y, tool_result B, text Z)`（同一 user 同时含 A、B 的 tool_result，分别属于两个 assistant）→ 应生成两条 SplitAndPromote 共享同一 source_user_idx；断言：(1) **升序最后一个** op（紧随 assistant_2 插入的 synthetic user）携带 `paired_remaining_blocks = [text X, text Y, text Z]` 且 `paired_source_user_idx = Some(原 user idx)`，承担 remove 原 user 的责任；(2) 第一个 op（紧随 assistant_1）`paired_remaining_blocks = []`、`paired_source_user_idx = None`；(3) 最终 messages 中 tool_result A 仅出现一次（不被第二个 op 误并入）、tool_result B 仅出现一次、原 user 已被 remove；(4) text X/Y/Z 只出现一次，且语义位置在 assistant_2 后的 synthetic user 内（而非 assistant_1 后），保持原始时间线
- **paired user 远离 assistant 不可误删中间消息（v4.9 评审 #3 不变量）**：构造 case：`assistant(tool_use A) → user(text)（独立中间消息，无 tool_result）→ assistant(text response) → user(tool_result A)`（A 的 tool_result 在远离 assistant 的下游 user 中）→ tool_repair 应把该 tool_result 抽出合成紧随原 assistant 的 synthetic user，并 remove 远端 paired user；断言：(1) 中间的 `user(text)` 与 `assistant(text response)` 完整保留、未被任何形式删除；(2) 远端 paired user 已被精确 remove（仅一条）；(3) 整流不出现 `messages[a..=p]` 区间替换的痕迹（中间消息不丢失）

`sse_state` / `wrap_sse_stream`：
- chunk 边界切在事件中段 → 缓冲后正确合并
- message_start 嵌套 message.model 替换为 fake_model
- thinking_enabled=false 时 thinking 块的 start/delta/stop 三元组全部丢弃
- thinking_enabled=true 时仅 redacted_thinking 三元组丢弃
- 关闭后又来 delta → 重新分配 index 并合成新 start
- 孤儿 delta → 合成 start
- 索引重映射保证下游 index 连续递增
- 解析失败事件原样透传 + warn
- **事件分隔符（v4.4 评审 #3）**：`patch_sse_event` 返回值不含 `\n\n`；`wrap_sse_stream` 输出每个事件必须以 `\n\n` 结尾（即使 patch 未修改）；多元素 yield 时各自补 `\n\n`，不合并为单 chunk
- **bypass_mode 触发后所有事件原样透传（v4.7 评审 #2）**：构造流 `[正常 message_start, 正常 content_block_start(idx=0), malformed JSON event, 正常 content_block_delta]` → 断言：前 2 事件按 patched index 输出；malformed event 触发 bypass=true；后续 delta 原样透传（保留 upstream raw index，不再读 state）；warn 日志记录一次
- **bypass 不回滚已发事件（v4.8 评审 #3）**：构造流 `[message_start, content_block_start(upstream idx=7) → patched idx=0, content_block_delta(upstream idx=7) → patched idx=0, malformed event, content_block_stop(upstream idx=7)]` → 断言客户端可见序列为「前 3 patched + malformed raw + stop raw idx=7」混合时间线；测试要点显式记录这是预期结果，bypass 仅止损不回滚

`patch_non_streaming_response`：
- 顶层 model 字段替换为 fake_model（评审 #5）
- redacted_thinking 块**整块 drop**（v4.5 评审 #4：与 SSE 状态机一致，不再改写为占位 text）
- effective_thinking_enabled=false 时普通 thinking 块整块删除（与 SSE 状态机一致，v4.3 评审 #5）
- effective_thinking_enabled=true 时普通 thinking 块保留
- content 因过滤清空时补 `[{"type":"text","text":"(empty)"}]`（v4.6 评审 #4 与 sanitize 兜底统一）
- patch 后必须调用 `strip_entity_headers_for_rebuilt_body` 移除 content-length（v4.2 评审 #2）
- SSE 嵌套 message.model 不在此函数处理（由 SSE 状态机负责）

`forwarder` 集成：
- ordered_headers 不包含 anthropic-beta（评审 #3）
- ForwardResult.deepseek_context 在 retry / failover success 分支正确填充（评审 #4）

`/v1/models` 伪造：
- DeepSeek Disguise Flash provider → `data` 包含 `claude-sonnet-4-6`（HAIKU/SONNET/OPUS 都配为同一名时去重为 1 条）
- DeepSeek Disguise Pro provider → `data` 同时包含 `claude-opus-4-7`（OPUS）与 `claude-sonnet-4-6`（HAIKU/SONNET），按 ANTHROPIC_MODEL → OPUS → SONNET → HAIKU 顺序去重
- 非 deepseek_anthropic provider 命中 `/v1/models` → 404

**集成验证：**
- Claude Code v2.1.126 连接 DeepSeek Flash 预设：无 400/422，工具调用正常
- 连接 DeepSeek Pro 预设：推理 UI 渲染正常，初次工具调用时 unsafe_tool_followup 自动降级，后续保持 thinking 模式
- 多轮含工具调用对话顺序正确，无孤立 tool_result
- 非流式 `/cost` 调用正常返回伪装模型名
- thinking 历史在多轮中正确过滤
- **opus-4-7 本地白名单实测**：必须在 v2.1.126 上验证客户端不会本地拦截

---

## 文件变更清单

| 文件 | 操作 |
|------|------|
| `src-tauri/src/proxy/providers/deepseek_anthropic/mod.rs` | 新建 |
| `src-tauri/src/proxy/providers/deepseek_anthropic/model_mapping.rs` | 新建 |
| `src-tauri/src/proxy/providers/deepseek_anthropic/request_sanitizer.rs` | 新建 |
| `src-tauri/src/proxy/providers/deepseek_anthropic/tool_repair.rs` | 新建 |
| `src-tauri/src/proxy/providers/deepseek_anthropic/sse_state.rs` | 新建（吸收 free-claude-code 状态机） |
| `src-tauri/src/proxy/providers/deepseek_anthropic/sse_stream.rs` | 新建 |
| `src-tauri/src/proxy/providers/deepseek_anthropic/response_patch.rs` | 新建 |
| `src-tauri/src/proxy/providers/mod.rs` | 修改（声明模块） |
| `src-tauri/src/proxy/providers/claude.rs` | 修改（2 处 match 分支加 deepseek_anthropic；needs_transform 不加） |
| `src-tauri/src/proxy/forwarder.rs` | 修改（mapped_body sanitize + ordered_headers 黑名单 + ForwardResult.deepseek_context + 私有 forward 签名） |
| `src-tauri/src/proxy/response_processor.rs` | 修改（`process_response` 签名加 `deepseek_context: Option<&DeepseekContext>`；handle_streaming/non_streaming 接入 deepseek_context；非流式 patch 后调 `strip_entity_headers_for_rebuilt_body`） |
| `src-tauri/src/proxy/handlers.rs` | 修改（`/v1/models` 伪造按 api_format 分支；`handle_claude_messages` 与其它 4 处 process_response 调用点同步签名） |
| `src/types.ts` | 修改（ProviderMeta.apiFormat 与 ClaudeApiFormat 联合类型加 deepseek_anthropic） |
| `src/config/claudeProviderPresets.ts` | 修改（v4.11 #3：`ProviderPreset.apiFormat` 联合加 deepseek_anthropic；新增 2 个 Claude Disguise 预设，使用 ANTHROPIC_API_KEY） |
| `src/components/providers/forms/ClaudeFormFields.tsx` | 修改（v4.11 #3：API Format 下拉新增第 5 个 SelectItem `deepseek_anthropic`） |
| `src/i18n/locales/zh.json` / `en.json` / `ja.json` | 修改（自动过滤说明 + `providerForm.apiFormatDeepseekAnthropic` key） |

---

## 修订历史

- **v1（2026-05-09）：** 初版，基于参考 PR 的 Node.js 代理设计
- **v2（2026-05-09）：** 参考 free-claude-code + CoDeepSeedeX 加固
- **v3（2026-05-09）：** 针对 9 条评审意见整改：SSE patch 移到 passthrough、预设字段对齐、不强制 stream、模型名 v4-flash/pro、output_config 子字段保留、白名单语义、SSE 事件解析、redacted_thinking 完整重写、3 处 match 分支同步
- **v4（2026-05-09）：** **重大修订**
  - 吸收 free-claude-code 3 项核心设计：unsafe_tool_followup 智能降级 / thinking 字段白名单重建 / SSE 块策略状态机
  - 8 条评审整改：tools 黑名单、ANTHROPIC_API_KEY、mapped_body sanitize、DeepseekContext 透传、非流式顶层 model patch、patch/wrap 职责分离、保守 tool 修复、Flash 显式 disabled
  - 显式 spec /v1/models 伪造路径
  - 子模块按职责拆分

- **v4.1（2026-05-09）：** **7 条评审整改**
  - **tool 顺序修复升级**：保守版漏掉「非相邻但存在」乱序场景，改为 split-and-promote（块级别拆分提升）
  - **Pro 默认启用 thinking**：默认值由 target_model 决定（pro=true / flash=false），客户端显式优先
  - **SSE 状态机接口改 `Vec<String>`**：支持单输入事件输出多事件（合成 stop + 转发改写后 start）
  - **redacted_thinking 始终删**：修正代码示例与文字描述自相矛盾
  - **/v1/models 路由新增**：实测路由表无 `/v1/models`，新增 route + handler，复用既有 desktop handler
  - **output_config 改回白名单**：仅保留 `effort`，避免未知子字段触发 400
  - **reasoning_content 删除策略加 TODO**：实测 Pro + MCP 工具循环降级率，决定是否改为转换为合成 thinking 块

- **v4.2（2026-05-09）：** **6 条评审整改（全部成立）**
  - **#1 SSE wrapper 位置说明**：明确 wrapper 必须放在 `create_logged_passthrough_stream` 之后，使 usage collector / 日志看到 DeepSeek 上游真实事件、客户端看到伪装事件；附完整理由说明
  - **#2 非流式 content-length 重算**：在 `patch_non_streaming_response` 重新序列化后**显式调用 `strip_entity_headers_for_rebuilt_body`**，移除上游 `content-length` / `content-encoding`，避免下游截断
  - **#3 wrap_sse_stream 错误类型对齐**：签名由 `Result<Bytes, ProxyError>` 改为 `Result<Bytes, std::io::Error>`，与 `create_logged_passthrough_stream` 既定类型一致；内部 patch 失败用 `io::Error::new(InvalidData, _)` 映射
  - **#4 /v1/models 非 deepseek 行为**：删除「透传上游」错误措辞（从未存在该路径），改为非 deepseek_anthropic 直接 404，严格等价于路由不存在时的客户端体验
  - **#5 DeepseekContext 字段重命名**：`thinking_enabled` → `effective_thinking_enabled`，与 `SanitizeResult.effective_thinking_enabled` 字段名一致绑定，杜绝命名漂移导致 SSE 端误用客户端原始 thinking
  - **#6 模块树补回 response_patch.rs**：之前在「子模块按职责拆分」段落漏列，已补回

- **v4.3（2026-05-09）：** **8 条评审整改（全部成立）**
  - **#1 tool_repair 描述/测试与 split-and-promote 对齐**：删除「保守版」「含 text 不移动」「多个 tool_result 不拆分」等矛盾措辞，重写测试覆盖列表
  - **#2 output_config 测试反向描述**：测试要点修正为「仅保留 effort，其它子字段一律删除」，与白名单实现一致
  - **#3 DeepseekContext 端到端传递链补完**：补 `process_response` 签名扩展、`handlers.rs:186` 调用点改造、其它 4 处 process_response 传 `None` 的伪代码，避免实现时遗漏导致 sanitize 生效但响应 patch 不生效
  - **#4 tool_repair case (a) 重定义**：从「首块匹配」改为「按 assistant message 分组、紧随 user 前 N 块按 expected_ids 顺序连续匹配」，正确支持「同一 assistant 多 tool_use → 同一 user 多 tool_result」常规形态
  - **#5 patch_non_streaming_response 镜像 SSE 策略**：增加 `effective_thinking_enabled` 参数，thinking 关闭时整块删除（与 SSE 状态机一致），避免流式/非流式响应客户端可见行为不一致
  - **#6 /v1/models 返回所有配置模型**：从仅返回 ANTHROPIC_MODEL 改为返回 ANTHROPIC_MODEL/OPUS/SONNET/HAIKU 去重并集，避免 Claude Code 校验时漏掉其它伪装名
  - **#7 client_intent 严格三态解析**：仅识别 `"enabled"` / `"disabled"`，未知值（含未来新增类型）回退到 target_model 默认值并 warn，不静默降级
  - **#8 patch_sse_event 残留 `Option<String>` 描述**：模块公开 API 注释同步为 `Vec<String>` + `(event, &mut state, fake_model, effective_thinking_enabled)` 完整签名

- **v4.4（2026-05-09）：** **7 条评审整改（全部成立）**
  - **#1 tool_choice 处理新增**：sanitize_request 新增 ⑨ 节，target=pro 时删除 `tool_choice` 避免 reasoner 400/422；target=flash 保留客户端原值；补单测三例
  - **#2 非流式 patch 与 usage 顺序明确**：`handle_non_streaming` 必须**先**基于 upstream JSON 走既有 usage 解析与 `spawn_log_usage`（记录 `deepseek-v4-pro/flash`），**后**调 `patch_non_streaming_response`，与流式语义对齐；附完整 ①②③④⑤⑥ 步骤伪代码
  - **#3 wrap_sse_stream 事件分隔符契约**：明确 `patch_sse_event` 返回 `Vec<String>` 元素**不带** `\n\n`（与切分对称）；`wrap_sse_stream` yield 时**必须** `format!("{}\n\n", e)` 显式补回；多元素各自独立 yield 不合并
  - **#4 重复 tool_result 处理**：步骤 3 引入 `consumed_tool_result_ids` 集合，每个 expected id 仅消费一次（按 (a)/(b) 优先级），后续重复（同 user 内或跨 user）全删；补单测
  - **#5 split-and-promote 后连续 user 合并**：新增步骤 2.5，把 split 后剩余 text/image 块的原 user 与新合成 user(tool_result) 合并为单条 `[tool_results..., 原 text 块...]`，避免连续 user 触发 422
  - **#6 /v1/models provider 选择策略明确**：删除伪函数 `resolve_active_claude_provider`，明确改用 `provider_router.select_providers("claude").first()`，与 `/v1/messages` 实际使用 provider 一致，避免 models 列表与真实消息路径漂移
  - **#7 数据流图 ⑦ 措辞同步**：`保守版 tool 顺序修复` → `split-and-promote tool 顺序修复（按 assistant 分组、按 expected_ids 顺序匹配）`；同时把新增 ⑨ tool_choice / ⑩ stream 编号对齐

- **v4.5（2026-05-09）：** **5 条评审整改（全部成立）**
  - **#1 tool_repair 重复清理逻辑修正**：v4.4 用 `consumed_tool_result_ids: HashSet<String>` 会误删 case (a) 已就位 / case (b) 新合成 user 中的合法 tool_result。改为「block-identity 跟踪 + 仅扫描原始残留块」：步骤 2 给 case (a)/(b)/(c) 输出的所有 tool_result 块标 `accepted`；步骤 3 仅对未 accepted 的残留原始块去重（孤立或 expected id 已被 accepted block 消费 → 删）
  - **#2 连续 user 合并范围限定**：v4.4 与「不合并多个独立 user 消息」字面冲突。改为只合并「步骤 2 新插入的 synthetic user ↔ 配对剩余原 user」一对一关系，由步骤 2 时记录配对指针，避免全局扫描误合并用户独立消息
  - **#3 SSE 错误策略统一**：v4.2 评审 #3 写「patch 失败映射 io::Error(InvalidData)」与测试要点「事件原样透传 + warn」直接矛盾。统一为**永远透传 + warn，永不终止流**；`io::Error` 仅承载上游 IO 错误。补完整故障矩阵
  - **#4 非流式 redacted_thinking 与 SSE 行为对齐**：v4.3 让非流式改写为占位 text，而 SSE 路径整块 drop。统一为**两路径都 drop**，content 清空兜底由空 text 块负责（已存在）；保持流式/非流式客户端可见行为完全一致
  - **#5 /v1/models handler 错误映射对齐**：v4.4 把所有 `select_providers` 错误映射为 `DatabaseError(500)`，与 `RequestContext::new` 行为不一致。补 `AllProvidersCircuitOpen` / `NoProvidersConfigured` / 通用 `DatabaseError` / `NoAvailableProvider` 四分支映射，复用既有 `ProxyError` 语义

- **v4.6（2026-05-09）：** **6 条评审整改（全部成立）**
  - **#1 日志宏统一为 `log::*`**：删除全部 `tracing::warn!` / `tracing::debug!`（项目仅依赖 `log = "0.4"`，无 `tracing` 依赖）；SSE 错误矩阵补「`warn!` 简写 = `log::warn!`」约定，使用 `{}` 格式化而非结构化字段语法
  - **#2 删除 raw pointer 实现建议**：`HashSet<*const Value>` 在 messages 插入/删除/移动时会因 Vec realloc / move 失效，引发 use-after-free。统一为 `_dsk_accepted: true` 临时字段方案（随 Value 一起移动，天然稳定），序列化前在 sanitize_request 末尾统一清理
  - **#3 「迟到合法 tool_result 原地保留」改为删除**：v4.5 步骤 3 留下了「迟到合法 tool_result 保留并标 accepted」的 case，但这种 tool_result 通常未与对应 assistant 相邻 → DeepSeek 仍 422。改为强制视为前置提升 bug → 删除，调试时露馅而非埋雷
  - **#4 空 content 兜底统一**：v4.4 写「`""` 或 text 块」二选一；统一为 `[{"type":"text","text":"(empty)"}]` 单一占位，sanitize_request 与 patch_non_streaming_response 共用同一占位字串以便单测断言
  - **#5 process_response 伪代码与现有签名对齐**：现有签名是 `&UsageParserConfig`（不是 `ParserConfig`）；`handle_streaming` 返回 `Response`（不是 `Result`），所以分发时必须 `Ok(handle_streaming(...).await)` 包装；前几版伪代码漏掉这两点，按那版实现会编译失败
  - **#6 tool_repair.rs 摘要同步**：v4.4 摘要只写「步骤 3 孤立清理」远不能覆盖 v4.4-v4.5-v4.6 累积逻辑（重复清理 / accepted 标记 / 配对合并 / 临时字段清理 / 步骤 1-5 完整流程）；改写为 5 步详细摘要，避免读者按简化版实现漏掉 80% 逻辑

- **v4.7（2026-05-09）：** **5 条评审整改（全部成立）**
  - **#1 tool_repair 索引稳定性**：v4.6 实现仍按初始 `(message_idx, block_idx)` 在步骤 2/2.5/3 中逐步 mutate messages，多 assistant + 跨消息 split 时索引会被前序操作错位（插入位移 / 删除塌陷）。改为 plan-then-apply：步骤 1 snapshot 只读、步骤 2/2.5/3 仅构造 `RepairOp` 列表（含 `SplitAndPromote` / `SynthesizePlaceholder` / `Merge` / `DeleteBlock` 四种）、步骤 4 按 `insert_after_assistant_idx` 倒序应用；同步重写 `tool_repair.rs` 摘要、补 `RepairOp` 枚举签名
  - **#2 SSE bypass_mode**：v4.6「结构性错误原样透传 + state 不更新」会让客户端在同一流内看到混合 raw upstream index 与 patched 下游 index，state 推进不一致最终破坏 message_stop 配平。新增 `bypass: bool` 单向开关，结构性错误（malformed event / JSON parse 失败 / 缺 index/type）一旦触发就置 true，后续所有事件直接原样透传不读不写 state；非结构性错误（仅 model 字段缺失等）保持原 warn 透传不进 bypass。错误矩阵新增 bypass 列、补 `sse_state.rs` 状态机字段说明
  - **#3 步骤 2.5 `paired_remaining_user_idx`**：v4.5/v4.6 写「步骤 2 时记录配对指针」字面易被实现为 `&Value` 引用或裸 idx，跨步骤 mutate 时同样失效。明确为 `RepairOp::SplitAndPromote { paired_remaining_user_idx: Option<usize>, ... }` 中携带的 snapshot 索引，仅在步骤 4 倒序统一应用阶段读取，杜绝跨步骤索引引用
  - **#4 tool_repair 测试要点扩补**：v4.6 测试集没有覆盖 v4.6/v4.7 关键不变量。新增 5 项测试：`_dsk_accepted` 临时字段最终清理；多 assistant 正/逆序处理；结构改写后无连续 user；未 accepted 的迟到合法 id 必须被删除；`apply_plan` 倒序应用与等价正序基线一致
  - **#5 `handle_claude_desktop_models` 分支**：v4.6 仅修改了 `/v1/models` 路由，桌面端独立网关 `/claude-desktop/v1/models` 仍走 `model_list_response`（基于 desktop 配置 `proxy_model_routes`，与 ANTHROPIC_*_MODEL 不一致），客户端会看到对不上的 id。spec 补完整伪代码：在 `validate_claude_desktop_gateway_auth` 之后、`model_list_response` 之前分支 deepseek_anthropic，调用与 `/v1/models` 共用的 `build_deepseek_disguised_models_payload`

- **v4.8（2026-05-09）：** **6 条评审整改（全部成立）**
  - **#1 RepairOp::Merge 位置不可稳定表达**：v4.7 把 split / insert / merge 拆为两条 plan op，但 `synthetic_user_pos` 是 `SplitAndPromote` 应用后才存在的位置，snapshot 阶段无法表达稳定 idx；倒序 apply 也救不回（前序 op 会推走 synthetic_user_pos）。删除 `RepairOp::Merge`；merge 语义并入 `SplitAndPromote.paired_remaining_blocks: Vec<Value>`（snapshot 阶段一次性预计算的剩余 content 副本），apply 时一次 vec 操作完成 split + insert + merge，无跨 op 位置依赖
  - **#2 apply plan 操作顺序不安全**：v4.7「先 block 抽取/清理，再 message 整体插入/删除/合并」让同一 source user 上的多 op 互相干扰（DeleteBlock / SplitAndPromote / Merge 都可能作用于同一 user，先后读取会读到已变更内容）。改为「按 source user 聚合 + snapshot 一次性预计算 final_remaining_blocks」：apply 阶段绝不重读 source user 当前 content，只读 plan snapshot 副本；每个 source user 至多被一次 mutate
  - **#3 SSE bypass_mode 文案过强**：v4.7 写「保证下游看到一致的同一时间线（要么全 patched 要么全 raw）」过强，bypass 只能止损不能回滚已发事件。文案改为「止损后续损坏，不保证整流一致」；测试要点新增「bypass 不回滚已发事件」用例（前 N 事件 patched + malformed + 后续 raw 的混合时间线显式记录为预期）
  - **#4 desktop handler 错误映射退回 DatabaseError**：v4.7 `handle_claude_desktop_models` 仍 `.map_err(|e| ProxyError::DatabaseError(e.to_string()))`，与 `handle_claude_models` 的 4 分支映射不一致。抽出 `select_models_endpoint_provider(state, app_type_str)` helper（4 分支映射 + first()），两条 endpoint 共用，避免语义漂移
  - **#5 非流式 usage 伪代码丢失非 JSON fallback**：v4.7 只展示 `if let Some(ref json)` 分支，但现有 `handle_non_streaming` (response_processor.rs:259-315) 对「JSON 解析失败」与「JSON 解析成功但 usage parser None」均会 `spawn_log_usage(TokenUsage::default())`；若按 v4.7 简化版实现会静默丢失非 JSON 日志。补完整 3 分支伪代码并标注与现有源码行号对应
  - **#6 sanitizer 编号不一致**：数据流图 ⑥ thinking 历史 / ⑦-⑩ 与正文 ⑥ context_management / ⑦-⑩ 错位（thinking 历史过滤实际在正文 ⑤ messages 净化的 sub-step `sanitize_thinking_blocks` 内）。统一为：数据流图 ⑤ messages 净化（含 thinking 历史过滤）/ ⑥ context_management / ⑦ tool repair / ⑧ max_tokens / ⑨ tool_choice / ⑩ stream，与正文 1:1 对齐

- **v4.9（2026-05-10）：** **5 条评审整改（全部成立）**
  - **#1 SynthesizePlaceholder 缺 paired_remaining_blocks**：v4.8 让 placeholder op 仅带 `insert_after_assistant_idx + synthetic_blocks`，case (c) 全缺失 + assistant 后原本紧跟 user(text) 时，插入 placeholder user 后形成 `assistant → user(placeholder) → user(text)` 连续 user，与 v4.4 评审 #5 修复目标矛盾。**修复：** placeholder op 同样 capture `paired_remaining_blocks` + `paired_source_user_idx`，与 SplitAndPromote 走同一原子合并路径
  - **#2 paired_remaining_blocks 在多 op 同 source user 时重复并入**：v4.8 让每个 SplitAndPromote 各自计算 `paired = original \ blocks_to_extract`，但同一 source user 可能被多个 assistant 各自抽走部分 tool_result（合法多 assistant 共享 user 容器场景），逐 op 计算会让 op_A 把 op_B 已抽走的 tool_result 当作 remaining 误并入 synthetic_A。**修复：** 步骤 2.5 改为先按 source user 全局聚合 `extracted_by_user ∪ deleted_by_user` → 一次性计算每个 user 的 `final_remaining_blocks` → 唯一绑定到该 source user 升序第一个 synthetic user；其余 op 的 paired 字段保持空
  - **#3 区间替换误删中间消息**：v4.8 写「`messages[a+1 ..= p]` 范围替换为合成 user」隐式假设 a 和 p 之间只有 paired user 一条；实际多轮对话中 paired user 可远离 assistant，中间存在独立的 user/assistant/system 消息会被无差别覆盖。**修复：** apply 阶段拆为「`insert(a+1, synthetic)`」+「条件 `remove(actual_p)`」两步独立 vec 操作，禁止区间替换；snapshot p_snap → actual_p 的转换仅在每 op 4b 阶段一次性完成，倒序循环保证后续 op 不影响本 op 索引
  - **#4 select_models_endpoint_provider 漏 async**：v4.8 helper 函数体调用 `.await`、调用方按 async 使用，但签名是 `fn`，会编译失败。**修复：** 改为 `async fn`
  - **#5 tool_repair 测试缺 placeholder 配对 + 同 user 多 split 用例**：v4.8 测试覆盖了 `_dsk_accepted` 清理、多 assistant 正逆序、无连续 user、迟到合法 id 删除、plan 倒序等价 5 项，但不覆盖最容易破坏 plan 聚合语义的两类。**修复：** 新增 3 项测试：placeholder + 下游 user(text) 合并；同一 source user 多个 SplitAndPromote 唯一绑定；paired user 远离 assistant 不可误删中间消息

- **v4.10（2026-05-10）：** **5 条评审整改（全部成立）**
  - **#1 snapshot → actual idx 局部推算未考虑前序倒序 op**：v4.9 步骤 4b 写「`p_snap` 若 ≥ insert_after_assistant_idx + 1，则当前实际位置为 `p_snap + 1`」——只考虑当前 op 自身 4a 的插入，没考虑前序倒序 op（即 `insert_after_assistant_idx` 较大、已先执行的 op）的 insert 与 remove paired 副作用：前序 op 的插入位置或被删 paired 位置可能出现在当前 op 的 paired_source_user_idx 之前（snapshot 维度），整体推动当前 paired_source_user_idx 的实际位置。**修复：** apply 阶段维护 `snapshot_to_current: HashMap<usize, usize>`，每次 `messages.insert(pos)` / `messages.remove(pos)` 同步更新（≥pos 加 1 / >pos 减 1，被移除的 snapshot 键删除）；所有 op 内的 snapshot 索引（insert_after_assistant_idx、paired_source_user_idx、DeleteBlock.user_idx）统一通过该 map 查实际位置
  - **#2 步骤 2.5 在 deleted_by_user 完整前执行**：v4.9 把残留扫描放在步骤 2.5 之后（step 2 → 2.5 → 3），但 2.5 计算 `final_remaining = original \ extracted ∪ deleted` 需要 `deleted_by_user`，而 DeleteBlock 是步骤 3 产出的——按 v4.9 顺序，2.5 看到的 deleted_by_user 是空集，迟到合法 id 的孤立 tool_result 会被并入 paired_remaining_blocks 重塞回 synthetic user，到步骤 3 已不在「未 accepted 的源残留」里。**修复：** 步骤顺序调整为 1 → 2 → 3 → 2.5 → 4，确保 deleted_by_user 在聚合阶段已就绪
  - **#3 final_remaining 升序第一个绑定破坏时间线**：v4.9 把 `final_remaining_blocks` 唯一绑定到「该 source user 升序第一个」 synthetic user，但同一 source user 被多个 assistant 共享时，残余 text/image 应紧贴最后一次消费该 user 的 assistant 下游，绑到第一个会把 text 前移到时间线之前。**修复：** 改为「升序最后一个」 synthetic user；并补充「多 assistant + case (a) tool_result 保留 + case (b) 整体 remove paired」的语义冲突保守降级：把 case (a) 的 tool_result 也加入 deleted_by_user 走 DeleteBlock 路径
  - **#4 SynthesizePlaceholder 缺 candidate_source_user_idx**：v4.9 placeholder op 带 `paired_source_user_idx` 但缺「该 placeholder 来自哪个 source user 的 case (c)」候选信息，步骤 2.5 无法精准把 placeholder 算入「该 source user 升序最后一个」聚合分类，会导致 placeholder 与 SplitAndPromote 共存时绑定决策错位。**修复：** SynthesizePlaceholder 增加 `candidate_source_user_idx: Option<usize>` 字段（步骤 2 case (c) 判定时记录原 user idx），仅参与步骤 2.5 聚合分类，apply 阶段忽略
  - **#5 测试描述误写为「步骤 1 marks accepted」**：v4.9 测试要点写「步骤 1 snapshot 扫描时给已就位的 tool_result 标 `_dsk_accepted`」，但 spec 正文步骤 1 是只读，标记发生在步骤 2。**修复：** 测试描述改为「步骤 2 在 case (a)/(b)/(c) 确认保留时打 `_dsk_accepted`」
