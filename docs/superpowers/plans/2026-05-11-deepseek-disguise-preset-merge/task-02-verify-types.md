# T02 — TypeScript 编译校验

**可并行：** 否
**依赖：** T01

**Files:**
- Verify: 整个 `src/` 目录（不修改文件）

## 目标

确认 T01 改动没有破坏 TypeScript 类型与构建。

## 步骤

- [ ] **Step 1：TS 类型检查**

  ```bash
  npx tsc --noEmit
  ```

  期望：退出码 0，无错误。

- [ ] **Step 2：前端 build**

  ```bash
  npm run build
  ```

  期望：构建成功，dist/ 目录正常生成。

- [ ] **Step 3：再次全局搜索旧名**

  ```bash
  rg "Claude Disguise · Flash|Claude Disguise · Pro" .
  ```

  期望：仅匹配 `docs/superpowers/specs/` 下旧 spec（v4.9 设计文档中的历史名称是允许的）。源码、i18n、测试均不应再有匹配。

## 验收

- tsc 与 build 均通过
- 源码与运行时数据中无旧名残留
