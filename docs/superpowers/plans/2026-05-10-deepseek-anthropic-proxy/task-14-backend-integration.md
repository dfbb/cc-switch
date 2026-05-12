# T14: 后端集成 — claude.rs + forwarder.rs

> **并行：** ⚠️ 需等 T03/T07/T11/T13 全部完成。

**Goal:** 将 deepseek_anthropic 接入现有代理管道：在 `claude.rs` 加分支、在 `forwarder.rs` 的 `forward()` 注入 `sanitize_request` 调用、扩展 `ForwardResult` 携带 `DeepseekContext`，并过滤禁用请求头。

**Files:**
- Modify: `src-tauri/src/proxy/providers/claude.rs`
- Modify: `src-tauri/src/proxy/forwarder.rs`

---

- [ ] **Step 1: 写失败测试**

`src-tauri/src/proxy/providers/claude.rs` 的改动无独立逻辑，测试在集成中覆盖（T18）。  
`forwarder.rs` 的 `DeepseekContext` 结构体有单元测试：

```rust
// 在 forwarder.rs 顶部或附近新增：
pub struct DeepseekContext {
    pub fake_model: String,
    pub effective_thinking_enabled: bool,
}

// ForwardResult 新增 deepseek_context 字段：
pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    pub claude_api_format: Option<String>,
    pub deepseek_context: Option<DeepseekContext>,  // 新增
}

// tests
#[cfg(test)]
mod tests_deepseek_context {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deepseek_context_fields() {
        let ctx = DeepseekContext {
            fake_model: "claude-opus-4-7".to_string(),
            effective_thinking_enabled: true,
        };
        assert_eq!(ctx.fake_model, "claude-opus-4-7");
        assert!(ctx.effective_thinking_enabled);
    }

    #[test]
    fn test_forward_result_with_context() {
        // Just verify struct compiles and deepseek_context is Option<DeepseekContext>
        // Can't construct a real ProxyResponse / Provider without a full server, so we just check type inference
        let ctx = DeepseekContext {
            fake_model: "claude-sonnet-4-6".to_string(),
            effective_thinking_enabled: false,
        };
        let _ = Option::<DeepseekContext>::Some(ctx);
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::forwarder::tests_deepseek_context 2>&1 | tail -15
```

Expected: FAIL (struct doesn't exist yet)

- [ ] **Step 3: 实现 — 修改 `claude.rs`**

文件: `src-tauri/src/proxy/providers/claude.rs`

在 `get_claude_api_format()` 函数的 `meta.apiFormat` 匹配分支中（约第 36-42 行），在已有的 `"anthropic"` / `"openai_chat"` / `"openai_responses"` / `"gemini_native"` 行之后新增：

```rust
"deepseek_anthropic" => "deepseek_anthropic",
```

在 `settings_config.api_format` 兼容路径（约第 51-56 行）的同等位置加同样分支。

**不**在 `claude_api_format_needs_transform()` 中加 `deepseek_anthropic`，保持返回 `false`（走 passthrough）。

- [ ] **Step 4: 实现 — 修改 `forwarder.rs`**

**4a. 新增 `DeepseekContext` 结构体**（在 `ForwardResult` 定义前）：

```rust
pub struct DeepseekContext {
    pub fake_model: String,
    pub effective_thinking_enabled: bool,
}
```

**4b. 扩展 `ForwardResult`**（在现有 `pub struct ForwardResult` 定义上追加字段）：

```rust
pub struct ForwardResult {
    pub response: ProxyResponse,
    pub provider: Provider,
    pub claude_api_format: Option<String>,
    pub deepseek_context: Option<DeepseekContext>,
}
```

**4c. 修复所有 `ForwardResult { ... }` 构造点**（搜索 `ForwardResult {`，约出现在第 259/395/588 行的 `Ok(ForwardResult {` 处）：

每处补上 `deepseek_context: None`，例如：

```rust
return Ok(ForwardResult {
    response,
    provider: provider.clone(),
    claude_api_format,
    deepseek_context: None,
});
```

**4d. 在 `forward()` 函数中注入 `sanitize_request`**

在 `forward()` 函数内，找到 `let mut mapped_body = normalize_thinking_type(mapped_body);` 之后、构造 `ordered_headers` 之前，新增：

```rust
// deepseek_anthropic：sanitize 请求体并收集 context
let deepseek_sanitize_result = if matches!(
    super::providers::get_claude_api_format(provider),
    "deepseek_anthropic"
) {
    Some(crate::proxy::providers::deepseek_anthropic::sanitize_request(&mut mapped_body))
} else {
    None
};
```

**4e. 修改 `forward()` 返回类型及返回点**

函数签名从 `-> Result<(ProxyResponse, Option<String>), ProxyError>` 改为：

```rust
async fn forward(
    &self,
    app_type: &AppType,
    provider: &Provider,
    endpoint: &str,
    body: &Value,
    headers: &axum::http::HeaderMap,
    extensions: &Extensions,
    adapter: &dyn ProviderAdapter,
) -> Result<(ProxyResponse, Option<String>, Option<DeepseekContext>), ProxyError>
```

将 `forward()` 内唯一的成功返回点（发送请求后构造结果的地方，搜索 `Ok((response, claude_api_format))`）改为：

```rust
let deepseek_ctx = deepseek_sanitize_result.map(|r| DeepseekContext {
    fake_model: r.fake_model,
    effective_thinking_enabled: r.effective_thinking_enabled,
});
Ok((response, claude_api_format, deepseek_ctx))
```

**4f. 修复 `forward_with_retry` 中所有调用 `self.forward(...)` 的解包**

搜索 `Ok((response, claude_api_format)) =>` 约 3 处（第 208/339/539 行），改为：

```rust
Ok((response, claude_api_format, deepseek_context)) => {
    // ... 现有逻辑 ...
    return Ok(ForwardResult {
        response,
        provider: provider.clone(),
        claude_api_format,
        deepseek_context,
    });
}
```

**4g. 过滤 `anthropic-beta` 等请求头**

在构造 `ordered_headers`（搜索 `ordered_headers` 的构建循环，约 1060 行附近）时，在 `deepseek_anthropic` 下跳过黑名单头：

```rust
const DEEPSEEK_HEADER_BLACKLIST: &[&str] = &[
    "anthropic-beta",
    "anthropic-dangerous-direct-browser-access",
];

let is_deepseek = matches!(
    super::providers::get_claude_api_format(provider),
    "deepseek_anthropic"
);

// 在循环内（构造 ordered_headers 的 for (k, v) in headers 循环里）：
for (k, v) in headers.iter() {
    let name = k.as_str().to_ascii_lowercase();
    if is_deepseek && DEEPSEEK_HEADER_BLACKLIST.iter().any(|b| *b == name) {
        continue;
    }
    ordered_headers.push((k.clone(), v.clone()));
}
```

- [ ] **Step 5: 运行验证通过**

```bash
cd src-tauri && cargo test 2>&1 | tail -15
```

Expected: 所有测试通过，无编译错误。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/proxy/providers/claude.rs src-tauri/src/proxy/forwarder.rs
git commit -m "feat(deepseek): wire sanitize_request into forwarder + DeepseekContext forwarding"
```
