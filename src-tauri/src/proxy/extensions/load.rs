use super::fingerprint_strip::FingerprintStrip;
use super::image_strip::ImageStrip;
use super::output_efficiency_rewrite::OutputEfficiencyRewrite;
use super::registry::ExtensionRegistry;
use super::ttl_tier_detect::TtlTierDetect;
use super::upstream_change_detection::UpstreamChangeDetection;

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

    registry.finalize();
    log::info!(
        "[EXTENSIONS] 已加载 {} request / {} response / {} stream extensions",
        registry.request_exts.len(),
        registry.response_exts.len(),
        registry.stream_exts.len()
    );
    registry
}
