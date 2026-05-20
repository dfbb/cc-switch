# Task 09: Multi-Hook Extension Group 1 — cache-telemetry

**可并行**: 是 — 与 Task 05-08, 10 全部并行

**依赖**: Task 01（traits/types）

**范围**: 1 个 extension，实现 Request + Response + Stream 三个 trait

## 文件

- Create: `src-tauri/src/proxy/extensions/cache_telemetry.rs`
- Modify: `src-tauri/src/proxy/extensions/load.rs`
- Modify: `src-tauri/src/proxy/extensions/mod.rs`

**JS 源码参考**: `/Users/dfbb/Sites/myidea/ccswitch/3rd/claude-code-cache-fix/proxy/extensions/cache-telemetry.mjs`

---

### Step 1: 翻译 cache_telemetry (order 600, default_enabled=true)

**JS 源文件**: `cache-telemetry.mjs`

三阶段 extension：

| 阶段 | Hook | 操作 |
|------|------|------|
| Request | `on_request` | 从请求提取 session ID，写入 `ctx.meta["_sessionId"]` |
| ResponseStart | `on_response_start` | 从 `upstream_headers` 提取 quota 数据，写入 `ctx.meta["_quotaData"]` |
| Stream | `on_stream_event` | 从 `message_start`/`message_delta` SSE 事件提取缓存命中率，持久化到 `~/.claude/quota-status/sessions/<id>.json` |

环境变量:
- `CACHE_FIX_QUOTA_SWEEP_DAYS`: 过期 session 文件清理天数（默认 7）

### Step 2: 文件结构

```rust
// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/cache-telemetry.mjs
// 翻译: 2026-05-20

use super::traits::*;
use super::context::*;
use super::errors::ExtensionError;

pub struct CacheTelemetry;

impl CacheTelemetry {
    pub fn new() -> Self { Self }
}

impl Extension for CacheTelemetry {
    fn name(&self) -> &str { "cache-telemetry" }
    fn order(&self) -> u32 { 600 }
    fn default_enabled(&self) -> bool { true }
}

impl RequestExtension for CacheTelemetry {
    fn on_request(&self, ctx: &mut RequestContext)
        -> Result<Option<(u16, Vec<u8>)>, ExtensionError>
    {
        // 从请求体提取 session ID → ctx.meta["_sessionId"]
        Ok(None)
    }
}

impl ResponseExtension for CacheTelemetry {
    fn on_response_start(&self, ctx: &mut ResponseStartContext)
        -> Result<(), ExtensionError>
    {
        // 从 upstream_headers 提取 quota 数据 → ctx.meta["_quotaData"]
        Ok(())
    }

    fn on_response(&self, _ctx: &mut ResponseContext)
        -> Result<(), ExtensionError>
    {
        // 不使用（仅流式路径需要）
        Ok(())
    }
}

impl StreamExtension for CacheTelemetry {
    fn on_stream_event(&self, ctx: &mut StreamEventContext)
        -> Result<(), ExtensionError>
    {
        // 从 message_start/message_delta 提取缓存统计 → 持久化
        Ok(())
    }
}
```

### Step 3: 在 load.rs 中注册

```rust
let cache_telemetry = Box::new(cache_telemetry::CacheTelemetry::new());
registry.request_exts.push(Box::new(cache_telemetry::CacheTelemetry::new()));
registry.response_exts.push(Box::new(cache_telemetry::CacheTelemetry::new()));
registry.stream_exts.push(Box::new(cache_telemetry::CacheTelemetry::new()));
```

> 注意：同一 extension 需多次构造（三个 trait object 各自独立）。跨阶段状态通过 ctx.meta 传递。

### Step 4: 在 mod.rs 中声明模块

```rust
pub mod cache_telemetry;
```

### Step 5: 编译 + 提交

```bash
cd src-tauri && cargo check 2>&1
git add src-tauri/src/proxy/extensions/
git commit -m "feat(extensions): add cache-telemetry (Request + ResponseStart + Stream)"
```
