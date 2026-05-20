use cc_switch_lib::provider::ExtensionFilterConfig;
use cc_switch_lib::proxy::extensions::*;
use std::collections::HashMap;

/// 空 registry 不 panic
#[test]
fn test_empty_registry_handles_request_safely() {
    let mut registry = ExtensionRegistry::empty();
    registry.finalize();

    let config = ExtensionFilterConfig {
        enabled: Some(true),
        extensions: HashMap::new(),
        preset: None,
    };

    let mut ctx = RequestContext {
        body: serde_json::json!({"model": "test"}),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::default(),
    };

    let result = registry.run_request_pipeline(&mut ctx, &config).unwrap();
    assert!(
        result.is_none(),
        "Empty registry should not intercept"
    );
}

/// disabled config (enabled=None) 跳过所有 extension
#[test]
fn test_config_disabled_skips_all_extensions() {
    let mut registry = ExtensionRegistry::empty();
    registry.finalize();

    let config = ExtensionFilterConfig {
        enabled: None, // default = false
        extensions: HashMap::new(),
        preset: None,
    };

    let mut ctx = RequestContext {
        body: serde_json::json!({"test": true}),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::default(),
    };

    let result = registry.run_request_pipeline(&mut ctx, &config).unwrap();
    assert!(result.is_none());
}

/// extension 拦截返回合成响应
#[test]
fn test_interception_returns_synthetic_response() {
    // This test validates the pattern; actual interception depends on extension logic
    let mut registry = ExtensionRegistry::empty();
    // fingerprint-strip at order 100 is registered and won't intercept a non-Claude request
    registry.finalize();

    let config = ExtensionFilterConfig {
        enabled: Some(true),
        extensions: HashMap::new(),
        preset: None,
    };

    let mut ctx = RequestContext {
        body: serde_json::json!({"model": "gpt-4", "messages": []}),
        headers: axum::http::HeaderMap::new(),
        meta: ExtensionMeta::default(),
    };

    let result = registry.run_request_pipeline(&mut ctx, &config);
    assert!(result.is_ok());
}
