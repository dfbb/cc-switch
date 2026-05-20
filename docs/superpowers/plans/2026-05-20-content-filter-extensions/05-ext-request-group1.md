# Task 05: Request Extension Group 1 — ttl-tier-detect, upstream-change-detection, output-efficiency-rewrite, fingerprint-strip, image-strip

**可并行**: 是 — 与 Task 06-10 全部并行

**依赖**: Task 01（traits/types）

**范围**: 5 个 extension（order 50-150），均只实现 `RequestExtension`

## 文件

- Create: `src-tauri/src/proxy/extensions/ttl_tier_detect.rs`
- Create: `src-tauri/src/proxy/extensions/upstream_change_detection.rs`
- Create: `src-tauri/src/proxy/extensions/output_efficiency_rewrite.rs`
- Create: `src-tauri/src/proxy/extensions/fingerprint_strip.rs`
- Create: `src-tauri/src/proxy/extensions/image_strip.rs`
- Modify: `src-tauri/src/proxy/extensions/load.rs`
- Modify: `src-tauri/src/proxy/extensions/mod.rs`

**JS 源码参考**: `/Users/dfbb/Sites/myidea/ccswitch/3rd/claude-code-cache-fix/proxy/extensions/`

---

### Step 1: 翻译 ttl_tier_detect (order 75)

**JS 源文件**: `ttl-tier-detect.mjs`

从请求体 `messages[].content[].cache_control.ttl` 检测 TTL 层级（5m/1h），写入 `ctx.meta["_ttlTier"]`。

### Step 2: 翻译 upstream_change_detection (order 50)

**JS 源文件**: `upstream-change-detection.mjs`

对请求结构生成指纹（位置/数量/哈希），持久化基线到 `~/.claude/upstream-baseline.json`，变更时写入 `~/.claude/upstream-changes.jsonl`。env var `CACHE_FIX_UPSTREAM_DETECTION=1` 激活。

### Step 3: 翻译 output_efficiency_rewrite (order 90)

**JS 源文件**: `output-efficiency-rewrite.mjs`

替换系统 prompt 中 "# Output efficiency" 章节。env var `CACHE_FIX_OUTPUT_EFFICIENCY_REPLACEMENT` 提供替换文本。

### Step 4: 翻译 fingerprint_strip (order 100)

**JS 源文件**: `fingerprint-strip.mjs`

从 `messages[0]` 的系统 prompt 中移除 `cc_version` 指纹，用真实用户消息中的文本重新计算。

### Step 5: 翻译 image_strip (order 150)

**JS 源文件**: `image-strip.mjs`

多 pass 剥离 base64 图片：Pass 0 保留最后 N 张、Pass 1 尺寸上限、Pass 2 请求体大小上限、Pass 3 Lanczos 缩放。env var `CACHE_FIX_IMAGE_GUARD=1` 激活。

### Step 6: 每个 extension 的标准结构

每个 `.rs` 文件遵循此模板（以 fingerprint_strip 为例）：

```rust
// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/fingerprint-strip.mjs
// 翻译: 2026-05-20

use super::traits::*;
use super::context::*;
use super::errors::ExtensionError;

pub struct FingerprintStrip;

impl FingerprintStrip {
    pub fn new() -> Self { Self }
}

impl Extension for FingerprintStrip {
    fn name(&self) -> &str { "fingerprint-strip" }
    fn order(&self) -> u32 { 100 }
    fn default_enabled(&self) -> bool { true }
}

impl RequestExtension for FingerprintStrip {
    fn on_request(&self, ctx: &mut RequestContext)
        -> Result<Option<(u16, Vec<u8>)>, ExtensionError>
    {
        // TODO: 翻译 JS 核心逻辑
        // 从 messages[0] 中查找并移除 cc_version 指纹
        Ok(None)
    }
}
```

> 实际逻辑由 Claude Code 从 JS 翻译为 Rust。每个 extension 的核心业务逻辑在 Step 中由 LLM 完成翻译。

### Step 7: 在 load.rs 中注册

```rust
// 在 load_extensions() 函数中添加：
registry.request_exts.push(Box::new(ttl_tier_detect::TtlTierDetect::new()));
registry.request_exts.push(Box::new(upstream_change_detection::UpstreamChangeDetection::new()));
registry.request_exts.push(Box::new(output_efficiency_rewrite::OutputEfficiencyRewrite::new()));
registry.request_exts.push(Box::new(fingerprint_strip::FingerprintStrip::new()));
registry.request_exts.push(Box::new(image_strip::ImageStrip::new()));
```

### Step 8: 在 mod.rs 中声明模块

```rust
pub mod ttl_tier_detect;
pub mod upstream_change_detection;
pub mod output_efficiency_rewrite;
pub mod fingerprint_strip;
pub mod image_strip;
```

### Step 9: 编译 + 提交

```bash
cd src-tauri && cargo check 2>&1
git add src-tauri/src/proxy/extensions/
git commit -m "feat(extensions): add request group1 — ttl-tier-detect, upstream-change-detection, output-efficiency-rewrite, fingerprint-strip, image-strip"
```
