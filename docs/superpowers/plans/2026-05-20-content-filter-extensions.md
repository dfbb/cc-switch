# 内容过滤扩展管道 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 CC Switch Rust 代理中实现通用内容过滤扩展管道，位于 DeepSeek（Claude disguise）适配层之前

**Architecture:** 23 个 extension 按三 trait 分离（Request/Response/Stream），通过 ExtensionRegistry 统一加载和排序。请求管道嵌入 forwarder per-attempt 循环，响应/流管道嵌入 response_processor。前端通过 Claude endpoint 表单面板控制每个 extension 的独立开关。

**Tech Stack:** Rust (tokio, serde_json, axum), React/TypeScript (shadcn/ui)

**任务文件目录**: `docs/superpowers/plans/2026-05-20-content-filter-extensions/`

---

## 任务索引

| # | 文件 | 可并行 | 描述 |
|---|------|--------|------|
| 01 | `01-traits-and-types.md` | 否 | traits.rs, context.rs, errors.rs, mod.rs 骨架 |
| 02 | `02-registry.md` | 否 | ExtensionRegistry + config.json + load |
| 03 | `03-provider-config.md` | 是 | ExtensionFilterConfig, ProviderMeta 扩展 |
| 04 | `04-proxy-wiring.md` | 否 | ProxyState 集成 registry |
| 05 | `05-ext-request-group1.md` | 是 | 5 ext: ttl-tier-detect, upstream-change-detection, output-efficiency-rewrite, fingerprint-strip, image-strip |
| 06 | `06-ext-request-group2.md` | 是 | 5 ext: sort-stabilization, fresh-session-sort, identity-normalization, smoosh-split, content-strip |
| 07 | `07-ext-request-group3.md` | 是 | 5 ext: tool-input-normalize, microcompact-stability, deferred-tools-restore, thinking-display, cache-control-normalize |
| 08 | `08-ext-request-group4.md` | 是 | 3 ext: messages-cache-breakpoint, ttl-management, prefix-diff |
| 09 | `09-ext-multi-hook-group1.md` | 是 | 1 ext: cache-telemetry (Request+ResponseStart+Stream) |
| 10 | `10-ext-multi-hook-group2.md` | 是 | 4 ext: overage-warning, rate-limit-log, usage-log, request-log |
| 11 | `11-forwarder-integration.md` | 否 | Request pipeline in forwarder.rs |
| 12 | `12-response-integration.md` | 否 | Response pipeline in response_processor.rs + handlers.rs |
| 13 | `13-frontend-ui.md` | 是 | ContentFilterPanel UI + types |
| 14 | `14-integration-tests.md` | 否 | Integration tests + extension unit tests |

共计 **14 个任务**，其中 **6 个可并行执行**。
