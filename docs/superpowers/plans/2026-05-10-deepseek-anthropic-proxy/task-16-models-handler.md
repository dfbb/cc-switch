# T16: /v1/models 路由 + handle_claude_models handler

> **并行：** ⚠️ 需等 T14 完成（依赖 `get_claude_api_format` 分支已加入 deepseek_anthropic）。

**Goal:** 新增 `/v1/models` + `/claude/v1/models` 路由；实现 `handle_claude_models` handler；抽出 `select_models_endpoint_provider` 与 `build_deepseek_disguised_models_payload` 共用函数；同步修改 `handle_claude_desktop_models` 新增 deepseek_anthropic 分支。

**Files:**
- Modify: `src-tauri/src/proxy/handlers.rs`
- Modify: `src-tauri/src/proxy/server.rs`

---

- [ ] **Step 1: 写失败测试**

在 `handlers.rs` 末尾新增（仅测试 `build_deepseek_disguised_models_payload`，无需完整 state）：

```rust
pub(crate) fn build_deepseek_disguised_models_payload(provider: &Provider) -> serde_json::Value {
    todo!()
}

#[cfg(test)]
mod tests_models {
    use super::*;
    use serde_json::json;
    use crate::provider::Provider;

    fn make_provider_with_env(env: serde_json::Value) -> Provider {
        let mut p = Provider::default();
        p.settings_config = json!({"env": env});
        p
    }

    #[test]
    fn test_disguised_payload_basic() {
        let provider = make_provider_with_env(json!({
            "ANTHROPIC_MODEL": "claude-opus-4-7",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-7",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-6",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-sonnet-4-6",
        }));
        let payload = build_deepseek_disguised_models_payload(&provider);
        let data = payload["data"].as_array().unwrap();
        // Dedup: only 2 unique model names
        assert_eq!(data.len(), 2);
        assert_eq!(data[0]["id"], "claude-opus-4-7");
        assert_eq!(data[1]["id"], "claude-sonnet-4-6");
    }

    #[test]
    fn test_disguised_payload_empty_env_fallback() {
        let provider = make_provider_with_env(json!({}));
        let payload = build_deepseek_disguised_models_payload(&provider);
        let data = payload["data"].as_array().unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], "claude-sonnet-4-6");
    }

    #[test]
    fn test_disguised_payload_deduplication_order() {
        // ANTHROPIC_MODEL appears first in MODEL_ENV_KEYS, so it leads
        let provider = make_provider_with_env(json!({
            "ANTHROPIC_MODEL": "claude-sonnet-4-6",
            "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-7",
        }));
        let payload = build_deepseek_disguised_models_payload(&provider);
        let data = payload["data"].as_array().unwrap();
        assert_eq!(data[0]["id"], "claude-sonnet-4-6");
        assert_eq!(data[1]["id"], "claude-opus-4-7");
    }

    #[test]
    fn test_disguised_payload_skips_empty_string() {
        let provider = make_provider_with_env(json!({
            "ANTHROPIC_MODEL": "",
            "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-6",
        }));
        let payload = build_deepseek_disguised_models_payload(&provider);
        let data = payload["data"].as_array().unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], "claude-sonnet-4-6");
    }

    #[test]
    fn test_disguised_payload_item_shape() {
        let provider = make_provider_with_env(json!({
            "ANTHROPIC_MODEL": "claude-opus-4-7",
        }));
        let payload = build_deepseek_disguised_models_payload(&provider);
        let item = &payload["data"][0];
        assert_eq!(item["type"], "model");
        assert_eq!(item["id"], "claude-opus-4-7");
        assert_eq!(item["display_name"], "claude-opus-4-7");
    }
}
```

- [ ] **Step 2: 运行验证失败**

```bash
cd src-tauri && cargo test deepseek_anthropic::handlers::tests_models 2>&1 | tail -15
```

- [ ] **Step 3: 实现 `build_deepseek_disguised_models_payload` + `select_models_endpoint_provider`**

在 `handlers.rs` 适当位置（建议放在 `handle_claude_desktop_models` 函数前）新增：

```rust
use std::collections::HashSet;

const MODEL_ENV_KEYS: &[&str] = &[
    "ANTHROPIC_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
];

pub(crate) fn build_deepseek_disguised_models_payload(provider: &Provider) -> serde_json::Value {
    let env_map = provider
        .settings_config
        .get("env")
        .and_then(|v| v.as_object());
    let mut disguised: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for key in MODEL_ENV_KEYS {
        if let Some(s) = env_map
            .and_then(|m| m.get(*key))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            if seen.insert(s.to_string()) {
                disguised.push(s.to_string());
            }
        }
    }
    if disguised.is_empty() {
        disguised.push("claude-sonnet-4-6".to_string());
    }
    let data: Vec<serde_json::Value> = disguised
        .into_iter()
        .map(|name| serde_json::json!({ "type": "model", "id": name, "display_name": name }))
        .collect();
    serde_json::json!({"data": data})
}

async fn select_models_endpoint_provider(
    state: &ProxyState,
    app_type_str: &'static str,
) -> Result<Provider, ProxyError> {
    let providers = state
        .provider_router
        .select_providers(app_type_str)
        .await
        .map_err(|e| match e {
            crate::error::AppError::AllProvidersCircuitOpen => ProxyError::AllProvidersCircuitOpen,
            crate::error::AppError::NoProvidersConfigured => ProxyError::NoProvidersConfigured,
            other => ProxyError::DatabaseError(other.to_string()),
        })?;
    providers.into_iter().next().ok_or(ProxyError::NoAvailableProvider)
}
```

- [ ] **Step 4: 实现 `handle_claude_models`**

在 `handlers.rs` 中新增：

```rust
pub async fn handle_claude_models(
    State(state): State<ProxyState>,
    _headers: axum::http::HeaderMap,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let provider = match select_models_endpoint_provider(&state, "claude").await {
        Ok(p) => p,
        Err(e) => return e.into_response(),
    };
    if super::providers::get_claude_api_format(&provider) == "deepseek_anthropic" {
        return axum::Json(build_deepseek_disguised_models_payload(&provider)).into_response();
    }
    axum::http::StatusCode::NOT_FOUND.into_response()
}
```

- [ ] **Step 5: 修改 `handle_claude_desktop_models`**

将现有函数（约第 92-106 行）改为：

```rust
pub async fn handle_claude_desktop_models(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
) -> Result<axum::Json<serde_json::Value>, ProxyError> {
    validate_claude_desktop_gateway_auth(&state, &headers)?;
    let provider = select_models_endpoint_provider(&state, "claude-desktop").await?;

    if super::providers::get_claude_api_format(&provider) == "deepseek_anthropic" {
        return Ok(axum::Json(build_deepseek_disguised_models_payload(&provider)));
    }

    let response = crate::claude_desktop_config::model_list_response(&provider)
        .map_err(|e| ProxyError::ConfigError(e.to_string()))?;
    Ok(axum::Json(response))
}
```

- [ ] **Step 6: 新增路由（`server.rs`）**

在 `build_router()` 中，在 `/v1/messages` 路由附近新增：

```rust
.route("/v1/models", get(handlers::handle_claude_models))
.route("/claude/v1/models", get(handlers::handle_claude_models))
```

- [ ] **Step 7: 运行验证通过**

```bash
cd src-tauri && cargo test 2>&1 | tail -10
```

Expected: 所有测试通过，含 `tests_models` 下所有用例。

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src/proxy/handlers.rs src-tauri/src/proxy/server.rs
git commit -m "feat(deepseek): add /v1/models route + handle_claude_models + disguised model list"
```
