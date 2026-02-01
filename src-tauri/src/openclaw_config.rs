//! OpenClaw 配置文件读写模块
//!
//! 处理 `~/.openclaw/openclaw.json` 配置文件的读写操作（JSON5 格式）。
//! OpenClaw 使用累加式供应商管理，所有供应商配置共存于同一配置文件中。
//!
//! ## 配置文件格式
//!
//! ```json5
//! {
//!   // 模型供应商配置（映射为 CC Switch 的"供应商"）
//!   models: {
//!     mode: "merge",
//!     providers: {
//!       "custom-provider": {
//!         baseUrl: "https://api.example.com/v1",
//!         apiKey: "${API_KEY}",
//!         api: "openai-completions",
//!         models: [{ id: "model-id", name: "Model Name" }]
//!       }
//!     }
//!   },
//!   // 环境变量配置
//!   env: {
//!     ANTHROPIC_API_KEY: "sk-...",
//!     vars: { ... }
//!   },
//!   // Agent 默认模型配置
//!   agents: {
//!     defaults: {
//!       model: {
//!         primary: "provider/model",
//!         fallbacks: ["provider2/model2"]
//!       }
//!     }
//!   }
//! }
//! ```

use crate::config::write_json_file;
use crate::error::AppError;
use crate::settings::get_openclaw_override_dir;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::PathBuf;

// ============================================================================
// Path Functions
// ============================================================================

/// 获取 OpenClaw 配置目录
///
/// 默认路径: `~/.openclaw/`
/// 可通过 settings.openclaw_config_dir 覆盖
pub fn get_openclaw_dir() -> PathBuf {
    if let Some(override_dir) = get_openclaw_override_dir() {
        return override_dir;
    }

    // 所有平台统一使用 ~/.openclaw
    dirs::home_dir()
        .map(|h| h.join(".openclaw"))
        .unwrap_or_else(|| PathBuf::from(".openclaw"))
}

/// 获取 OpenClaw 配置文件路径
///
/// 返回 `~/.openclaw/openclaw.json`
pub fn get_openclaw_config_path() -> PathBuf {
    get_openclaw_dir().join("openclaw.json")
}

// ============================================================================
// Type Definitions
// ============================================================================

/// OpenClaw 供应商配置（对应 models.providers 中的条目）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawProviderConfig {
    /// API 基础 URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// API Key（支持环境变量引用 ${VAR_NAME}）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// API 类型（如 "openai-completions", "anthropic" 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,

    /// 支持的模型列表
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<OpenClawModelEntry>,

    /// 其他自定义字段（保留原始配置）
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// OpenClaw 模型条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawModelEntry {
    /// 模型 ID
    pub id: String,

    /// 模型显示名称
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 其他自定义字段
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

// ============================================================================
// Core Read/Write Functions
// ============================================================================

/// 读取 OpenClaw 配置文件
///
/// 支持 JSON5 格式，返回完整的配置 JSON 对象
pub fn read_openclaw_config() -> Result<Value, AppError> {
    let path = get_openclaw_config_path();

    if !path.exists() {
        // Return empty config structure
        return Ok(json!({
            "models": {
                "mode": "merge",
                "providers": {}
            }
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    // 尝试 JSON5 解析（支持注释和尾随逗号）
    json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse OpenClaw config as JSON5: {}",
            e
        ))
    })
}

/// 写入 OpenClaw 配置文件（原子写入）
///
/// 使用标准 JSON 格式写入（JSON5 是 JSON 的超集）
pub fn write_openclaw_config(config: &Value) -> Result<(), AppError> {
    let path = get_openclaw_config_path();

    // 确保目录存在
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    // 复用统一的原子写入逻辑
    write_json_file(&path, config)?;

    log::debug!("OpenClaw config written to {path:?}");
    Ok(())
}

// ============================================================================
// Provider Functions (Untyped - for raw JSON operations)
// ============================================================================

/// 获取所有供应商配置（原始 JSON）
///
/// 从 `models.providers` 读取
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_openclaw_config()?;
    Ok(config
        .get("models")
        .and_then(|m| m.get("providers"))
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 设置供应商配置（原始 JSON）
///
/// 写入到 `models.providers`
pub fn set_provider(id: &str, provider_config: Value) -> Result<(), AppError> {
    let mut full_config = read_openclaw_config()?;

    // 确保 models 结构存在
    if full_config.get("models").is_none() {
        full_config["models"] = json!({
            "mode": "merge",
            "providers": {}
        });
    }

    // 确保 providers 对象存在
    if full_config["models"].get("providers").is_none() {
        full_config["models"]["providers"] = json!({});
    }

    // 设置供应商
    if let Some(providers) = full_config["models"]
        .get_mut("providers")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), provider_config);
    }

    write_openclaw_config(&full_config)
}

/// 删除供应商配置
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;

    if let Some(providers) = config
        .get_mut("models")
        .and_then(|m| m.get_mut("providers"))
        .and_then(|v| v.as_object_mut())
    {
        providers.remove(id);
    }

    write_openclaw_config(&config)
}

// ============================================================================
// Provider Functions (Typed)
// ============================================================================

/// 获取所有供应商配置（类型化）
pub fn get_typed_providers() -> Result<IndexMap<String, OpenClawProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenClawProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(e) => {
                log::warn!("Failed to parse OpenClaw provider '{id}': {e}");
                // Skip invalid providers but continue
            }
        }
    }

    Ok(result)
}

/// 设置供应商配置（类型化）
pub fn set_typed_provider(id: &str, config: &OpenClawProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}
