//! OpenCode 配置文件读写模块
//!
//! 处理 `~/.config/opencode/opencode.json` 配置文件的读写操作。
//! OpenCode 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。
//!
//! ## 配置文件格式
//!
//! ```json
//! {
//!   "$schema": "https://opencode.ai/config.json",
//!   "provider": {
//!     "my-provider": {
//!       "npm": "@ai-sdk/openai-compatible",
//!       "options": { "baseURL": "...", "apiKey": "{env:API_KEY}" },
//!       "models": { "gpt-4o": { "name": "GPT-4o" } }
//!     }
//!   },
//!   "mcp": {
//!     "my-server": { "type": "local", "command": ["..."] }
//!   }
//! }
//! ```

use crate::error::AppError;
use crate::provider::{OpenCodeModel, OpenCodeProviderConfig, OpenCodeProviderOptions};
use crate::settings::get_opencode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::path::PathBuf;

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 OpenCode 配置目录
///
/// 默认路径: `~/.config/opencode/`
/// 可通过 settings.opencode_config_dir 覆盖
pub fn get_opencode_dir() -> PathBuf {
    if let Some(override_dir) = get_opencode_override_dir() {
        return override_dir;
    }

    #[cfg(target_os = "windows")]
    {
        // Windows: %APPDATA%\opencode
        dirs::data_dir()
            .map(|d| d.join("opencode"))
            .unwrap_or_else(|| PathBuf::from(".config").join("opencode"))
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Unix: ~/.config/opencode
        dirs::home_dir()
            .map(|h| h.join(".config").join("opencode"))
            .unwrap_or_else(|| PathBuf::from(".config").join("opencode"))
    }
}

/// 获取 OpenCode 配置文件路径
///
/// 返回 `~/.config/opencode/opencode.json`
pub fn get_opencode_config_path() -> PathBuf {
    get_opencode_dir().join("opencode.json")
}

/// 获取 OpenCode 环境变量文件路径（如果存在）
///
/// 返回 `~/.config/opencode/.env`
#[allow(dead_code)]
pub fn get_opencode_env_path() -> PathBuf {
    get_opencode_dir().join(".env")
}

// ============================================================================
// Core Read/Write Functions
// ============================================================================

/// 读取 OpenCode 配置文件
///
/// 返回完整的配置 JSON 对象
pub fn read_opencode_config() -> Result<Value, AppError> {
    let path = get_opencode_config_path();

    if !path.exists() {
        // Return empty config with schema
        return Ok(json!({
            "$schema": "https://opencode.ai/config.json"
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))
}

/// 写入 OpenCode 配置文件（原子写入）
///
/// 使用临时文件 + 重命名确保原子性
pub fn write_opencode_config(config: &Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // Write to temporary file first
    let temp_path = path.with_extension("json.tmp");
    let content =
        serde_json::to_string_pretty(config).map_err(|e| AppError::JsonSerialize { source: e })?;

    std::fs::write(&temp_path, &content).map_err(|e| AppError::io(&temp_path, e))?;

    // Atomic rename
    std::fs::rename(&temp_path, &path).map_err(|e| AppError::io(&path, e))?;

    log::debug!("OpenCode config written to {:?}", path);
    Ok(())
}

/// 检查 OpenCode 配置文件是否存在
pub fn config_exists() -> bool {
    get_opencode_config_path().exists()
}

// ============================================================================
// Provider Functions (Untyped - for raw JSON operations)
// ============================================================================

/// 获取所有供应商配置（原始 JSON）
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置供应商配置（原始 JSON）
pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

/// 删除供应商配置
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_opencode_config(&config)
}

// ============================================================================
// Provider Functions (Typed - using OpenCodeProviderConfig)
// ============================================================================

/// 获取所有供应商配置（类型化）
pub fn get_typed_providers() -> Result<IndexMap<String, OpenCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse provider '{}': {}", id, e);
                // Skip invalid providers but continue
            }
        }
    }

    Ok(result)
}

/// 获取单个供应商配置（类型化）
pub fn get_typed_provider(id: &str) -> Result<Option<OpenCodeProviderConfig>, AppError> {
    let providers = get_providers()?;

    match providers.get(id) {
        Some(value) => {
            let config = serde_json::from_value::<OpenCodeProviderConfig>(value.clone())
                .map_err(|e| AppError::JsonSerialize { source: e })?;
            Ok(Some(config))
        }
        None => Ok(None),
    }
}

/// 设置供应商配置（类型化）
pub fn set_typed_provider(id: &str, config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    let value =
        serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

/// 批量设置供应商配置
pub fn set_providers_batch(
    providers: &IndexMap<String, OpenCodeProviderConfig>,
) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    let mut provider_map = Map::new();
    for (id, config) in providers {
        let value =
            serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
        provider_map.insert(id.clone(), value);
    }

    full_config["provider"] = Value::Object(provider_map);
    write_opencode_config(&full_config)
}

// ============================================================================
// MCP Functions
// ============================================================================

/// 获取所有 MCP 服务器配置
pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置 MCP 服务器配置
pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

/// 删除 MCP 服务器配置
pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_opencode_config(&config)
}

/// 批量设置 MCP 服务器配置
pub fn set_mcp_servers_batch(servers: &Map<String, Value>) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;
    full_config["mcp"] = Value::Object(servers.clone());
    write_opencode_config(&full_config)
}

/// 清空所有 MCP 服务器配置
pub fn clear_mcp_servers() -> Result<(), AppError> {
    let mut config = read_opencode_config()?;
    config["mcp"] = json!({});
    write_opencode_config(&config)
}

// ============================================================================
// Utility Functions
// ============================================================================

/// 创建新的供应商配置
///
/// 便捷方法，用于创建 OpenAI 兼容的供应商配置
pub fn create_provider_config(
    npm_package: &str,
    base_url: Option<&str>,
    api_key: Option<&str>,
    models: HashMap<String, String>,
) -> OpenCodeProviderConfig {
    let options = OpenCodeProviderOptions {
        base_url: base_url.map(|s| s.to_string()),
        api_key: api_key.map(|s| s.to_string()),
        headers: None,
    };

    let model_map: HashMap<String, OpenCodeModel> = models
        .into_iter()
        .map(|(id, name)| {
            (
                id,
                OpenCodeModel {
                    name,
                    limit: None,
                },
            )
        })
        .collect();

    OpenCodeProviderConfig {
        npm: npm_package.to_string(),
        name: None,
        options,
        models: model_map,
    }
}

/// 验证供应商配置
pub fn validate_provider_config(config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    // npm package must not be empty
    if config.npm.trim().is_empty() {
        return Err(AppError::localized(
            "opencode.provider.npm.empty",
            "npm 包名不能为空",
            "npm package name cannot be empty",
        ));
    }

    // npm package should start with @ or be a valid package name
    if !config.npm.starts_with('@') && !config.npm.chars().all(|c| c.is_alphanumeric() || c == '-')
    {
        log::warn!(
            "Unusual npm package name: {}. Expected format like '@ai-sdk/openai'",
            config.npm
        );
    }

    Ok(())
}

/// 从通用 Provider 转换为 OpenCode 配置
///
/// 用于从数据库 Provider 结构转换为 OpenCode 配置格式
pub fn provider_to_opencode_config(
    settings_config: &Value,
) -> Result<OpenCodeProviderConfig, AppError> {
    serde_json::from_value(settings_config.clone()).map_err(|e| AppError::JsonSerialize { source: e })
}

/// 将 OpenCode 配置转换为通用 Provider settings_config
pub fn opencode_config_to_provider(config: &OpenCodeProviderConfig) -> Result<Value, AppError> {
    serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })
}

