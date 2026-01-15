//! OpenCode 配置文件读写模块
//!
//! 处理 `~/.config/opencode/opencode.json` 配置文件的读写操作。
//! OpenCode 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。

use crate::error::AppError;
use crate::settings::get_opencode_override_dir;
use std::path::PathBuf;

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

/// 读取 OpenCode 配置文件
///
/// 返回完整的配置 JSON 对象
pub fn read_opencode_config() -> Result<serde_json::Value, AppError> {
    let path = get_opencode_config_path();

    if !path.exists() {
        // Return empty config with schema
        return Ok(serde_json::json!({
            "$schema": "https://opencode.ai/config.json"
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))
}

/// 写入 OpenCode 配置文件（原子写入）
///
/// 使用临时文件 + 重命名确保原子性
pub fn write_opencode_config(config: &serde_json::Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // Write to temporary file first
    let temp_path = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| AppError::JsonSerialize { source: e })?;

    std::fs::write(&temp_path, &content).map_err(|e| AppError::io(&temp_path, e))?;

    // Atomic rename
    std::fs::rename(&temp_path, &path).map_err(|e| AppError::io(&path, e))?;

    Ok(())
}

/// 获取所有供应商配置
pub fn get_providers() -> Result<serde_json::Map<String, serde_json::Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置供应商配置
pub fn set_provider(id: &str, config: serde_json::Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = serde_json::json!({});
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

/// 获取所有 MCP 服务器配置
pub fn get_mcp_servers() -> Result<serde_json::Map<String, serde_json::Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置 MCP 服务器配置
pub fn set_mcp_server(id: &str, config: serde_json::Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = serde_json::json!({});
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
