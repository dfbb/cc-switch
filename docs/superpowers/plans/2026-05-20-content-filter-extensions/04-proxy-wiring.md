# Task 04: ProxyState 集成 — 注册 ExtensionRegistry

**可并行**: 否 — 依赖 Task 01, 02

**依赖**: Task 01, 02

## 目标

将 `ExtensionRegistry` 添加到 `ProxyState` 中，代理启动时加载并注册所有 extension。

## 文件

- Modify: `src-tauri/src/proxy/server.rs` — ProxyState, ProxyServer 启动流程
- Create: `src-tauri/src/proxy/extensions/load.rs` — extension 加载和注册函数

---

### Step 1: 创建 load.rs — 集中管理 extension 注册

`src-tauri/src/proxy/extensions/load.rs`:

```rust
use super::registry::ExtensionRegistry;

/// 加载并注册所有已实现的 extension。
/// 每个 extension 在此函数中构造并 `register_*` 到 registry。
/// 
/// v1 阶段：extension 为占位实现（空 body/no-op），
/// 实际逻辑在后续 task 中逐个翻译自 JS。
pub fn load_extensions() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::empty();

    // 所有 extension 在此注册（v1 占位，后续 task 替换为实际实现）
    // 示例：
    // registry.register_request(Box::new(FingerprintStrip::new()));
    // registry.register_request(Box::new(ImageStrip::new()));
    // ...

    registry.finalize();
    log::info!(
        "[EXTENSIONS] 已加载 {} request / {} response / {} stream extensions",
        registry.request_exts.len(),
        registry.response_exts.len(),
        registry.stream_exts.len()
    );
    registry
}
```

### Step 2: 更新 extensions/mod.rs

添加 `load` 模块声明：

```rust
pub mod load;

pub use load::load_extensions;
```

### Step 3: 在 ProxyState 中添加 registry 字段

在 `src-tauri/src/proxy/server.rs` 的 `ProxyState` struct 中添加：

```rust
    /// Extension 注册表（跨请求共享）
    pub extension_registry: Arc<ExtensionRegistry>,
```

在 `use` 区域添加导入：

```rust
use super::extensions::{load_extensions, ExtensionRegistry};
```

### Step 4: ProxyServer::new() 中初始化 registry

在 `ProxyServer::new()` 的 `ProxyState` 构造中，添加字段：

```rust
            extension_registry: Arc::new(load_extensions()),
```

### Step 5: 编译验证

```bash
cd src-tauri && cargo check 2>&1
```

### Step 6: 提交

```bash
git add src-tauri/src/proxy/extensions/load.rs src-tauri/src/proxy/extensions/mod.rs src-tauri/src/proxy/server.rs
git commit -m "feat(extensions): wire ExtensionRegistry into ProxyState"
```
