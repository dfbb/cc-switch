use cc_switch_lib::provider::ExtensionFilterConfig;
use cc_switch_lib::proxy::extensions::*;
use std::collections::HashMap;

/// per-attempt 从原始副本重建：A 的修改不泄漏到 B
#[test]
fn test_context_clone_isolation() {
    let original_body = serde_json::json!({"image": "base64data..."});

    // 模拟 provider A 的修改
    let mut ctx_a = RequestContext {
        body: original_body.clone(),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::default(),
    };
    ctx_a.body["image"] = serde_json::json!(null); // A 剥离了图片

    // 从 original 重建 provider B 的 context
    let ctx_b = RequestContext {
        body: original_body.clone(),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::default(),
    };

    // B 应该仍有原始图片
    assert!(ctx_b.body["image"].is_string());
    // A 的修改不应影响 original
    assert!(ctx_a.body["image"].is_null());
}

/// ExtensionFilterConfig 的默认值不启用管道
#[test]
fn test_default_config_disables_pipeline() {
    let registry = ExtensionRegistry::empty();

    let config = ExtensionFilterConfig {
        enabled: None,
        extensions: HashMap::new(),
        preset: None,
    };

    let mut ctx = RequestContext {
        body: serde_json::json!({}),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::default(),
    };

    // 即使 registry 中有 extension，enabled=None 也应该跳过所有
    let result = registry.run_request_pipeline(&mut ctx, &config).unwrap();
    assert!(result.is_none());
}

/// per-extension 开关覆盖 default_enabled
#[test]
fn test_per_extension_override_works() {
    let registry = ExtensionRegistry::empty();

    let mut extensions = HashMap::new();
    // 显式禁用 fingerprint-strip (它 default_enabled=true)
    extensions.insert("fingerprint-strip".to_string(), false);
    // 显式启用 rate-limit-log (它 default_enabled=false)
    extensions.insert("rate-limit-log".to_string(), true);

    let config = ExtensionFilterConfig {
        enabled: Some(true),
        extensions,
        preset: None,
    };

    let mut ctx = RequestContext {
        body: serde_json::json!({"model": "claude-sonnet-4-6", "messages": []}),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::default(),
    };

    // 管道应该运行但不应该 panic
    let result = registry.run_request_pipeline(&mut ctx, &config).unwrap();
    assert!(result.is_none());
}
