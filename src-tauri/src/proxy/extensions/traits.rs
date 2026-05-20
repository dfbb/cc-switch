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
