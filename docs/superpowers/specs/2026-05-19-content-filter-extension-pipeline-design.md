# 内容过滤扩展管道设计

**日期**: 2026-05-19
**状态**: 待实施
**来源**: 复用 `claude-code-cache-fix` v3.6.1 的 23 个 extension

## 1. 概述

在 CC Switch Rust 代理中实现通用内容过滤扩展管道，位于 DeepSeek（Claude disguise）适配层之前。所有 extension 逻辑复用自 [claude-code-cache-fix](https://github.com/anthropics/claude-code-cache-fix)，从 JS 手动翻译为 Rust。

### 目标

- 对所有 Claude provider 请求执行通用内容过滤（缓存稳定、身份标准化、图片剥离等）
- DeepSeek disguise 作为 provider 特定后处理，在通用管道之后运行
- 用户可在 Claude endpoint 配置中独立开关每个 extension
- 与上游 `claude-code-cache-fix` 保持功能同步（大版本发布时手动翻译）

### 非目标

- 不做代码生成器。翻译过程在 Claude Code 中手动完成
- 不替换已有的 `deepseek_anthropic/` sanitize 逻辑
- 不改变现有非 DeepSeek provider 的行为（除非用户主动启用扩展）

## 2. 架构

### 管道分层

```
Claude Code CLI → CC Switch Proxy
  → [Extension 管道]（所有 provider，通用过滤）
    → [DeepSeek 适配层]（仅 disguise，provider 特定）
      → Upstream API
```

两层独立运行，各有开关：

- **Extension 管道**：缓存修复、身份标准化、图片剥离、TTL 管理等
- **DeepSeek 适配层**：移除不支持的 block、修复 tool 顺序、重建 thinking、模型名映射

### 集成点

在 `forwarder.rs` 和 `response_processor.rs` 中嵌入：

```
1. 解析请求 → RequestContext
2. [NEW] registry.run_request_pipeline(&mut ctx)
   → 返回 Some((status, body)) 则短路返回
3. [仅 disguise] request_sanitizer::sanitize_request()
4. forwarder.forward() — 发送到上游
5. [NEW] registry.run_response_start_pipeline(&mut ctx)
6. [NEW] [stream] registry.run_stream_event_pipeline(&mut ctx) × N
   [NEW] [non-stream] registry.run_response_pipeline(&mut ctx)
7. 返回给客户端
```

## 3. Trait 设计

### 基础 trait

```rust
/// 所有 extension 必须实现
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn order(&self) -> u32;
    fn enabled(&self) -> bool;
}
```

### 三 trait 分离

```rust
pub trait RequestExtension: Extension {
    /// 请求预处理，返回 Some((status, body)) 则拦截请求
    fn on_request(&self, ctx: &mut RequestContext)
        -> Result<Option<(u16, Vec<u8>)>, ExtensionError>;
}

pub trait ResponseExtension: Extension {
    /// 响应状态/headers 到达时调用
    fn on_response_start(&self, ctx: &mut ResponseStartContext)
        -> Result<(), ExtensionError>;
    /// 完整响应体就绪后调用
    fn on_response(&self, ctx: &mut ResponseContext)
        -> Result<(), ExtensionError>;
}

pub trait StreamExtension: Extension {
    /// 处理单个 SSE 事件，ctx.drop = true 则丢弃此事件
    fn on_stream_event(&self, ctx: &mut StreamEventContext)
        -> Result<(), ExtensionError>;
}
```

### Context 类型

| Context | 阶段 | 可修改 |
|---------|------|--------|
| `RequestContext` | 请求前 | body, headers, meta |
| `ResponseStartContext` | 响应 headers | headers（只读）, meta |
| `ResponseContext` | 响应体 | body, headers, meta |
| `StreamEventContext` | SSE 事件 | data, drop, meta, telemetry |

Extension 只实现需要的 trait。例如 `fingerprint-strip` 只需 `RequestExtension`。

## 4. Extension 注册和执行

### ExtensionRegistry

```rust
pub struct ExtensionRegistry {
    extensions: Vec<Box<dyn Extension>>,
    request_exts: Vec<usize>,    // 实现 RequestExtension 的索引
    response_exts: Vec<usize>,   // 实现 ResponseExtension 的索引
    stream_exts: Vec<usize>,     // 实现 StreamExtension 的索引
}
```

- 代理启动时加载 `extensions/config.json`
- 按 order 排序，根据 enabled 过滤
- 支持文件监控热重载（watch config.json 变化）
- 单个 extension 错误不中断整个管道

### 管道执行

```rust
impl ExtensionRegistry {
    // 每个管道方法：遍历索引，调用对应 trait 方法，catch 错误
    pub fn run_request_pipeline(&self, ctx: &mut RequestContext)
        -> Result<Option<(u16, Vec<u8>)>, ExtensionError>;
    pub fn run_response_start_pipeline(&self, ctx: &mut ResponseStartContext)
        -> Result<(), ExtensionError>;
    pub fn run_response_pipeline(&self, ctx: &mut ResponseContext)
        -> Result<(), ExtensionError>;
    pub fn run_stream_event_pipeline(&self, ctx: &mut StreamEventContext)
        -> Result<(), ExtensionError>;
}
```

## 5. Extension 清单（23 个）

### 缓存稳定性（5 个，默认启用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| fingerprint-strip | 100 | Request | 从 messages[0] 移除 cc_version，稳定 fingerprint |
| sort-stabilization | 200 | Request | skills/deferred-tools/tools 字母序排列 |
| fresh-session-sort | 250 | Request | 新会话：重排 scattered blocks 到 messages[0] |
| cache-control-normalize | 400 | Request | 规范化 cache_control 标记位置 |
| messages-cache-breakpoint | 410 | Request | 注入断点 #3 cache_control 标记 |

### 身份标准化（4 个，默认启用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| identity-normalization | 300 | Request | 清理 SessionStart 标记、Last active、system_reminder |
| smoosh-split | 320 | Request | 分离粘合在 tool_result 中的系统提醒 |
| content-strip | 330 | Request | 移除 "Continue" 尾随文本、记账提醒 |
| tool-input-normalize | 340 | Request | 按 schema 排序 tool_use input 字段 |

### 图像/内容处理（2 个，默认启用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| image-strip | 150 | Request | 剥离 base64 图片数据（支持 keep-last/dim-cap 等策略） |
| thinking-display | 360 | Request | Opus 4.7 请求注入 thinking.display |

### TTL/缓存管理（2 个，默认启用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| ttl-tier-detect | 75 | ResponseStart | 检测请求的 TTL 层级（5m/1h） |
| ttl-management | 500 | Request | 根据 TTL 层级注入正确的 cache_control |

### 会话持久化（1 个，默认启用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| deferred-tools-restore | 350 | Request | 跨会话持久化和恢复 deferred-tools 块 |

### 监控/诊断（2 个，默认启用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| cache-telemetry | 600 | Stream | 从 SSE 流提取缓存命中率并持久化 |
| overage-warning | 610 | ResponseStart | 超量阈值告警，写入 stderr + JSONL |

### 检测/诊断（6 个，默认禁用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| upstream-change-detection | 50 | Request | 上游请求结构指纹变更检测 |
| microcompact-stability | 350 | Request | 微压缩 sentinel 检测和标准化 |
| rate-limit-log | 660 | ResponseStart | 429 限流事件日志 |
| request-log | 700 | ResponseStart | 请求计时 NDJSON 日志 |
| output-efficiency-rewrite | — | Request | 重写系统 prompt 的效率章节 |
| prefix-diff | — | Request | 请求前缀差异诊断 |

## 6. 与 DeepSeek Disguise 的兼容

### 执行顺序

Extension 管道在所有 provider 上运行。DeepSeek disguise 的 `request_sanitizer::sanitize_request()` 仅在 `api_format == "deepseek_anthropic"` 时运行，位于 extension 管道之后。

### 关键协同

- **image-strip** 先剥离图片 → disguise 不需要处理图片相关清理
- **cache-control 规范化**后的请求在 disguise 层也能正确缓存
- **thinking-display** 必须在 disguise 之前执行，disguise 需识别已注入的 thinking 标记
- 模型名映射：extension 用 `claude-*` 名，disguise 的 `model_mapping.rs` 再映射到 `deepseek-*`

### 潜在冲突

- `thinking-display` 注入的 thinking 标记与 disguise 的 thinking 重建可能冲突。解决方案：disguise 检测到已有的 thinking 标记则跳过重建
- JSON 序列化开销：从 bytes 解析一次，pipe 中复用 `serde_json::Value` 引用

## 7. 文件结构

```
src-tauri/src/proxy/
├── extensions/
│   ├── mod.rs                        # 注册所有模块
│   ├── traits.rs                     # Extension / Request / Response / Stream trait
│   ├── context.rs                    # Context 类型
│   ├── registry.rs                   # ExtensionRegistry
│   ├── errors.rs                     # ExtensionError
│   ├── config.json                   # Extension 配置（order + enabled）
│   │
│   ├── fingerprint_strip.rs          # order 100
│   ├── sort_stabilization.rs         # order 200
│   ├── fresh_session_sort.rs         # order 250
│   ├── identity_normalization.rs     # order 300
│   ├── smoosh_split.rs              # order 320
│   ├── content_strip.rs             # order 330
│   ├── tool_input_normalize.rs       # order 340
│   ├── image_strip.rs                # order 150
│   ├── thinking_display.rs           # order 360
│   ├── ttl_tier_detect.rs            # order 75
│   ├── ttl_management.rs             # order 500
│   ├── deferred_tools_restore.rs     # order 350
│   ├── cache_control_normalize.rs    # order 400
│   ├── messages_cache_breakpoint.rs  # order 410
│   ├── cache_telemetry.rs            # order 600
│   ├── overage_warning.rs            # order 610
│   └── (诊断 extensions 默认禁用)
│
├── providers/deepseek_anthropic/     # 已有，不变
└── forwarder.rs, response_processor.rs  # 集成管道调用
```

## 8. 配置管理

### 后端配置

`extensions/config.json` — 控制每个 extension 的加载和顺序：

```json
{
  "fingerprint-strip": { "enabled": true, "order": 100 },
  "sort-stabilization": { "enabled": true, "order": 200 }
}
```

`ProviderMeta` 新增字段：

```rust
pub struct ExtensionFilterConfig {
    pub enabled: bool,                       // 总开关
    pub extensions: HashMap<String, bool>,   // 每个 extension 独立开关
    pub preset: String,                      // "full" | "cache-only" | "minimal"
}
```

### 前端 UI

在 Claude endpoint 配置表单中新增「内容过滤」折叠面板：

- 总开关：启用/禁用整个过滤管道
- 预设快速切换：「完整模式」（23 个全开）、「仅缓存修复」（15 个核心）、「最小模式」（fingerprint + identity）
- 扩展列表：每个 extension 独立开关
- 数据存入 `ProviderMeta.extension_filter_config`，每个 provider 独立

## 9. 翻译策略

不构建代码生成器。当 `claude-code-cache-fix` 发布大版本时：

1. 在 Claude Code 中打开上游 JS extension 源码
2. Claude Code 将 JS 逻辑翻译为 Rust，实现对应 trait
3. 翻译完成后人工审查修正
4. 更新 `config.json` 中的 order/enabled（如上游有变更）

每个 extension 文件头部注释标注来源版本：

```rust
// 来源: claude-code-cache-fix v3.6.1 — proxy/extensions/fingerprint-strip.mjs
// 翻译: 2026-05-19
```

## 10. 错误处理

- 单个 extension 错误使用 `log::warn!` 记录，不中断管道
- `ExtensionError` 包含 extension name 和错误详情
- 管道返回 `Result`，由调用方决定是否降级
- JSON 解析失败不 panic，记录 warning 并跳过该 extension

## 11. 测试策略

- 每个 extension 有独立单元测试，用 fixture JSON 请求体验证输入→输出
- `ExtensionRegistry` 有集成测试，验证排序、过滤、错误隔离
- 端到端测试：启动代理 → 发送请求 → 验证 extension 管道执行
- 与 `claude-code-cache-fix` 的 JS 版本做行为对比测试（相同输入 → 相同输出）
