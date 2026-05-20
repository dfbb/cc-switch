# Task 01: Extension 基础设施 — Traits, Context, Errors

**可并行**: 否 — 所有 extension 和 registry 的共同基础

**依赖**: 无

## 目标

创建 `src-tauri/src/proxy/extensions/` 模块骨架，定义所有 extension 必须实现的 trait、4 种 context 类型和错误类型。

## 文件

- Create: `src-tauri/src/proxy/extensions/mod.rs`
- Create: `src-tauri/src/proxy/extensions/traits.rs`
- Create: `src-tauri/src/proxy/extensions/context.rs`
- Create: `src-tauri/src/proxy/extensions/errors.rs`
- Modify: `src-tauri/src/proxy/mod.rs`

---

### Step 1: 创建 mod.rs 模块声明

`src-tauri/src/proxy/extensions/mod.rs`:

```rust
pub mod context;
pub mod errors;
pub mod traits;

pub use context::*;
pub use errors::ExtensionError;
pub use traits::*;
```

### Step 2: 创建 errors.rs

`src-tauri/src/proxy/extensions/errors.rs`:

```rust
use std::fmt;

/// Extension 执行错误。
/// 单个 extension 错误不会中断管道——registry 内部 catch 并 log::warn。
#[derive(Debug)]
pub struct ExtensionError {
    pub extension_name: String,
    pub message: String,
    pub kind: ExtensionErrorKind,
}

#[derive(Debug)]
pub enum ExtensionErrorKind {
    /// JSON 解析/结构访问失败
    Json(String),
    /// 业务逻辑错误
    Logic(String),
    /// I/O 错误（如文件写入失败）
    Io(std::io::Error),
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.extension_name,
            match &self.kind {
                ExtensionErrorKind::Json(_) => "JSON",
                ExtensionErrorKind::Logic(_) => "Logic",
                ExtensionErrorKind::Io(_) => "IO",
            },
            self.message
        )
    }
}

impl ExtensionError {
    pub fn json(name: &str, msg: impl Into<String>) -> Self {
        Self {
            extension_name: name.to_string(),
            message: msg.into(),
            kind: ExtensionErrorKind::Json(msg.into()),
        }
    }

    pub fn logic(name: &str, msg: impl Into<String>) -> Self {
        Self {
            extension_name: name.to_string(),
            message: msg.into(),
            kind: ExtensionErrorKind::Logic(msg.into()),
        }
    }
}
```

### Step 3: 创建 traits.rs

`src-tauri/src/proxy/extensions/traits.rs`:

```rust
use super::context::*;
use super::errors::ExtensionError;

/// 所有 extension 必须实现的基础 trait。
/// Send + Sync 要求使得 extension 实例可以放入 Arc 并在多线程中使用。
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn order(&self) -> u32;
    fn default_enabled(&self) -> bool;
}

/// 请求预处理 extension。
/// 在请求转发到上游之前运行于 forwarder per-attempt 循环内。
pub trait RequestExtension: Extension {
    /// 检查和/或修改请求。
    /// - 返回 `Ok(None)` 表示正常转发
    /// - 返回 `Ok(Some((status, body)))` 表示拦截请求，直接返回合成响应
    fn on_request(
        &self,
        ctx: &mut RequestContext,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError>;
}

/// 响应后处理 extension。
/// 分别在 headers 到达和完整 body 就绪后调用。
pub trait ResponseExtension: Extension {
    /// 响应状态和 headers 到达时立即调用（body 尚未读取）。
    fn on_response_start(
        &self,
        ctx: &mut ResponseStartContext,
    ) -> Result<(), ExtensionError>;

    /// 完整响应体就绪后调用（非流式路径）。
    fn on_response(&self, ctx: &mut ResponseContext) -> Result<(), ExtensionError>;
}

/// SSE 流事件处理 extension。
/// 对每个流事件调用一次。
pub trait StreamExtension: Extension {
    /// 处理单个 SSE 事件。
    /// 设置 `ctx.drop = true` 可丢弃当前事件。
    fn on_stream_event(
        &self,
        ctx: &mut StreamEventContext,
    ) -> Result<(), ExtensionError>;
}
```

### Step 4: 创建 context.rs

`src-tauri/src/proxy/extensions/context.rs`:

```rust
use axum::http::HeaderMap;
use serde_json::Value;
use std::collections::HashMap;

/// 贯穿整个请求生命周期的共享元数据。
/// Extension 之间通过此 map 传递数据（跨阶段状态共享）。
#[derive(Debug, Clone, Default)]
pub struct ExtensionMeta {
    pub data: HashMap<String, Value>,
}

impl ExtensionMeta {
    pub fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        self.data.insert(key.into(), value);
    }
}

/// 累计遥测数据（跨 SSE 事件累积）。
#[derive(Debug, Clone, Default)]
pub struct TelemetryCollector {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub model: Option<String>,
}

/// 请求预处理上下文 — per-attempt 从原始请求克隆重建。
#[derive(Debug)]
pub struct RequestContext {
    /// 可变请求体（serde_json::Value）
    pub body: Value,
    /// 可变请求 headers
    pub headers: HeaderMap,
    /// 跨阶段共享元数据
    pub meta: ExtensionMeta,
}

/// 响应 headers 阶段上下文。
#[derive(Debug)]
pub struct ResponseStartContext {
    /// HTTP 状态码
    pub status: u16,
    /// 最终返回给客户端的 headers（只读）
    pub headers: HeaderMap,
    /// 原始上游响应 headers（只读，含 quota/ratelimit 等）
    pub upstream_headers: HeaderMap,
    /// 跨阶段共享元数据
    pub meta: ExtensionMeta,
}

/// 完整响应体上下文（非流式路径）。
#[derive(Debug)]
pub struct ResponseContext {
    /// HTTP 状态码
    pub status: u16,
    /// 可变响应 headers
    pub headers: HeaderMap,
    /// 可变响应体（bytes）
    pub body: Vec<u8>,
    /// 跨阶段共享元数据
    pub meta: ExtensionMeta,
}

/// SSE 流事件上下文。
#[derive(Debug)]
pub struct StreamEventContext {
    /// 事件类型: "message_start", "content_block_delta", "message_delta", ...
    pub event_type: String,
    /// 当前 SSE 事件的完整 JSON data
    pub data: Value,
    /// 原始上游响应 headers（只读）
    pub response_headers: HeaderMap,
    /// 设为 true 则丢弃此事件
    pub drop: bool,
    /// 跨阶段共享元数据
    pub meta: ExtensionMeta,
    /// 累计遥测数据
    pub telemetry: TelemetryCollector,
}
```

### Step 5: 在 proxy/mod.rs 中声明 extensions 模块

在 `src-tauri/src/proxy/mod.rs` 末尾的模块声明区域添加：

```rust
pub mod extensions;
```

### Step 6: 编译验证

```bash
cd src-tauri && cargo check 2>&1
```

预期: 编译通过，仅有未使用类型的 warning（正常）。

### Step 7: 提交

```bash
git add src-tauri/src/proxy/extensions/ src-tauri/src/proxy/mod.rs
git commit -m "feat(extensions): add Extension trait, context types, and error types"
```
