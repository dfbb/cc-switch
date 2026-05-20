# Task 12: Response Pipeline Integration — response_processor.rs + handlers.rs

**可并行**: 否 — 依赖 Task 01, 02

**依赖**: Task 01, 02, 04

## 目标

在响应路径中嵌入 extension 管道：
1. `handle_claude_transform` 的转换路径也走 extension
2. 透传路径嵌入 `run_response_start_pipeline` / `run_response_pipeline` / `run_stream_event_pipeline`
3. Entity header 重建

## 文件

- Modify: `src-tauri/src/proxy/response_processor.rs` — 添加 pipeline 接入点
- Modify: `src-tauri/src/proxy/handlers.rs` — handle_claude_transform 路径接入
- Modify: `src-tauri/src/proxy/server.rs` — 传递 extension_registry

---

### Step 1: 在 response_processor.rs 中添加扩展管道包装函数

在文件顶部添加 imports：

```rust
use super::extensions::{
    ExtensionRegistry,
    ResponseStartContext as ExtResponseStartContext,
    ResponseContext as ExtResponseContext,
    StreamEventContext as ExtStreamEventContext,
    ExtensionMeta, TelemetryCollector,
};
use crate::provider::ExtensionFilterConfig;
```

新增 `process_response_with_extensions()` 函数：

```rust
/// 在现有 process_response 逻辑前后嵌入 extension 管道。
pub async fn process_response_with_extensions(
    state: &ProxyState,
    upstream_response: ProxyResponse,
    extension_config: &ExtensionFilterConfig,
    ext_meta: ExtensionMeta,
) -> Response {
    let registry = &state.extension_registry;

    // Step 1: ResponseStart pipeline
    {
        let mut rsc = ExtResponseStartContext {
            status: upstream_response.status(),
            headers: upstream_response.headers().clone(),
            upstream_headers: upstream_response.headers().clone(),
            meta: ext_meta.clone(),
        };
        let _ = registry.run_response_start_pipeline(&mut rsc, extension_config);
    }

    let is_stream = is_sse_response(&upstream_response);

    if is_stream {
        // 流式路径：包装 SSE 流以注入 extension 处理
        // (见 Step 2)
        handle_streaming_with_extensions(state, upstream_response, extension_config, ext_meta).await
    } else {
        // 非流式路径：读取完整 body → 运行 response pipeline → 重建 entity headers
        handle_non_streaming_with_extensions(state, upstream_response, extension_config, ext_meta).await
    }
}
```

### Step 2: 实现流式扩展处理

```rust
async fn handle_streaming_with_extensions(
    state: &ProxyState,
    upstream_response: ProxyResponse,
    extension_config: &ExtensionFilterConfig,
    mut ext_meta: ExtensionMeta,
) -> Response {
    let registry = Arc::clone(&state.extension_registry);
    let config = extension_config.clone();
    let resp_headers = upstream_response.headers().clone();

    // 包装原始 SSE 流
    let stream = upstream_response.into_stream();
    let wrapped_stream = stream.map(move |chunk_result| {
        match chunk_result {
            Ok(bytes) => {
                // 解析 SSE line → event_type + data
                // 构造 StreamEventContext → run_stream_event_pipeline
                // 检查 ctx.drop → 跳过/继续
                // TODO: 实际 SSE 解析
                Ok(bytes) // 占位
            }
            Err(e) => Err(e),
        }
    });

    // 构建流式响应
    axum::response::Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .body(axum::body::Body::from_stream(wrapped_stream))
        .unwrap()
}
```

### Step 3: 实现非流式扩展处理 + entity header 重建

```rust
async fn handle_non_streaming_with_extensions(
    state: &ProxyState,
    upstream_response: ProxyResponse,
    extension_config: &ExtensionFilterConfig,
    mut ext_meta: ExtensionMeta,
) -> Response {
    let registry = &state.extension_registry;
    let status = upstream_response.status();
    let mut headers = upstream_response.headers().clone();

    // 读取完整 body
    let body_bytes = upstream_response.into_body_bytes().await.unwrap_or_default();

    // 运行 response pipeline
    let mut rc = ExtResponseContext {
        status,
        headers: headers.clone(),
        body: body_bytes,
        meta: ext_meta,
    };
    let _ = registry.run_response_pipeline(&mut rc, extension_config);

    // 重建 entity headers（body 可能已被修改）
    let final_body = rc.body;
    let mut final_headers = rc.headers;
    final_headers.remove("content-length");
    final_headers.remove("content-encoding");
    final_headers.remove("transfer-encoding");

    let mut response = axum::response::Response::new(axum::body::Body::from(final_body));
    *response.status_mut() = axum::http::StatusCode::from_u16(status).unwrap_or(axum::http::StatusCode::OK);
    *response.headers_mut() = final_headers;
    response
}
```

### Step 4: 在 handlers.rs 的转换路径接入

在 `handle_claude_transform` 返回之前，将响应包装为 extension pipeline：

对于转换后的 Anthropic 格式响应（流式或非流式），在返回前运行 `process_response_with_extensions()`。

### Step 5: 编译验证

```bash
cd src-tauri && cargo check 2>&1
```

### Step 6: 提交

```bash
git add src-tauri/src/proxy/response_processor.rs src-tauri/src/proxy/handlers.rs
git commit -m "feat(extensions): integrate response pipeline into response_processor and handlers"
```
