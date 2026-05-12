# DeepSeek Anthropic 兼容代理层实现计划（主索引）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 cc-switch 中新增 `deepseek_anthropic` api_format，将 Claude Code 的 Anthropic Messages API 请求通过代理层翻译并转发至 DeepSeek `/anthropic` 端点，同时支持 SSE 流式响应的模型名伪装与 thinking 块过滤。

**Architecture:** 新增 `src-tauri/src/proxy/providers/deepseek_anthropic/` 模块，在 `forwarder.rs` 的 mapped_body 上做 sanitize，在 `response_processor.rs` 的流式路径插入 SSE wrapper，在 `handlers.rs` 新增 `/v1/models` 路由。前端新增 2 个 preset 和 1 个下拉选项。

**Tech Stack:** Rust (serde_json, tokio, axum, log 0.4)、TypeScript (React, shadcn/ui, i18n)

**Spec 参考：** `docs/superpowers/specs/2026-05-09-deepseek-anthropic-proxy-design.md`

---

## 并行关系图

```
T01 (模块骨架)
  ├─ T02 (model_mapping)          ─────────┐
  ├─ T03 (response_patch)         ──────────┤
  ├─ T04a (strip_unsupported)     ──────┐   │
  ├─ T04b (thinking_blocks)       ──────┤   │
  ├─ T04c (normalize_tool_result) ──────┤   │
  ├─ T04d (context_management)    ──┐   │   │
  ├─ T06a (tools_blacklist)       ──┤   │   │
  ├─ T06b (sanitize_tool_choice)  ──┤   │   │
  ├─ T06c (output_config+tokens)  ──┤   │   │
  ├─ T08  (tool_repair 步骤1+2)   ──────────────┐
  ├─ T12  (sse_state.rs)          ──────────────┤
  └─ T17  (前端)                  ──────────────┤ (完全独立)
       │                           │   │   │   │
       ▼                           │   │   │   │
      T02 → T05 (thinking_rebuild)─┘   │   │   │
                                   │   │   │   │
      T04a/b/c/d + T05 + T06a/b/c ─┴───┴───┘   │
                         ↓                      │
                    T07 (sanitize_request)       │
                         │                      │
                         │   T08 → T09 → T10 → T11 (tool_repair)
                         │                      │
                         │   T12 → T13 (sse_stream)
                         │                      │
                    T03, T07, T11, T13 → T14 (后端集成: claude.rs + forwarder.rs)
                                              │
                              ┌───────────────┤
                              ↓               ↓
                         T15 (response_processor)  T16 (/v1/models handler)
                              │               │
                              └───────────────┤
                                             ↓
                              T17 (前端) ───→ T18 (集成验证)
```

---

## 任务列表

| 任务 | 文件 | 可并行 | 前置 |
|------|------|--------|------|
| [T01](task-01-module-scaffold.md) | 新建空模块骨架 | 首先执行 | — |
| [T02](task-02-model-mapping.md) | model_mapping.rs | ✅ 与 T03/T04*/T06*/T08/T12/T17 并行 | T01 |
| [T03](task-03-response-patch.md) | response_patch.rs | ✅ 与 T02/T04*/T06*/T08/T12/T17 并行 | T01 |
| [T04a](task-04a-strip-unsupported.md) | strip_unsupported_attachments | ✅ 与 T02/T03/T04b/c/d/T06*/T08/T12/T17 并行 | T01 |
| [T04b](task-04b-thinking-blocks.md) | sanitize_thinking_blocks + strip_reasoning_content | ✅ 同上 | T01 |
| [T04c](task-04c-normalize-tool-result.md) | normalize_tool_result_content | ✅ 同上 | T01 |
| [T04d](task-04d-context-management.md) | filter_context_management_edits | ✅ 同上 | T01 |
| [T05](task-05-thinking-rebuild.md) | thinking 字段重建 + unsafe_tool_followup | ⚠️ 需等 T02 | T01 + T02 |
| [T06a](task-06a-tools-blacklist.md) | tools 黑名单过滤 | ✅ 与 T02/T03/T04*/T08/T12/T17 并行 | T01 |
| [T06b](task-06b-sanitize-tool-choice.md) | sanitize_tool_choice | ✅ 同上 | T01 |
| [T06c](task-06c-output-config.md) | output_config 白名单 + max_tokens 兜底 | ✅ 同上 | T01 |
| [T07](task-07-sanitize-request.md) | sanitize_request 编排 + SanitizeResult | ⚠️ 需等全部 T04*/T05/T06* | T04a+b+c+d + T05 + T06a+b+c |
| [T08](task-08-tool-repair-plan.md) | tool_repair 步骤1(snapshot)+步骤2(plan构造) | ✅ 与 T02/T03/T04*/T06*/T12/T17 并行 | T01 |
| [T09](task-09-tool-repair-cleanup.md) | tool_repair 步骤3(残留清理+_dsk_accepted) | ⚠️ 需等 T08 | T08 |
| [T10](task-10-tool-repair-aggregate.md) | tool_repair 步骤2.5(paired聚合+唯一绑定) | ⚠️ 需等 T09 | T09 |
| [T11](task-11-tool-repair-apply.md) | tool_repair 步骤4(三阶段apply+snapshot_to_current) | ⚠️ 需等 T10 | T10 |
| [T12](task-12-sse-state.md) | sse_state.rs 状态机 | ✅ 与 T02-T10/T17 并行 | T01 |
| [T13](task-13-sse-stream.md) | sse_stream.rs wrap_sse_stream | ⚠️ 需等 T12 | T12 |
| [T14](task-14-backend-integration.md) | claude.rs + providers/mod.rs + forwarder.rs 集成 | ⚠️ 需等 T03/T07/T11/T13 | T03 + T07 + T11 + T13 |
| [T15](task-15-response-processor.md) | response_processor.rs 签名扩展 + 流路径接入 | ⚠️ 需等 T14 | T14 |
| [T16](task-16-models-handler.md) | handlers.rs /v1/models 路由 + handler | ⚠️ 需等 T14 | T14 |
| [T17](task-17-frontend.md) | types.ts + claudeProviderPresets.ts + ClaudeFormFields.tsx + i18n | ✅ 完全独立，可最早并行执行 | T01（仅需目录存在） |
| [T18](task-18-integration.md) | 集成验证 + 端到端冒烟测试 | ⚠️ 需等全部后端+前端 | T15 + T16 + T17 |

---

## 推荐执行批次

**批次 0（串行）：** T01  
**批次 1（并行）：** T02 + T03 + T04a + T04b + T04c + T04d + T06a + T06b + T06c + T08 + T12 + T17  
**批次 2（串行 T05，待 T02）；并行 T09（待 T08）；并行 T13（待 T12）**  
**批次 3（并行）：** T07（待 T04*/T05/T06*）+ T10（待 T09）  
**批次 4（串行）：** T11（待 T10）  
**批次 5（串行）：** T14（待 T03/T07/T11/T13）  
**批次 6（并行）：** T15 + T16  
**批次 7（串行）：** T18  
