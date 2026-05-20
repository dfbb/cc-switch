# Task 10: Multi-Hook Extension Group 2 — overage-warning, rate-limit-log, usage-log, request-log

**可并行**: 是 — 与 Task 05-09 全部并行

**依赖**: Task 01（traits/types）

**范围**: 4 个 extension，均实现多 trait

## 文件

- Create: `src-tauri/src/proxy/extensions/overage_warning.rs`
- Create: `src-tauri/src/proxy/extensions/rate_limit_log.rs`
- Create: `src-tauri/src/proxy/extensions/usage_log.rs`
- Create: `src-tauri/src/proxy/extensions/request_log.rs`
- Modify: `src-tauri/src/proxy/extensions/load.rs`
- Modify: `src-tauri/src/proxy/extensions/mod.rs`

**JS 源码参考**: `/Users/dfbb/Sites/myidea/ccswitch/3rd/claude-code-cache-fix/proxy/extensions/`

---

### Step 1: 翻译 overage_warning (order 610, default_enabled=true)

**JS 源文件**: `overage-warning.mjs`

| Hook | 操作 |
|------|------|
| ResponseStart | 从 upstream_headers 检测超量阈值，写入触发状态到 `ctx.meta["_overageTriggered"]` |
| Stream | 在 message_delta 事件中写入 stderr 警告 + `~/.claude/overage-warnings.jsonl` |

### Step 2: 翻译 rate_limit_log (order 660, default_enabled=false)

**JS 源文件**: `rate-limit-log.mjs`

| Hook | 操作 |
|------|------|
| Request | 从请求体提取 model 和请求路径 |
| Response | 从响应 body 判断 `error.type == "rate_limit_error"`，记录到 `~/.claude/usage-log/rate-limit-events.jsonl` |

### Step 3: 翻译 usage_log (order 650, default_enabled=false)

**JS 源文件**: `usage-log.mjs`

| Hook | 操作 |
|------|------|
| Stream | 从 SSE 流事件提取 token 用量（model, input/output/cache tokens, quota utilization），写入 `~/.claude/usage.jsonl`（MeterRowSchema v:1） |

需要 `ctx.data`（SSE 事件 JSON）、`ctx.response_headers`（quota 头）、`ctx.telemetry.requestedModel`。

### Step 4: 翻译 request_log (order 700, default_enabled=false)

**JS 源文件**: `request-log.mjs`

| Hook | 操作 |
|------|------|
| Request | 记录请求开始时间到 `ctx.meta["_reqStartTime"]` |
| ResponseStart | 记录 HTTP status 到 `ctx.meta["_reqStatus"]` |
| Stream | 在 message_delta 中记录 output tokens，最终输出 NDJSON 行到 `CACHE_FIX_REQUEST_LOG` 路径 |

### Step 5: 在 load.rs 中注册

```rust
// overage-warning
registry.response_exts.push(Box::new(overage_warning::OverageWarning::new()));
registry.stream_exts.push(Box::new(overage_warning::OverageWarning::new()));

// rate-limit-log
registry.request_exts.push(Box::new(rate_limit_log::RateLimitLog::new()));
registry.response_exts.push(Box::new(rate_limit_log::RateLimitLog::new()));

// usage-log
registry.stream_exts.push(Box::new(usage_log::UsageLog::new()));

// request-log
registry.request_exts.push(Box::new(request_log::RequestLog::new()));
registry.response_exts.push(Box::new(request_log::RequestLog::new()));
registry.stream_exts.push(Box::new(request_log::RequestLog::new()));
```

### Step 6: 在 mod.rs 中声明模块

```rust
pub mod overage_warning;
pub mod rate_limit_log;
pub mod usage_log;
pub mod request_log;
```

### Step 7: 编译 + 提交

```bash
cd src-tauri && cargo check 2>&1
git add src-tauri/src/proxy/extensions/
git commit -m "feat(extensions): add multi-hook group2 — overage-warning, rate-limit-log, usage-log, request-log"
```
