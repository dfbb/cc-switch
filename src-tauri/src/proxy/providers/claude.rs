//! Claude (Anthropic) Provider Adapter
//!
//! 支持透传模式和 OpenAI Chat Completions 格式转换模式
//!
//! ## API 格式
//! - **anthropic** (默认): Anthropic Messages API 格式，直接透传
//! - **openai_chat**: OpenAI Chat Completions 格式，需要 Anthropic ↔ OpenAI 转换
//!
//! ## 认证模式
//! - **Claude**: Anthropic 官方 API (x-api-key + anthropic-version)
//! - **ClaudeAuth**: 中转服务 (仅 Bearer 认证，无 x-api-key)
//! - **OpenRouter**: 已支持 Claude Code 兼容接口，默认透传

use super::{AuthInfo, AuthStrategy, ProviderAdapter, ProviderType};
use crate::provider::Provider;
use crate::proxy::error::ProxyError;
use reqwest::RequestBuilder;

/// Claude 适配器
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }

    /// 获取供应商类型
    ///
    /// 根据 base_url 和 auth_mode 检测具体的供应商类型：
    /// - OpenRouter: base_url 包含 openrouter.ai
    /// - ClaudeAuth: auth_mode 为 bearer_only
    /// - Claude: 默认 Anthropic 官方
    pub fn provider_type(&self, provider: &Provider) -> ProviderType {
        // 检测 OpenRouter
        if self.is_openrouter(provider) {
            return ProviderType::OpenRouter;
        }

        // 检测 ClaudeAuth (仅 Bearer 认证)
        if self.is_bearer_only_mode(provider) {
            return ProviderType::ClaudeAuth;
        }

        ProviderType::Claude
    }

    /// 检测是否使用 OpenRouter
    fn is_openrouter(&self, provider: &Provider) -> bool {
        if let Ok(base_url) = self.extract_base_url(provider) {
            return base_url.contains("openrouter.ai");
        }
        false
    }

    /// 获取 API 格式
    ///
    /// 从 settings_config.api_format 读取格式设置：
    /// - "anthropic" (默认): Anthropic Messages API 格式，直接透传
    /// - "openai_chat": OpenAI Chat Completions 格式，需要格式转换
    ///
    /// 为了向后兼容，如果存在旧的 openrouter_compat_mode=true，也会启用 openai_chat 格式
    fn get_api_format(&self, provider: &Provider) -> &'static str {
        // 1. 首先检查新的 api_format 字段
        if let Some(api_format) = provider
            .settings_config
            .get("api_format")
            .and_then(|v| v.as_str())
        {
            return match api_format {
                "openai_chat" => "openai_chat",
                _ => "anthropic",
            };
        }

        // 2. 向后兼容：检查旧的 openrouter_compat_mode 字段
        let raw = provider.settings_config.get("openrouter_compat_mode");
        let is_compat_enabled = match raw {
            Some(serde_json::Value::Bool(enabled)) => *enabled,
            Some(serde_json::Value::Number(num)) => num.as_i64().unwrap_or(0) != 0,
            Some(serde_json::Value::String(value)) => {
                let normalized = value.trim().to_lowercase();
                normalized == "true" || normalized == "1"
            }
            _ => false,
        };

        if is_compat_enabled {
            return "openai_chat";
        }

        // 3. 默认使用 Anthropic 原生格式
        "anthropic"
    }

    /// 检测是否为仅 Bearer 认证模式
    fn is_bearer_only_mode(&self, provider: &Provider) -> bool {
        // 检查 settings_config 中的 auth_mode
        if let Some(auth_mode) = provider
            .settings_config
            .get("auth_mode")
            .and_then(|v| v.as_str())
        {
            if auth_mode == "bearer_only" {
                return true;
            }
        }

        // 检查 env 中的 AUTH_MODE
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(auth_mode) = env.get("AUTH_MODE").and_then(|v| v.as_str()) {
                if auth_mode == "bearer_only" {
                    return true;
                }
            }
        }

        false
    }

    /// 从 Provider 配置中提取 API Key
    fn extract_key(&self, provider: &Provider) -> Option<String> {
        if let Some(env) = provider.settings_config.get("env") {
            // Anthropic 标准 key
            if let Some(key) = env
                .get("ANTHROPIC_AUTH_TOKEN")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                log::debug!("[Claude] 使用 ANTHROPIC_AUTH_TOKEN");
                return Some(key.to_string());
            }
            if let Some(key) = env
                .get("ANTHROPIC_API_KEY")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                log::debug!("[Claude] 使用 ANTHROPIC_API_KEY");
                return Some(key.to_string());
            }
            // OpenRouter key
            if let Some(key) = env
                .get("OPENROUTER_API_KEY")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                log::debug!("[Claude] 使用 OPENROUTER_API_KEY");
                return Some(key.to_string());
            }
            // 备选 OpenAI key (用于 OpenRouter)
            if let Some(key) = env
                .get("OPENAI_API_KEY")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                log::debug!("[Claude] 使用 OPENAI_API_KEY");
                return Some(key.to_string());
            }
        }

        // 尝试直接获取
        if let Some(key) = provider
            .settings_config
            .get("apiKey")
            .or_else(|| provider.settings_config.get("api_key"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            log::debug!("[Claude] 使用 apiKey/api_key");
            return Some(key.to_string());
        }

        log::warn!("[Claude] 未找到有效的 API Key");
        None
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderAdapter for ClaudeAdapter {
    fn name(&self) -> &'static str {
        "Claude"
    }

    fn extract_base_url(&self, provider: &Provider) -> Result<String, ProxyError> {
        // 1. 从 env 中获取
        if let Some(env) = provider.settings_config.get("env") {
            if let Some(url) = env.get("ANTHROPIC_BASE_URL").and_then(|v| v.as_str()) {
                return Ok(url.trim_end_matches('/').to_string());
            }
        }

        // 2. 尝试直接获取
        if let Some(url) = provider
            .settings_config
            .get("base_url")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        if let Some(url) = provider
            .settings_config
            .get("baseURL")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        if let Some(url) = provider
            .settings_config
            .get("apiEndpoint")
            .and_then(|v| v.as_str())
        {
            return Ok(url.trim_end_matches('/').to_string());
        }

        Err(ProxyError::ConfigError(
            "Claude Provider 缺少 base_url 配置".to_string(),
        ))
    }

    fn extract_auth(&self, provider: &Provider) -> Option<AuthInfo> {
        let provider_type = self.provider_type(provider);
        let strategy = match provider_type {
            ProviderType::OpenRouter => AuthStrategy::Bearer,
            ProviderType::ClaudeAuth => AuthStrategy::ClaudeAuth,
            _ => AuthStrategy::Anthropic,
        };

        self.extract_key(provider)
            .map(|key| AuthInfo::new(key, strategy))
    }

    fn build_url(&self, base_url: &str, endpoint: &str) -> String {
        // NOTE:
        // 过去 OpenRouter 只有 OpenAI Chat Completions 兼容接口，需要把 Claude 的 `/v1/messages`
        // 映射到 `/v1/chat/completions`，并做 Anthropic ↔ OpenAI 的格式转换。
        //
        // 现在 OpenRouter 已推出 Claude Code 兼容接口，因此默认直接透传 endpoint。
        // 如需回退旧逻辑，可在 forwarder 中根据 needs_transform 改写 endpoint。

        let base = format!(
            "{}/{}",
            base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );

        // 为 /v1/messages 端点添加 ?beta=true 参数
        // 这是某些上游服务（如 DuckCoding）验证请求来源的关键参数
        if endpoint.contains("/v1/messages") && !endpoint.contains("?") {
            format!("{base}?beta=true")
        } else {
            base
        }
    }

    fn add_auth_headers(&self, request: RequestBuilder, auth: &AuthInfo) -> RequestBuilder {
        // 注意：anthropic-version 由 forwarder.rs 统一处理（透传客户端值或设置默认值）
        // 这里不再设置 anthropic-version，避免 header 重复
        match auth.strategy {
            // Anthropic 官方: Authorization Bearer + x-api-key
            AuthStrategy::Anthropic => request
                .header("Authorization", format!("Bearer {}", auth.api_key))
                .header("x-api-key", &auth.api_key),
            // ClaudeAuth 中转服务: 仅 Bearer，无 x-api-key
            AuthStrategy::ClaudeAuth => {
                request.header("Authorization", format!("Bearer {}", auth.api_key))
            }
            // OpenRouter: Bearer
            AuthStrategy::Bearer => {
                request.header("Authorization", format!("Bearer {}", auth.api_key))
            }
            _ => request,
        }
    }

    fn needs_transform(&self, provider: &Provider) -> bool {
        // 根据 api_format 配置决定是否需要格式转换
        // - "anthropic" (默认): 直接透传，无需转换
        // - "openai_chat": 需要 Anthropic ↔ OpenAI 格式转换
        self.get_api_format(provider) == "openai_chat"
    }

    fn transform_request(
        &self,
        body: serde_json::Value,
        provider: &Provider,
    ) -> Result<serde_json::Value, ProxyError> {
        super::transform::anthropic_to_openai(body, provider)
    }

    fn transform_response(&self, body: serde_json::Value) -> Result<serde_json::Value, ProxyError> {
        super::transform::openai_to_anthropic(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_provider(config: serde_json::Value) -> Provider {
        Provider {
            id: "test".to_string(),
            name: "Test Claude".to_string(),
            settings_config: config,
            website_url: None,
            category: Some("claude".to_string()),
            created_at: None,
            sort_index: None,
            notes: None,
            meta: None,
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    fn test_extract_base_url_from_env() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }));

        let url = adapter.extract_base_url(&provider).unwrap();
        assert_eq!(url, "https://api.anthropic.com");
    }

    #[test]
    fn test_extract_auth_anthropic() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-ant-test-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-ant-test-key");
        assert_eq!(auth.strategy, AuthStrategy::Anthropic);
    }

    #[test]
    fn test_extract_auth_anthropic_api_key() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_API_KEY": "sk-ant-test-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-ant-test-key");
        assert_eq!(auth.strategy, AuthStrategy::Anthropic);
    }

    #[test]
    fn test_extract_auth_openrouter() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://openrouter.ai/api",
                "OPENROUTER_API_KEY": "sk-or-test-key"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-or-test-key");
        assert_eq!(auth.strategy, AuthStrategy::Bearer);
    }

    #[test]
    fn test_extract_auth_claude_auth_mode() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://some-proxy.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-proxy-key"
            },
            "auth_mode": "bearer_only"
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-proxy-key");
        assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
    }

    #[test]
    fn test_extract_auth_claude_auth_env_mode() {
        let adapter = ClaudeAdapter::new();
        let provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://some-proxy.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-proxy-key",
                "AUTH_MODE": "bearer_only"
            }
        }));

        let auth = adapter.extract_auth(&provider).unwrap();
        assert_eq!(auth.api_key, "sk-proxy-key");
        assert_eq!(auth.strategy, AuthStrategy::ClaudeAuth);
    }

    #[test]
    fn test_provider_type_detection() {
        let adapter = ClaudeAdapter::new();

        // Anthropic 官方
        let anthropic = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-ant-test"
            }
        }));
        assert_eq!(adapter.provider_type(&anthropic), ProviderType::Claude);

        // OpenRouter
        let openrouter = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://openrouter.ai/api",
                "OPENROUTER_API_KEY": "sk-or-test"
            }
        }));
        assert_eq!(adapter.provider_type(&openrouter), ProviderType::OpenRouter);

        // ClaudeAuth
        let claude_auth = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://some-proxy.com",
                "ANTHROPIC_AUTH_TOKEN": "sk-test"
            },
            "auth_mode": "bearer_only"
        }));
        assert_eq!(
            adapter.provider_type(&claude_auth),
            ProviderType::ClaudeAuth
        );
    }

    #[test]
    fn test_build_url_anthropic() {
        let adapter = ClaudeAdapter::new();
        // /v1/messages 端点会自动添加 ?beta=true 参数
        let url = adapter.build_url("https://api.anthropic.com", "/v1/messages");
        assert_eq!(url, "https://api.anthropic.com/v1/messages?beta=true");
    }

    #[test]
    fn test_build_url_openrouter() {
        let adapter = ClaudeAdapter::new();
        // /v1/messages 端点会自动添加 ?beta=true 参数
        let url = adapter.build_url("https://openrouter.ai/api", "/v1/messages");
        assert_eq!(url, "https://openrouter.ai/api/v1/messages?beta=true");
    }

    #[test]
    fn test_build_url_no_beta_for_other_endpoints() {
        let adapter = ClaudeAdapter::new();
        // 非 /v1/messages 端点不添加 ?beta=true
        let url = adapter.build_url("https://api.anthropic.com", "/v1/complete");
        assert_eq!(url, "https://api.anthropic.com/v1/complete");
    }

    #[test]
    fn test_build_url_preserve_existing_query() {
        let adapter = ClaudeAdapter::new();
        // 已有查询参数时不重复添加
        let url = adapter.build_url("https://api.anthropic.com", "/v1/messages?foo=bar");
        assert_eq!(url, "https://api.anthropic.com/v1/messages?foo=bar");
    }

    #[test]
    fn test_needs_transform() {
        let adapter = ClaudeAdapter::new();

        // Default: no transform (anthropic format)
        let anthropic_provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.anthropic.com"
            }
        }));
        assert!(!adapter.needs_transform(&anthropic_provider));

        // Explicit anthropic format: no transform
        let explicit_anthropic = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "api_format": "anthropic"
        }));
        assert!(!adapter.needs_transform(&explicit_anthropic));

        // OpenAI Chat format: needs transform
        let openai_chat_provider = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "api_format": "openai_chat"
        }));
        assert!(adapter.needs_transform(&openai_chat_provider));

        // Backward compatibility: openrouter_compat_mode=true should enable transform
        let legacy_compat_enabled = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "openrouter_compat_mode": true
        }));
        assert!(adapter.needs_transform(&legacy_compat_enabled));

        // Backward compatibility: openrouter_compat_mode=false should not enable transform
        let legacy_compat_disabled = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "openrouter_compat_mode": false
        }));
        assert!(!adapter.needs_transform(&legacy_compat_disabled));

        // api_format takes precedence over openrouter_compat_mode
        let format_precedence = create_provider(json!({
            "env": {
                "ANTHROPIC_BASE_URL": "https://api.example.com"
            },
            "api_format": "anthropic",
            "openrouter_compat_mode": true
        }));
        assert!(!adapter.needs_transform(&format_precedence));
    }
}
