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
        _ctx: &mut ResponseStartContext,
    ) -> Result<(), ExtensionError> {
        Ok(())
    }

    /// 完整响应体就绪后调用（非流式路径）。
    fn on_response(&self, _ctx: &mut ResponseContext) -> Result<(), ExtensionError> {
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use serde_json::json;

    struct NoopExtension;
    impl Extension for NoopExtension {
        fn name(&self) -> &str { "noop" }
        fn order(&self) -> u32 { 0 }
        fn default_enabled(&self) -> bool { true }
    }
    impl RequestExtension for NoopExtension {
        fn on_request(&self, _ctx: &mut RequestContext) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
            Ok(None)
        }
    }
    impl ResponseExtension for NoopExtension {}
    impl StreamExtension for NoopExtension {
        fn on_stream_event(&self, _ctx: &mut StreamEventContext) -> Result<(), ExtensionError> {
            Ok(())
        }
    }

    #[test]
    fn extension_is_object_safe() {
        let ext: &dyn Extension = &NoopExtension;
        assert_eq!(ext.name(), "noop");
        assert_eq!(ext.order(), 0);
        assert!(ext.default_enabled());
    }

    #[test]
    fn request_extension_returns_none_by_default() {
        let ext = NoopExtension;
        let mut ctx = RequestContext {
            body: json!({}),
            headers: HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let result = ext.on_request(&mut ctx).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn response_extension_default_impls_return_ok() {
        let ext = NoopExtension;
        let mut rsc = ResponseStartContext {
            status: 200,
            headers: HeaderMap::new(),
            upstream_headers: HeaderMap::new(),
            meta: ExtensionMeta::default(),
        };
        let mut rc = ResponseContext {
            status: 200,
            headers: HeaderMap::new(),
            body: vec![],
            meta: ExtensionMeta::default(),
        };
        assert!(ext.on_response_start(&mut rsc).is_ok());
        assert!(ext.on_response(&mut rc).is_ok());
    }
}
