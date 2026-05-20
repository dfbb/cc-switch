# Task 08: Request Extension Group 4 — messages-cache-breakpoint, ttl-management, prefix-diff

**可并行**: 是 — 与 Task 05-07, 09-10 全部并行

**依赖**: Task 01（traits/types）

**范围**: 3 个 extension（order 410-680），均只实现 `RequestExtension`

## 文件

- Create: `src-tauri/src/proxy/extensions/messages_cache_breakpoint.rs`
- Create: `src-tauri/src/proxy/extensions/ttl_management.rs`
- Create: `src-tauri/src/proxy/extensions/prefix_diff.rs`
- Modify: `src-tauri/src/proxy/extensions/load.rs`
- Modify: `src-tauri/src/proxy/extensions/mod.rs`

**JS 源码参考**: `/Users/dfbb/Sites/myidea/ccswitch/3rd/claude-code-cache-fix/proxy/extensions/`

---

### Step 1: 翻译 messages_cache_breakpoint (order 410, default_enabled=true)

**JS 源文件**: `messages-cache-breakpoint.mjs`

在 CC 自动注入的 blocks（hooks/skills/CLAUDE.md/deferred-tools/MCP）与第一条真实用户内容之间，注入缺失的 breakpoint #3 cache_control 标记。env var `CACHE_FIX_INJECT_MESSAGES_BREAKPOINT=1` 激活。仅在现有标记 1-3 时注入（不超过 Anthropic 的 4 上限）。

### Step 2: 翻译 ttl_management (order 500, default_enabled=true)

**JS 源文件**: `ttl-management.mjs`

根据 `ctx.meta["_ttlTier"]`（由 ttl-tier-detect 写入）注入正确的 TTL cache_control。区分主线程与 subagent（检测 "Claude Agent SDK" 系统 prompt）。env var `CACHE_FIX_TTL_MAIN`/`CACHE_FIX_TTL_SUBAGENT` 可配置（默认 "1h"）。

### Step 3: 翻译 prefix_diff (order 680, default_enabled=true)

**JS 源文件**: `prefix-diff.mjs`

每次请求时快照 system+tools+前 5 条消息到 `~/.claude/cache-fix-snapshots/<key>-last.json`。与上一次快照差异时写入 diff 文件和 stderr。env var `CACHE_FIX_PREFIXDIFF=1` 激活。

### Step 4: 在 load.rs 中注册

```rust
registry.request_exts.push(Box::new(messages_cache_breakpoint::MessagesCacheBreakpoint::new()));
registry.request_exts.push(Box::new(ttl_management::TtlManagement::new()));
registry.request_exts.push(Box::new(prefix_diff::PrefixDiff::new()));
```

### Step 5: 在 mod.rs 中声明模块

```rust
pub mod messages_cache_breakpoint;
pub mod ttl_management;
pub mod prefix_diff;
```

### Step 6: 编译 + 提交

```bash
cd src-tauri && cargo check 2>&1
git add src-tauri/src/proxy/extensions/
git commit -m "feat(extensions): add request group4 — messages-cache-breakpoint, ttl-management, prefix-diff"
```
