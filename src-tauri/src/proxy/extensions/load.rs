use super::registry::ExtensionRegistry;

/// 加载并注册所有已实现的 extension。
/// 每个 extension 在此函数中构造并 push 到 registry。
///
/// v1 阶段：extension 为占位实现（空 body/no-op），
/// 实际逻辑在后续 task 中逐个翻译自 JS。
pub fn load_extensions() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::empty();

    // 所有 extension 在此注册（v1 占位，后续 task 替换为实际实现）
    // 示例：
    // registry.request_exts.push(Box::new(FingerprintStrip::new()));

    registry.finalize();
    log::info!(
        "[EXTENSIONS] 已加载 {} request / {} response / {} stream extensions",
        registry.request_exts.len(),
        registry.response_exts.len(),
        registry.stream_exts.len()
    );
    registry
}
