# Task 06: Request Extension Group 2 — sort-stabilization, fresh-session-sort, identity-normalization, smoosh-split, content-strip

**可并行**: 是 — 与 Task 05, 07-10 全部并行

**依赖**: Task 01（traits/types）

**范围**: 5 个 extension（order 200-330），均只实现 `RequestExtension`

## 文件

- Create: `src-tauri/src/proxy/extensions/sort_stabilization.rs`
- Create: `src-tauri/src/proxy/extensions/fresh_session_sort.rs`
- Create: `src-tauri/src/proxy/extensions/identity_normalization.rs`
- Create: `src-tauri/src/proxy/extensions/smoosh_split.rs`
- Create: `src-tauri/src/proxy/extensions/content_strip.rs`
- Modify: `src-tauri/src/proxy/extensions/load.rs`
- Modify: `src-tauri/src/proxy/extensions/mod.rs`

**JS 源码参考**: `/Users/dfbb/Sites/myidea/ccswitch/3rd/claude-code-cache-fix/proxy/extensions/`

---

### Step 1: 翻译 sort_stabilization (order 200)

**JS 源文件**: `sort-stabilization.mjs`

对 skills、deferred-tools、tool definitions 进行字母序排列。排序 user-invocable skills block、deferred tools block、`body.tools` 数组。

### Step 2: 翻译 fresh_session_sort (order 250)

**JS 源文件**: `fresh-session-sort.mjs`

新会话时将 hooks、skills、deferred-tools、MCP 描述等 scattered blocks 重排到 `messages[0]`，按 deferred → mcp → skills → hooks 顺序。

### Step 3: 翻译 identity_normalization (order 300)

**JS 源文件**: `identity-normalization.mjs`

规范化易变的身份标识字段：`SessionStart:resume` → `SessionStart:startup`、移除 `<session-id>` 标签、删除 `Last active:` 行、固定 system_reminder 块。

### Step 4: 翻译 smoosh_split (order 320)

**JS 源文件**: `smoosh-split.mjs`

将粘合在 `tool_result.content` 中的系统提醒分离为独立的文本块。

### Step 5: 翻译 content_strip (order 330)

**JS 源文件**: `content-strip.mjs`

从用户消息中移除 "Continue from where you left off." 文本块和 token/预算/剩余轮次等记账提醒。

### Step 6: 在 load.rs 中注册

```rust
registry.request_exts.push(Box::new(sort_stabilization::SortStabilization::new()));
registry.request_exts.push(Box::new(fresh_session_sort::FreshSessionSort::new()));
registry.request_exts.push(Box::new(identity_normalization::IdentityNormalization::new()));
registry.request_exts.push(Box::new(smoosh_split::SmooshSplit::new()));
registry.request_exts.push(Box::new(content_strip::ContentStrip::new()));
```

### Step 7: 在 mod.rs 中声明模块

```rust
pub mod sort_stabilization;
pub mod fresh_session_sort;
pub mod identity_normalization;
pub mod smoosh_split;
pub mod content_strip;
```

### Step 8: 编译 + 提交

```bash
cd src-tauri && cargo check 2>&1
git add src-tauri/src/proxy/extensions/
git commit -m "feat(extensions): add request group2 — sort-stabilization, fresh-session-sort, identity-normalization, smoosh-split, content-strip"
```
