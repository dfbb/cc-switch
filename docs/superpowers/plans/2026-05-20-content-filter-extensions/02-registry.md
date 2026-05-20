# Task 02: ExtensionRegistry — 加载、排序、管道执行

**可并行**: 否 — 依赖 Task 01（traits/types）

**依赖**: Task 01

## 目标

实现 `ExtensionRegistry`，负责加载所有 extension、按 order 排序、以及执行四种管道（request/response_start/response/stream_event）。

## 文件

- Create: `src-tauri/src/proxy/extensions/registry.rs`
- Create: `src-tauri/src/proxy/extensions/config.rs`
- Create: `src-tauri/src/proxy/extensions/config.json`
- Modify: `src-tauri/src/proxy/extensions/mod.rs`

---

### Step 1: 创建 config.json

`src-tauri/src/proxy/extensions/config.json`:

```json
{
  "upstream-change-detection": { "default_enabled": true, "order": 50 },
  "ttl-tier-detect": { "default_enabled": true, "order": 75 },
  "output-efficiency-rewrite": { "default_enabled": false, "order": 90 },
  "fingerprint-strip": { "default_enabled": true, "order": 100 },
  "image-strip": { "default_enabled": true, "order": 150 },
  "sort-stabilization": { "default_enabled": true, "order": 200 },
  "fresh-session-sort": { "default_enabled": true, "order": 250 },
  "identity-normalization": { "default_enabled": true, "order": 300 },
  "smoosh-split": { "default_enabled": true, "order": 320 },
  "content-strip": { "default_enabled": true, "order": 330 },
  "tool-input-normalize": { "default_enabled": true, "order": 340 },
  "microcompact-stability": { "default_enabled": true, "order": 350 },
  "deferred-tools-restore": { "default_enabled": true, "order": 350 },
  "thinking-display": { "default_enabled": true, "order": 360 },
  "cache-control-normalize": { "default_enabled": true, "order": 400 },
  "messages-cache-breakpoint": { "default_enabled": true, "order": 410 },
  "ttl-management": { "default_enabled": true, "order": 500 },
  "cache-telemetry": { "default_enabled": true, "order": 600 },
  "overage-warning": { "default_enabled": true, "order": 610 },
  "rate-limit-log": { "default_enabled": false, "order": 660 },
  "request-log": { "default_enabled": false, "order": 700 },
  "usage-log": { "default_enabled": false, "order": 650 },
  "prefix-diff": { "default_enabled": true, "order": 680 }
}
```

### Step 2: 创建 config.rs

`src-tauri/src/proxy/extensions/config.rs`:

```rust
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
```

### Step 3: 创建 registry.rs

`src-tauri/src/proxy/extensions/registry.rs`:

```rust
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
    fn is_enabled(&self, name: &str, default_enabled: bool, filter: &ExtensionFilterConfig) -> bool {
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
```

### Step 4: 更新 extensions/mod.rs

在 `mod.rs` 中新增模块声明：

```rust
pub mod config;
pub mod registry;

pub use config::load_extension_config;
pub use registry::ExtensionRegistry;
```

### Step 5: 编译验证

```bash
cd src-tauri && cargo check 2>&1
```

### Step 6: 提交

```bash
git add src-tauri/src/proxy/extensions/
git commit -m "feat(extensions): add ExtensionRegistry with config loading and pipeline execution"
```
