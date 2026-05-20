use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 单个 extension 在 config.json 中的配置条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionConfigEntry {
    pub default_enabled: bool,
    pub order: u32,
}

/// config.json 的完整内容
pub type ExtensionConfigMap = HashMap<String, ExtensionConfigEntry>;

/// 从嵌入的 config.json 字节加载配置
pub fn load_extension_config() -> ExtensionConfigMap {
    let json_str = include_str!("config.json");
    serde_json::from_str(json_str).unwrap_or_else(|e| {
        log::error!("[EXTENSIONS] 解析 config.json 失败: {e}");
        HashMap::new()
    })
}
