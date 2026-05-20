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
