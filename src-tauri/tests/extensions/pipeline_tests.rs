use cc_switch_lib::provider::ExtensionFilterConfig;
use cc_switch_lib::proxy::extensions::*;
use std::collections::HashMap;

/// pipeline 执行后 meta 可跨阶段传递
#[test]
fn test_meta_persists_across_contexts() {
    let mut meta = ExtensionMeta::default();
    meta.set("test_key", serde_json::json!("test_value"));

    let rsc = ResponseStartContext {
        status: 200,
        headers: axum::http::HeaderMap::new(),
        upstream_headers: axum::http::HeaderMap::new(),
        meta: meta.clone(),
    };

    assert_eq!(
        rsc.meta.get("test_key").and_then(|v| v.as_str()),
        Some("test_value")
    );
}

/// StreamEventContext drop flag works
#[test]
fn test_stream_event_drop_flag() {
    let mut ctx = StreamEventContext {
        event_type: "test".into(),
        data: serde_json::json!({}),
        response_headers: axum::http::HeaderMap::new(),
        drop: false,
        meta: ExtensionMeta::default(),
        telemetry: TelemetryCollector::default(),
    };

    assert!(!ctx.drop);
    ctx.drop = true;
    assert!(ctx.drop);
}

/// 空 registry 的 stream pipeline 不 panic
#[test]
fn test_empty_registry_stream_pipeline() {
    let mut registry = ExtensionRegistry::empty();
    registry.finalize();

    let config = ExtensionFilterConfig {
        enabled: Some(true),
        extensions: HashMap::new(),
        preset: None,
    };

    let mut ctx = StreamEventContext {
        event_type: "message_start".into(),
        data: serde_json::json!({"message": {"model": "test"}}),
        response_headers: axum::http::HeaderMap::new(),
        drop: false,
        meta: ExtensionMeta::default(),
        telemetry: TelemetryCollector::default(),
    };

    let result = registry.run_stream_event_pipeline(&mut ctx, &config);
    assert!(result.is_ok());
}
