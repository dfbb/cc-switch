use super::cache_control_normalize::CacheControlNormalize;
use super::cache_telemetry::CacheTelemetry;
use super::content_strip::ContentStrip;
use super::deferred_tools_restore::DeferredToolsRestore;
use super::fingerprint_strip::FingerprintStrip;
use super::fresh_session_sort::FreshSessionSort;
use super::identity_normalization::IdentityNormalization;
use super::image_strip::ImageStrip;
use super::messages_cache_breakpoint::MessagesCacheBreakpoint;
use super::microcompact_stability::MicrocompactStability;
use super::output_efficiency_rewrite::OutputEfficiencyRewrite;
use super::overage_warning::OverageWarning;
use super::prefix_diff::PrefixDiff;
use super::rate_limit_log::RateLimitLog;
use super::request_log::RequestLog;
use super::registry::ExtensionRegistry;
use super::smoosh_split::SmooshSplit;
use super::sort_stabilization::SortStabilization;
use super::thinking_display::ThinkingDisplay;
use super::tool_input_normalize::ToolInputNormalize;
use super::ttl_management::TtlManagement;
use super::ttl_tier_detect::TtlTierDetect;
use super::upstream_change_detection::UpstreamChangeDetection;
use super::usage_log::UsageLog;

/// 加载并注册所有已实现的 extension。
/// 每个 extension 在此函数中构造并 push 到 registry。
pub fn load_extensions() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::empty();

    // Request Extension Group 1
    registry
        .request_exts
        .push(Box::new(UpstreamChangeDetection::new()));
    registry
        .request_exts
        .push(Box::new(TtlTierDetect::new()));
    registry
        .request_exts
        .push(Box::new(OutputEfficiencyRewrite::new()));
    registry
        .request_exts
        .push(Box::new(FingerprintStrip::new()));
    registry.request_exts.push(Box::new(ImageStrip::new()));

    // Request Extension Group 2
    registry
        .request_exts
        .push(Box::new(SortStabilization::new()));
    registry
        .request_exts
        .push(Box::new(FreshSessionSort::new()));
    registry
        .request_exts
        .push(Box::new(IdentityNormalization::new()));
    registry
        .request_exts
        .push(Box::new(SmooshSplit::new()));
    registry
        .request_exts
        .push(Box::new(ContentStrip::new()));

    // Request Extension Group 3
    registry
        .request_exts
        .push(Box::new(ToolInputNormalize::new()));
    registry
        .request_exts
        .push(Box::new(MicrocompactStability::new()));
    registry
        .request_exts
        .push(Box::new(DeferredToolsRestore::new()));
    registry
        .request_exts
        .push(Box::new(ThinkingDisplay::new()));
    registry
        .request_exts
        .push(Box::new(CacheControlNormalize::new()));

    // Request Extension Group 4
    registry
        .request_exts
        .push(Box::new(MessagesCacheBreakpoint::new()));
    registry
        .request_exts
        .push(Box::new(TtlManagement::new()));
    registry
        .request_exts
        .push(Box::new(PrefixDiff::new()));

    // Multi-Hook Extension Group 1 — cache-telemetry (Request + ResponseStart + Stream)
    let ct_req = Box::new(CacheTelemetry::new());
    let ct_res = Box::new(CacheTelemetry::new());
    let ct_stream = Box::new(CacheTelemetry::new());
    registry.request_exts.push(ct_req);
    registry.response_exts.push(ct_res);
    registry.stream_exts.push(ct_stream);

    // Multi-Hook Extension Group 2 — overage_warning (Response + Stream), order 610
    let ow_res = Box::new(OverageWarning::new());
    let ow_stream = Box::new(OverageWarning::new());
    registry.response_exts.push(ow_res);
    registry.stream_exts.push(ow_stream);

    // Multi-Hook Extension Group 2 — usage_log (Stream only), order 650
    registry.stream_exts.push(Box::new(UsageLog::new()));

    // Multi-Hook Extension Group 2 — rate_limit_log (Request + Response), order 660
    let rl_req = Box::new(RateLimitLog::new());
    let rl_res = Box::new(RateLimitLog::new());
    registry.request_exts.push(rl_req);
    registry.response_exts.push(rl_res);

    // Multi-Hook Extension Group 2 — request_log (Request + Response + Stream), order 700
    let rql_req = Box::new(RequestLog::new());
    let rql_res = Box::new(RequestLog::new());
    let rql_stream = Box::new(RequestLog::new());
    registry.request_exts.push(rql_req);
    registry.response_exts.push(rql_res);
    registry.stream_exts.push(rql_stream);

    registry.finalize();
    log::info!(
        "[EXTENSIONS] 已加载 {} request / {} response / {} stream extensions",
        registry.request_exts.len(),
        registry.response_exts.len(),
        registry.stream_exts.len()
    );
    registry
}
