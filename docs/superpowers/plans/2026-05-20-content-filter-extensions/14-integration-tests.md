# Task 14: 集成测试 + Extension 单元测试

**可并行**: 否 — 依赖所有前置任务

**依赖**: Task 01-13 全部完成

## 目标

编写端到端测试验证 extension 管道的正确性：注册、排序、管道执行、错误隔离、故障转移 context 重置。每个 extension 有独立的输入→输出单元测试。

## 文件

- Create: `src-tauri/tests/extensions/mod.rs`
- Create: `src-tauri/tests/extensions/registry_tests.rs`
- Create: `src-tauri/tests/extensions/pipeline_tests.rs`
- Create: `src-tauri/tests/extensions/fixtures/` — JSON 请求 fixture 文件
- Modify: `src-tauri/tests/mod.rs` — 注册 extensions 测试模块

---

### Step 1: 创建 registry 测试

`src-tauri/tests/extensions/registry_tests.rs`:

```rust
use cc_switch::proxy::extensions::*;
use crate::provider::ExtensionFilterConfig;
use std::collections::HashMap;

/// 验证空 registry 不会 panic
#[test]
fn test_empty_registry_no_panic() {
    let registry = ExtensionRegistry::empty();
    let config = ExtensionFilterConfig {
        enabled: Some(true),
        extensions: HashMap::new(),
        preset: None,
    };

    let mut ctx = RequestContext {
        body: serde_json::json!({}),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::new(),
    };

    let result = registry.run_request_pipeline(&mut ctx, &config);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

/// 验证 disabled config 跳过所有 extension
#[test]
fn test_config_disabled_skips_all() {
    let mut registry = ExtensionRegistry::empty();
    registry.finalize();

    let config = ExtensionFilterConfig {
        enabled: None, // 缺失 = false
        extensions: HashMap::new(),
        preset: None,
    };

    let mut ctx = RequestContext {
        body: serde_json::json!({}),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::new(),
    };

    let result = registry.run_request_pipeline(&mut ctx, &config);
    assert!(result.is_ok());
}
```

### Step 2: 创建管道排序测试

验证 extension 按 order 执行：

```rust
/// 通过两个 extension 的 side effect 验证执行顺序。
#[test]
fn test_extensions_executed_in_order() {
    // 创建两个带 side effect 的 test extension：
    // - ext_a (order 10): 追加 "a" 到 ctx.meta["trace"]
    // - ext_b (order 20): 追加 "b" 到 ctx.meta["trace"]
    // 验证最终 trace == "ab"
}
```

### Step 3: 创建错误隔离测试

```rust
/// 验证单个 extension 的错误不中断管道。
#[test]
fn test_extension_error_does_not_block_pipeline() {
    // ext_a: 返回 Err
    // ext_b: 正常执行，在 meta 中写入 "ok"
    // 验证 meta["test"] == "ok"
}
```

### Step 4: 创建 context 克隆测试

```rust
/// 验证 per-attempt 从原始副本重建的结果：
/// A 的修改不泄漏到 B。
#[test]
fn test_context_clone_isolation() {
    let original_body = serde_json::json!({"image": "base64..."});

    // 模拟 provider A 的 image-strip 修改
    let mut ctx_a = RequestContext {
        body: original_body.clone(),
        headers: HeaderMap::new(),
        meta: ExtensionMeta::new(),
    };
    // ... 运行 A 的 pipeline ...

    // 从 original 重建 provider B 的 context
    let mut ctx_b = RequestContext {
        body: original_body.clone(),
        headers: HeaderMap::new(),
        meta: ExtensionMeta::new(),
    };
    // 验证 B 的 body 仍然保留 image 字段
    assert!(ctx_b.body["image"].is_string());
}
```

### Step 5: 创建 extension 单元测试

每个 extension 至少一个测试用例（放入对应的 extension 文件中的 `#[cfg(test)]` 模块）：

以 `fingerprint_strip.rs` 为例：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fingerprint_strip_removes_cc_version() {
        let ext = FingerprintStrip::new();
        let fixture = serde_json::json!({
            "model": "claude-sonnet-4-6",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": "cc_version: 1.2.3\n\nHello Claude"
                }]
            }]
        });

        let mut ctx = RequestContext {
            body: fixture,
            headers: axum::http::HeaderMap::new(),
            meta: ExtensionMeta::new(),
        };

        let result = ext.on_request(&mut ctx);
        assert!(result.is_ok());

        // 验证 cc_version 已移除，但用户文本保留
        let text = &ctx.body["messages"][0]["content"][0]["text"];
        let text_str = text.as_str().unwrap();
        assert!(!text_str.contains("cc_version"));
        assert!(text_str.contains("Hello Claude"));
    }
}
```

测试 fixture 文件放入 `src-tauri/tests/extensions/fixtures/` 目录：

- `fixtures/standard_claude_request.json` — 标准 Claude 请求
- `fixtures/request_with_images.json` — 含图片的请求
- `fixtures/deepseek_disguise_request.json` — DeepSeek disguise 请求

### Step 6: 注册测试模块

`src-tauri/tests/mod.rs`:

```rust
mod extensions;
```

`src-tauri/tests/extensions/mod.rs`:

```rust
mod registry_tests;
mod pipeline_tests;
```

### Step 7: 运行所有测试

```bash
cd src-tauri && cargo test extensions 2>&1
```

预期: 所有测试通过。

### Step 8: 提交

```bash
git add src-tauri/tests/
git commit -m "test(extensions): add registry, pipeline, isolation, and per-extension unit tests"
```
