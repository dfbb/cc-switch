# Task 03: ProviderMeta 扩展 + ExtensionFilterConfig

**可并行**: 是 — 与 Task 02 并行

**依赖**: Task 01

## 目标

在 `ProviderMeta` 中新增 `extension_filter_config` 字段，定义 `ExtensionFilterConfig` 类型，含 `#[serde(default)]` 确保向后兼容。

## 文件

- Modify: `src-tauri/src/provider.rs` — 添加 `ExtensionFilterConfig` struct 和 `ProviderMeta` 新字段
- Modify: `src-tauri/src/database/schema.rs` — 数据库迁移（如需要）

---

### Step 1: 在 provider.rs 中添加 ExtensionFilterConfig

在 `src-tauri/src/provider.rs` 的 `ProviderMeta` struct 定义之前添加：

```rust
/// Extension 过滤配置（每个 provider 独立）。
/// 存储在 ProviderMeta.extension_filter_config 中。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFilterConfig {
    /// 总开关。缺失时视为 false——已有 provider 不受影响。
    #[serde(default)]
    pub enabled: Option<bool>,
    /// 每个 extension 的独立启用/禁用，覆盖 config.json 的 default_enabled。
    /// 缺失时回退到 extension 的 default_enabled。
    #[serde(default)]
    pub extensions: std::collections::HashMap<String, bool>,
    /// 预设标识: "full" | "cache-only" | "minimal" | null (自定义)。
    /// 仅 UI 记录用途，后端执行时不依赖此字段做判定。
    #[serde(default)]
    pub preset: Option<String>,
}
```

### Step 2: 在 ProviderMeta 中添加 extension_filter_config 字段

在 `ProviderMeta` struct 的最后（`github_account_id` 字段之后）添加：

```rust
    /// Extension 过滤配置（每个 provider 独立控制扩展管道）
    #[serde(
        default,
        rename = "extensionFilterConfig",
        skip_serializing_if = "Option::is_none"
    )]
    pub extension_filter_config: Option<ExtensionFilterConfig>,
```

### Step 3: ProviderMeta 支持读取扩展配置

为 `ProviderMeta` 添加辅助方法（放在现有 `impl ProviderMeta` 块中）：

```rust
    /// 获取 extension 过滤配置，缺失时返回默认（管道不运行）。
    pub fn get_extension_filter_config(&self) -> ExtensionFilterConfig {
        self.extension_filter_config.clone().unwrap_or(ExtensionFilterConfig {
            enabled: None,
            extensions: Default::default(),
            preset: None,
        })
    }
```

### Step 4: 编译验证

```bash
cd src-tauri && cargo check 2>&1
```

预期: 编译通过。新增类型在所有引用 `ProviderMeta` 的地方兼容（serde(default) 确保旧数据不报错）。

### Step 5: 提交

```bash
git add src-tauri/src/provider.rs
git commit -m "feat(extensions): add ExtensionFilterConfig to ProviderMeta"
```
