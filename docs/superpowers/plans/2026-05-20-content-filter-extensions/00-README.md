# 内容过滤扩展管道 — 实施计划总览

**Spec**: `docs/superpowers/specs/2026-05-19-content-filter-extension-pipeline-design.md`

## 依赖关系图

```
Phase 0 — 基础设施（顺序执行）
  01-traits-and-types.md        ← 无依赖
    ↓
  02-registry.md                ← 依赖 01
    ↓
  03-provider-config.md         ← 依赖 01（可与 02 并行）
  04-proxy-wiring.md            ← 依赖 01, 02

Phase 1 — Extension 实现（全部可与 Phase 0 后续任务并行）
  05-14: 15 个 Request-only extensions  ← 每个依赖 01，彼此可完全并行
  15-20: 8 个 Multi-hook extensions     ← 每个依赖 01，彼此可完全并行

Phase 2 — 集成（顺序）
  21-forwarder-integration.md   ← 依赖 01, 02
  22-response-integration.md    ← 依赖 01, 02
  23-frontend-ui.md             ← 依赖 03（可与 21, 22 并行）

Phase 3 — 测试
  24-integration-tests.md       ← 依赖全部完成
```

## 并行执行策略

| 阶段 | 任务 | 可并行？ |
|------|------|---------|
| Phase 0 | 01 | 否 — 所有 extension 的基础 |
| Phase 0 | 02 | 否 — 依赖 01 |
| Phase 0 | 03 | **是** — 与 02 并行 |
| Phase 0 | 04 | 否 — 依赖 02 |
| Phase 1 | 05-14 | **是** — 15 个文件可同时开工 |
| Phase 1 | 15-20 | **是** — 8 个文件可同时开工 |
| Phase 2 | 21, 22 | 否 — 依赖 02 |
| Phase 2 | 23 | **是** — 与 21, 22 并行 |
| Phase 3 | 24 | 否 — 依赖全部 |

## 文件清单

| # | 文件 | 内容 | 可并行 |
|---|------|------|--------|
| 01 | `01-traits-and-types.md` | traits.rs, context.rs, errors.rs, mod.rs 骨架 | 否 |
| 02 | `02-registry.md` | ExtensionRegistry — 加载、排序、管道执行 | 否 |
| 03 | `03-provider-config.md` | ExtensionFilterConfig, config.json, ProviderMeta 扩展 | 是 |
| 04 | `04-proxy-wiring.md` | ProxyState 集成 registry, server.rs 启动加载 | 否 |
| 05 | `05-ext-request-group1.md` | ttl-tier-detect, upstream-change-detection, output-efficiency-rewrite, fingerprint-strip, image-strip (5 个 order 50-150) | 是 |
| 06 | `06-ext-request-group2.md` | sort-stabilization, fresh-session-sort, identity-normalization, smoosh-split, content-strip (5 个 order 200-330) | 是 |
| 07 | `07-ext-request-group3.md` | tool-input-normalize, microcompact-stability, deferred-tools-restore, thinking-display, cache-control-normalize (5 个 order 340-400) | 是 |
| 08 | `08-ext-request-group4.md` | messages-cache-breakpoint, ttl-management, prefix-diff (3 个 order 410-680) | 是 |
| 09 | `09-ext-multi-hook-group1.md` | cache-telemetry (Request + ResponseStart + Stream) | 是 |
| 10 | `10-ext-multi-hook-group2.md` | overage-warning (ResponseStart + Stream), rate-limit-log (Request + Response), usage-log (Stream), request-log (Request + ResponseStart + Stream) | 是 |
| 11 | `11-forwarder-integration.md` | forwarder.rs 请求管道嵌入 | 否 |
| 12 | `12-response-integration.md` | response_processor.rs + handlers.rs 响应管道嵌入 | 否 |
| 13 | `13-frontend-ui.md` | ClaudeFormFields 内容过滤面板, types.ts | 是 |
| 14 | `14-integration-tests.md` | 端到端测试 + extension 单元测试 | 否 |

共计 **14 个任务文件**，其中 6 个标注为可并行。
