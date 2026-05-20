# Task 07: Request Extension Group 3 — tool-input-normalize, microcompact-stability, deferred-tools-restore, thinking-display, cache-control-normalize

**可并行**: 是 — 与 Task 05-06, 08-10 全部并行

**依赖**: Task 01（traits/types）

**范围**: 5 个 extension（order 340-400），均只实现 `RequestExtension`

## 文件

- Create: `src-tauri/src/proxy/extensions/tool_input_normalize.rs`
- Create: `src-tauri/src/proxy/extensions/microcompact_stability.rs`
- Create: `src-tauri/src/proxy/extensions/deferred_tools_restore.rs`
- Create: `src-tauri/src/proxy/extensions/thinking_display.rs`
- Create: `src-tauri/src/proxy/extensions/cache_control_normalize.rs`
- Modify: `src-tauri/src/proxy/extensions/load.rs`
- Modify: `src-tauri/src/proxy/extensions/mod.rs`

**JS 源码参考**: `/Users/dfbb/Sites/myidea/ccswitch/3rd/claude-code-cache-fix/proxy/extensions/`

---

### Step 1: 翻译 tool_input_normalize (order 340, default_enabled=true)

**JS 源文件**: `tool-input-normalize.mjs`

按 schema 定义排序 `tool_use.input` 字段，删除多余字段，使 tool_use 输入确定性。

### Step 2: 翻译 microcompact_stability (order 350, default_enabled=true)

**JS 源文件**: `microcompact-stability.mjs`

检测和标准化 `time_based_microcompact` sentinel。Mode A（精确匹配，可标准化）和 Mode B（前缀匹配，仅诊断）。env var `CACHE_FIX_NORMALIZE_MICROCOMPACT=1` 激活。

### Step 3: 翻译 deferred_tools_restore (order 350, default_enabled=true)

**JS 源文件**: `deferred-tools-restore.mjs`

跨会话持久化和恢复 deferred-tools attachment block。AVAILABLE 时保存 clean snapshot，UNAVAILABLE 时从快照恢复。按项目 cwd 建 key。

### Step 4: 翻译 thinking_display (order 360, default_enabled=true)

**JS 源文件**: `thinking-display.mjs`

对 Opus 4.7 请求注入 `thinking.display`。env var `CACHE_FIX_THINKING_DISPLAY` 控制模式（默认 "summarized"）。

### Step 5: 翻译 cache_control_normalize (order 400, default_enabled=true)

**JS 源文件**: `cache-control-normalize.mjs`

从用户消息中剥离分散的 cache_control 标记，在最后一条用户消息的最后一个 block 上应用单一正则位置 `cache_control: { type: "ephemeral" }`。

### Step 6: 在 load.rs 中注册

```rust
registry.request_exts.push(Box::new(tool_input_normalize::ToolInputNormalize::new()));
registry.request_exts.push(Box::new(microcompact_stability::MicrocompactStability::new()));
registry.request_exts.push(Box::new(deferred_tools_restore::DeferredToolsRestore::new()));
registry.request_exts.push(Box::new(thinking_display::ThinkingDisplay::new()));
registry.request_exts.push(Box::new(cache_control_normalize::CacheControlNormalize::new()));
```

### Step 7: 在 mod.rs 中声明模块

```rust
pub mod tool_input_normalize;
pub mod microcompact_stability;
pub mod deferred_tools_restore;
pub mod thinking_display;
pub mod cache_control_normalize;
```

### Step 8: 编译 + 提交

```bash
cd src-tauri && cargo check 2>&1
git add src-tauri/src/proxy/extensions/
git commit -m "feat(extensions): add request group3 — tool-input-normalize, microcompact-stability, deferred-tools-restore, thinking-display, cache-control-normalize"
```
