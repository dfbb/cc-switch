use crate::provider::ExtensionFilterConfig;

use super::config::{load_extension_config, ExtensionConfigMap};
use super::context::*;
use super::errors::ExtensionError;
use super::traits::*;

pub struct ExtensionRegistry {
    pub request_exts: Vec<Box<dyn RequestExtension>>,
    pub response_exts: Vec<Box<dyn ResponseExtension>>,
    pub stream_exts: Vec<Box<dyn StreamExtension>>,
    config_map: ExtensionConfigMap,
}

impl ExtensionRegistry {
    /// 空 registry（无任何 extension 注册时使用）。
    pub fn empty() -> Self {
        Self {
            request_exts: Vec::new(),
            response_exts: Vec::new(),
            stream_exts: Vec::new(),
            config_map: load_extension_config(),
        }
    }

    /// 注册所有 extension 后调用，按 order 排序。
    pub fn finalize(&mut self) {
        let cfg = &self.config_map;
        self.request_exts.sort_by_key(|e| {
            cfg.get(e.name()).map(|c| c.order).unwrap_or(e.order())
        });
        self.response_exts.sort_by_key(|e| {
            cfg.get(e.name()).map(|c| c.order).unwrap_or(e.order())
        });
        self.stream_exts.sort_by_key(|e| {
            cfg.get(e.name()).map(|c| c.order).unwrap_or(e.order())
        });
    }

    /// 判断某个 extension 是否应在当前 provider 配置下执行。
    fn is_enabled(
        &self,
        name: &str,
        default_enabled: bool,
        filter: &ExtensionFilterConfig,
    ) -> bool {
        if !filter.enabled.unwrap_or(false) {
            return false;
        }
        filter
            .extensions
            .get(name)
            .copied()
            .unwrap_or(default_enabled)
    }

    /// 执行请求预处理管道。
    /// 返回 `Ok(None)` = 正常转发；`Ok(Some(...))` = extension 拦截。
    /// `Err` 仅用于框架致命错误。
    pub fn run_request_pipeline(
        &self,
        ctx: &mut RequestContext,
        config: &ExtensionFilterConfig,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError> {
        for ext in &self.request_exts {
            if !self.is_enabled(ext.name(), ext.default_enabled(), config) {
                continue;
            }
            match ext.on_request(ctx) {
                Ok(Some(intercept)) => return Ok(Some(intercept)),
                Ok(None) => {}
                Err(e) => log::warn!("[EXTENSIONS] {} on_request 错误: {e}", ext.name()),
            }
        }
        Ok(None)
    }

    /// 执行响应 headers 管道。
    pub fn run_response_start_pipeline(
        &self,
        ctx: &mut ResponseStartContext,
        config: &ExtensionFilterConfig,
    ) -> Result<(), ExtensionError> {
        for ext in &self.response_exts {
            if !self.is_enabled(ext.name(), ext.default_enabled(), config) {
                continue;
            }
            if let Err(e) = ext.on_response_start(ctx) {
                log::warn!("[EXTENSIONS] {} on_response_start 错误: {e}", ext.name());
            }
        }
        Ok(())
    }

    /// 执行完整响应管道（非流式）。
    pub fn run_response_pipeline(
        &self,
        ctx: &mut ResponseContext,
        config: &ExtensionFilterConfig,
    ) -> Result<(), ExtensionError> {
        for ext in &self.response_exts {
            if !self.is_enabled(ext.name(), ext.default_enabled(), config) {
                continue;
            }
            if let Err(e) = ext.on_response(ctx) {
                log::warn!("[EXTENSIONS] {} on_response 错误: {e}", ext.name());
            }
        }
        Ok(())
    }

    /// 执行流事件管道。
    pub fn run_stream_event_pipeline(
        &self,
        ctx: &mut StreamEventContext,
        config: &ExtensionFilterConfig,
    ) -> Result<(), ExtensionError> {
        for ext in &self.stream_exts {
            if !self.is_enabled(ext.name(), ext.default_enabled(), config) {
                continue;
            }
            if let Err(e) = ext.on_stream_event(ctx) {
                log::warn!("[EXTENSIONS] {} on_stream_event 错误: {e}", ext.name());
            }
            if ctx.drop {
                break;
            }
        }
        Ok(())
    }
}
