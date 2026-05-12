# T15: response_processor.rs + handlers.rs — 响应管道接入

> **并行：** ⚠️ 需等 T14 完成（依赖 `DeepseekContext`）。

**Goal:** 扩展 `handle_streaming` / `handle_non_streaming` / `process_response` 签名，接受 `deepseek_context: Option<&DeepseekContext>`；在流式路径插入 `wrap_sse_stream`；在非流式路径插入 `patch_non_streaming_response`（usage 日志之后）；更新 `handlers.rs` 所有调用点透传 context。

**Files:**
- Modify: `src-tauri/src/proxy/response_processor.rs`
- Modify: `src-tauri/src/proxy/handlers.rs`

---

- [ ] **Step 1: 写失败测试**

在 `response_processor.rs` 末尾追加（仅验证函数签名接受新参数，无需全量集成）：

```rust
// 步骤1：将下方 todo!() stub 插入 response_processor.rs
// 只需验证新签名可编译

pub async fn process_response(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    deepseek_context: Option<&crate::proxy::forwarder::DeepseekContext>,
) -> Result<Response, ProxyError> {
    todo!()
}
```

运行：

```bash
cd src-tauri && cargo check 2>&1 | tail -15
```

Expected: compile error — `process_response` duplicate definition or `todo!()` placeholder is unused.

- [ ] **Step 2: 实现 — 修改 `response_processor.rs`**

**2a. 导入 `DeepseekContext`**（在 `use` 区域顶部新增）：

```rust
use crate::proxy::forwarder::DeepseekContext;
```

**2b. 扩展 `handle_streaming` 签名**（当前定义在第 179 行）：

```rust
pub async fn handle_streaming(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    deepseek_context: Option<&DeepseekContext>,  // 新增
) -> Response {
```

在函数内找到（约第 221-222 行）：

```rust
let logged_stream =
    create_logged_passthrough_stream(stream, ctx.tag, Some(usage_collector), timeout_config);
```

其后紧接插入：

```rust
let final_stream: axum::body::Body = if let Some(dsk) = deepseek_context {
    axum::body::Body::from_stream(
        crate::proxy::providers::deepseek_anthropic::wrap_sse_stream(
            logged_stream,
            dsk.fake_model.clone(),
            dsk.effective_thinking_enabled,
        )
    )
} else {
    axum::body::Body::from_stream(logged_stream)
};
```

将原来的：

```rust
let body = axum::body::Body::from_stream(logged_stream);
```

改为：

```rust
let body = final_stream;
```

**2c. 扩展 `handle_non_streaming` 签名**（当前定义在第 235 行）：

```rust
pub async fn handle_non_streaming(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    deepseek_context: Option<&DeepseekContext>,  // 新增
) -> Result<Response, ProxyError> {
```

在函数内，找到 usage 日志结束后、构建响应之前（约第 317 行，`// 构建响应` 注释前），插入 patch 逻辑：

```rust
// patch + 序列化（deepseek_anthropic 非流式响应改写）
let body_bytes = if let Some(dsk) = deepseek_context {
    if let Ok(mut json_value) = serde_json::from_slice::<Value>(&body_bytes) {
        crate::proxy::providers::deepseek_anthropic::patch_non_streaming_response(
            &mut json_value,
            &dsk.fake_model,
            dsk.effective_thinking_enabled,
        );
        let new_bytes: bytes::Bytes = serde_json::to_vec(&json_value)
            .map(bytes::Bytes::from)
            .unwrap_or(body_bytes);
        // 剥离实体头（content-length 等会因 body 改变而失效）
        strip_entity_headers_for_rebuilt_body(&mut response_headers);
        new_bytes
    } else {
        body_bytes
    }
} else {
    body_bytes
};
```

注意：`body_bytes` 是上方通过 `read_decoded_body` 得到的 `bytes::Bytes`；需要用 `let body_bytes = body_bytes;` 使其可重绑定，或直接 shadowing。

**2d. 扩展 `process_response` 签名**（当前定义在第 333 行）：

```rust
pub async fn process_response(
    response: ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    parser_config: &UsageParserConfig,
    deepseek_context: Option<&DeepseekContext>,  // 新增
) -> Result<Response, ProxyError> {
    if is_sse_response(&response) {
        Ok(handle_streaming(response, ctx, state, parser_config, deepseek_context).await)
    } else {
        handle_non_streaming(response, ctx, state, parser_config, deepseek_context).await
    }
}
```

- [ ] **Step 3: 实现 — 修改 `handlers.rs`**

**3a. `handle_claude_messages`（约第 186 行）：** 将 `process_response` 调用改为传入 context：

```rust
// 原来：
process_response(response, &ctx, &state, &CLAUDE_PARSER_CONFIG).await

// 改为：
let deepseek_context = result.deepseek_context.as_ref();
process_response(response, &ctx, &state, &CLAUDE_PARSER_CONFIG, deepseek_context).await
```

注意：`result.deepseek_context` 需要在 `result.response` 提取之前保留或复制引用。改为：

```rust
ctx.provider = result.provider;
let api_format = result
    .claude_api_format
    .as_deref()
    .unwrap_or_else(|| get_claude_api_format(&ctx.provider))
    .to_string();
let deepseek_context = result.deepseek_context;   // 取 owned DeepseekContext
let response = result.response;

// ...（needs_transform check）...

process_response(response, &ctx, &state, &CLAUDE_PARSER_CONFIG, deepseek_context.as_ref()).await
```

**3b. 所有其它 `process_response` 调用点**（约第 489/543/597/662 行，OpenAI/Codex/Gemini 路径）：**补 `None`**：

```rust
process_response(response, &ctx, &state, &OPENAI_PARSER_CONFIG, None).await
process_response(response, &ctx, &state, &CODEX_PARSER_CONFIG, None).await
// 等等
```

- [ ] **Step 4: 运行验证通过**

```bash
cd src-tauri && cargo test 2>&1 | tail -10
```

Expected: 所有测试通过，无编译错误。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/proxy/response_processor.rs src-tauri/src/proxy/handlers.rs
git commit -m "feat(deepseek): wire wrap_sse_stream + patch_non_streaming_response into response pipeline"
```
