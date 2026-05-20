# Task 11: forwarder.rs 集成 — 请求管道

**可并行**: 否 — 依赖 Task 01, 02

**依赖**: Task 01, 02, 04

## 目标

在 `forward_with_retry_inner` 的 per-attempt 循环中嵌入 `registry.run_request_pipeline()`，位于 provider 选择之后、disguise sanitize 之前。

## 文件

- Modify: `src-tauri/src/proxy/forwarder.rs` — forward_with_retry_inner 函数
- Modify: `src-tauri/src/proxy/handler_context.rs` — RequestContext 可能需调整

**参考**: spec 第 2 节"集成点"

---

### Step 1: 在 forwarder.rs 文件头部添加 imports

```rust
use super::extensions::{ExtensionRegistry, RequestContext as ExtRequestContext, ExtensionMeta};
use crate::provider::ExtensionFilterConfig;
```

### Step 2: 添加 context 克隆辅助函数

在 `forwarder.rs` 的 impl 块中添加：

```rust
/// 从 body + headers 克隆构建 extension 的 RequestContext。
fn build_ext_request_context(
    body: &Value,
    headers: &axum::http::HeaderMap,
) -> ExtRequestContext {
    ExtRequestContext {
        body: body.clone(),
        headers: headers.clone(),
        meta: ExtensionMeta::new(),
    }
}
```

### Step 3: 在 per-attempt 循环中嵌入请求管道

在 `forward_with_retry_inner` 的 per-provider 循环中，在 `// 上限检查` 和熔断器检查之后、`rectifier_retried` 之前，添加：

```rust
// 保存原始请求用于 per-attempt 重建
let original_body = body.clone();
let original_headers = headers.clone();

for provider in providers.iter() {
    // ... 已有：上限检查、熔断器 ...

    // —— NEW: Extension 请求管道 ——
    let ext_config = provider.meta.get_extension_filter_config();
    let mut ext_ctx = build_ext_request_context(&original_body, &original_headers);

    let registry = &self.extension_registry;
    if let Some((status, body_bytes)) = registry
        .run_request_pipeline(&mut ext_ctx, &ext_config)
        .unwrap_or(None)
    {
        // Extension 拦截请求，返回合成响应
        log::info!(
            "[EXTENSIONS] 请求被拦截: status={status}, provider={}",
            provider.name
        );
        return Ok(ForwardResult {
            response: ProxyResponse::Synthetic {
                status,
                headers: axum::http::HeaderMap::new(),
                body: body_bytes,
            },
            provider: provider.clone(),
            connection_guard: None,
        });
    }

    // 管道通过后，使用（可能被修改的）body 继续
    // ... 已有：rectifier、forward 等 ...
```

### Step 4: 在 forward() 调用前使用 ext_ctx 的修改后 body

将 `forward()` 调用中的 `body` 参数替换为 `ext_ctx.body.clone()`（如果 ext_ctx 仍存活）：

注意：由于 ext_ctx 后续还要用于其他 provider，需要在这个 provider 的 forward 调用中使用修改后的 body。每个 attempt 从 original 开始，所以在 extension 修改之后、forward 之前使用 `ext_ctx.body`：

```rust
    // 使用 extension 修改后的 body（如果有修改）
    let current_body = ext_ctx.body.clone();
    let current_headers = ext_ctx.headers.clone();

    // ... 已有：rectifier + forward，使用 current_body 和 current_headers
```

### Step 5: 编译验证

```bash
cd src-tauri && cargo check 2>&1
```

修正可能的方法调用错误。

### Step 6: 提交

```bash
git add src-tauri/src/proxy/forwarder.rs
git commit -m "feat(extensions): integrate request pipeline into forwarder per-attempt loop"
```
