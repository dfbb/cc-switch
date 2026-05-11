# DeepSeek Disguise 预设合并 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 把 `claudeProviderPresets.ts` 中两个独立 disguise 预设（Flash / Pro）合并为单个 `DeepSeek (Claude Disguise)`，默认 opus→Pro、sonnet/haiku→Flash。

**Architecture:** 纯前端预设数据替换，无后端改动。表单已支持四个模型字段独立编辑，路由完全由用户填入的 Claude 模型名决定，后端 `deepseek_anthropic` 适配器原封不动。

**Tech Stack:** TypeScript, React (cc-switch 前端) + Tauri 2 (代码层不变，仅校验)

**Spec:** `docs/superpowers/specs/2026-05-11-deepseek-disguise-preset-merge.md`

---

## 任务列表

| Task | 文件 | 可并行 | 依赖 |
|---|---|---|---|
| [T01](./task-01-replace-preset.md) | 替换 `claudeProviderPresets.ts` 中两条预设为单条 | — | — |
| [T02](./task-02-verify-types.md) | TypeScript 编译校验 + 全局引用扫描 | 否 | T01 |
| [T03](./task-03-manual-smoke-test.md) | 手工烟雾测试（启动应用并验证表单与 curl） | 否 | T02 |

**说明：**

- 本次改动只涉及一个源文件 + 一份用户文档，任务链短，**T01 后只能串行**（编译与运行依赖前一步的产物）。
- 没有可并行的子任务（不存在多个独立文件需要改动）。
- T03 是手工验证，由人执行；agentic worker 可在 T03 前停下来交接。

---

## 验收标准

- [ ] `claudeProviderPresets.ts` 中只有一条 disguise 预设，名为 `DeepSeek (Claude Disguise)`
- [ ] `npm run build` 通过、`npx tsc --noEmit` 通过
- [ ] 代码库内没有任何对旧名 `DeepSeek (Claude Disguise · Flash)` / `DeepSeek (Claude Disguise · Pro)` 的引用
- [ ] 启动应用后，添加 Claude Provider 时预设列表里出现新名称、且默认值符合 spec
- [ ] curl 验证伪装模型名透传 + DeepSeek 上游返回正确
