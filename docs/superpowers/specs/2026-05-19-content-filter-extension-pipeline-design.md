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
- 默认迁移策略：`ExtensionFilterConfig.enabled` 缺失时视为 `false`，已有 provider 不受影响

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
1. 解析请求 → RequestContext（body + headers 的副本）
2. forwarder.forward_with_retry() — 故障转移循环
   for each provider:
     a. [NEW] registry.run_request_pipeline(&mut ctx, &provider)
        → 按当前 provider 的 extension_filter_config 过滤执行
        → 返回 Some((status, body)) 则短路返回
     b. [仅 disguise] request_sanitizer::sanitize_request()
     c. forward() — 发送到上游
     d. 成功: 跳出循环; 失败: 尝试下一个 provider
3. [NEW] registry.run_response_start_pipeline(&mut ctx)
4. [NEW] [stream] registry.run_stream_event_pipeline(&mut ctx) × N
   [NEW] [non-stream] registry.run_response_pipeline(&mut ctx)
5. 返回给客户端
```

请求管道放在 per-attempt provider 选择之后，确保每个 provider 的独立配置生效。

每次 attempt 基于原始请求的克隆执行：
- 进入循环前，保存一份 `original_body` 和 `original_headers` 的副本
- 每次 attempt 开始时，从原始副本重建 `RequestContext`，确保上一个 provider 的
  body/header 变更不会泄漏到下一个 provider（例如 A 启用 image-strip 失败后，
  B 未启用扩展也不应收到已剥离图片的请求）
- 成功的 provider：其修改后的 ctx 保留并带入响应阶段
- 任意 attempt 中 `run_request_pipeline()` 返回 `Some((status, body))`：
  extension 主动拦截请求，合成响应直接返回客户端，不经过上游
- 所有 provider 的 HTTP 转发都失败（包括重试耗尽和不可重试错误）：
  按已有故障转移逻辑返回最后一个错误，不走拦截响应路径

### 响应管道的统一接入

响应 extension 必须看到 Anthropic 格式的 body/SSE 事件（cache-telemetry 等依赖
Anthropic 的 `message_start`、`message_delta` 事件结构和 model 字段），因此管道 hook
放在格式转换**之后**，而非原始上游响应之上：

- **透传路径**（anthropic / deepseek_anthropic）：原始响应即是 Anthropic 格式，
  直接嵌入 `run_response_start_pipeline` → `run_response_pipeline` / `run_stream_event_pipeline`
- **转换路径**（openai_chat / openai_responses / gemini_native）：
  `handle_claude_transform` 先将上游响应转换为 Anthropic 格式的 body 或 SSE 流，
  然后在转换结果上运行响应管道。具体做法：
  - 非流式：转换后的 JSON body → 构造 `ResponseContext` → `run_response_pipeline`
  - 流式：转换后 SSE 流的每个事件 → 构造 `StreamEventContext` → `run_stream_event_pipeline`
- `run_response_start_pipeline` 在所有路径的统一位置调用，传入最终返回给客户端的
  HTTP status 和 headers

## 3. Trait 设计

### 基础 trait

```rust
/// 所有 extension 必须实现
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn order(&self) -> u32;
    fn default_enabled(&self) -> bool;  // 出厂默认，实际启用由 provider 配置覆盖
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
| `ResponseStartContext` | 响应 headers | status, headers（只读）, meta |
| `ResponseContext` | 响应体 | body, headers, meta |
| `StreamEventContext` | SSE 事件 | data, drop, meta, telemetry |

Extension 只实现需要的 trait。例如 `fingerprint-strip` 只需 `RequestExtension`。

## 4. Extension 注册和执行

### ExtensionRegistry

```rust
pub struct ExtensionRegistry {
    // Rust 不支持从 dyn Extension 向下转型到子 trait，
    // 因此按 trait 分别存储各自的 trait object，直接可用。
    request_exts: Vec<Box<dyn RequestExtension>>,
    response_exts: Vec<Box<dyn ResponseExtension>>,
    stream_exts: Vec<Box<dyn StreamExtension>>,
}

impl ExtensionRegistry {
    /// 加载全部 extension，只按 order 排序，不做 enabled 过滤。
    /// 过滤推迟到管道执行时，根据当前 provider 的配置决定。
    pub fn load_all() -> Self { ... }
}
```

- 代理启动时加载 `extensions/config.json`，读取 order 和 default_enabled
- **全部 extension 都载入**并按 order 排序，不在此阶段过滤
- 过滤决策推迟到 `run_*_pipeline()` 执行时：结合当前 provider 的
  `extension_filter_config` 与 extension 的 `default_enabled` 决定是否调用
- 这样用户在任何 provider 中显式启用某个默认禁用的诊断 extension 时，
  registry 中已有该 extension 可直接执行
- v1 不做文件监控热重载（后续版本考虑）

### 跨阶段状态共享

三个 Vec 分别存储不同 trait object，同一 extension 若实现多个 trait 会注册到多个 Vec
中。extension 实例本身不应持有跨阶段可变状态（会被多次不可变借用），所有需要在
Request → ResponseStart → Stream/Response 之间传递的数据统一通过 `ctx.meta` 传递：

```rust
// ExtensionMeta 贯穿整个请求生命周期
pub struct ExtensionMeta {
    data: HashMap<String, serde_json::Value>,
}
```

- `ttl-tier-detect`（Request）写入 `ctx.meta["_ttlTier"]`，`ttl-management`（Request）读取
- `cache-telemetry`（Request）写入 `ctx.meta["_sessionId"]`，
  ResponseStart 写入 `ctx.meta["_quotaData"]`，Stream 读取两者
- extension 实例保持无状态或仅持有不可变配置

### 管道执行

```rust
impl ExtensionRegistry {
    // 每个管道方法接收 &Provider 以读取 extension_filter_config，
    // 结合 extension 的 default_enabled 决定是否调用
    pub fn run_request_pipeline(
        &self, ctx: &mut RequestContext, config: &ExtensionFilterConfig,
    ) -> Result<Option<(u16, Vec<u8>)>, ExtensionError>;

    pub fn run_response_start_pipeline(
        &self, ctx: &mut ResponseStartContext, config: &ExtensionFilterConfig,
    ) -> Result<(), ExtensionError>;

    pub fn run_response_pipeline(
        &self, ctx: &mut ResponseContext, config: &ExtensionFilterConfig,
    ) -> Result<(), ExtensionError>;

    pub fn run_stream_event_pipeline(
        &self, ctx: &mut StreamEventContext, config: &ExtensionFilterConfig,
    ) -> Result<(), ExtensionError>;
}
```

**执行时的启用判定逻辑**：

```
extension 实际执行 = config.enabled.unwrap_or(false)          // 总开关必须为 true
                    && config.extensions[name].unwrap_or(     // 优先取 provider 显式设置
                        extension.default_enabled              // 回退到出厂默认
                    )
```

- `config.enabled` 缺失或 `false` → 跳过所有 extension
- `config.enabled == Some(true)` → 逐个检查 extension 级别的开关
  - `config.extensions[name]` 有值 → 使用显式设置
  - `config.extensions[name]` 无值 → 使用 `extension.default_enabled`（来自 config.json）

## 5. Extension 清单（23 个）

> 表中的"默认启用/禁用"仅表示出厂预设。实际运行时，需 `ExtensionFilterConfig.enabled == Some(true)` 才会执行。
> 已有 provider 升级后该字段缺失 → 管道不运行 → 行为不变。

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
| ttl-tier-detect | 75 | Request | 从请求体 cache_control 检测 TTL 层级，写入 ctx.meta._ttlTier |
| ttl-management | 500 | Request | 根据 ctx.meta._ttlTier 注入正确的 cache_control（跨 extension 通讯） |

### 会话持久化（1 个，默认启用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| deferred-tools-restore | 350 | Request | 跨会话持久化和恢复 deferred-tools 块 |

### 监控/诊断（2 个，默认启用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| cache-telemetry | 600 | Request + ResponseStart + Stream | Request 存 sessionId → ResponseStart 存 quotaData → Stream 提取缓存命中率并持久化 |
| overage-warning | 610 | ResponseStart | 从响应 headers 检测超量阈值，写入 stderr + JSONL |

### 检测/诊断（7 个，默认禁用）

| Extension | Order | Trait | 功能 |
|-----------|-------|-------|------|
| upstream-change-detection | 50 | Request | 上游请求结构指纹变更检测 |
| microcompact-stability | 350 | Request | 微压缩 sentinel 检测和标准化 |
| rate-limit-log | 660 | ResponseStart | 429 限流事件日志 |
| request-log | 700 | ResponseStart | 请求计时 NDJSON 日志 |
| usage-log | 710 | Response | 从响应体提取 token 用量，写入 `~/.claude/usage.jsonl`（MeterRowSchema v:1） |
| output-efficiency-rewrite | 370 | Request | 重写系统 prompt 的效率章节 |
| prefix-diff | 680 | Request | 请求前缀差异诊断（需在所有修改后 snapshot） |

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

`extensions/config.json` — 定义每个 extension 的默认 order 和出厂状态：

```json
{
  "fingerprint-strip": { "default_enabled": true, "order": 100 },
  "sort-stabilization": { "default_enabled": true, "order": 200 }
}
```

`ProviderMeta` 新增字段：

```rust
pub struct ExtensionFilterConfig {
    /// 总开关。缺失/None 时视为 false——已有 provider 不受影响
    pub enabled: Option<bool>,
    /// 每个 extension 独立开关，覆盖 config.json 的 default_enabled
    pub extensions: HashMap<String, bool>,
    /// 预设标识: "full" | "cache-only" | "minimal" | "" (自定义)
    pub preset: Option<String>,
}
```

迁移策略：
- `enabled` 字段缺失或 `None` → 管道不运行，行为与升级前完全一致
- `preset` 为空字符串或 `None` → 使用自定义开关
- 用户从 UI 选择预设后，`enabled` 和 `extensions` 由前端写入显式值

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

- 单个 extension 执行错误（如 JSON 解析失败、字段缺失）由 `ExtensionRegistry` 内部
  捕获，使用 `log::warn!` 记录（含 extension name 和错误详情），继续执行后续 extension
- `run_*_pipeline()` 方法签名上的 `Result` 仅用于**框架级致命错误**（如 registry 锁中毒），
  正常 extension 错误不会传播出管道
- 接入点无需 `?` 处理 extension 错误——extension 错误不存在时管道保证返回 `Ok`
- 原则：一个 extension 出错不影响同一 pipeline 中其他 extension，也不影响请求的转发/响应

## 11. 测试策略

- 每个 extension 有独立单元测试，用 fixture JSON 请求体验证输入→输出
- `ExtensionRegistry` 有集成测试，验证排序、过滤、错误隔离
- 端到端测试：启动代理 → 发送请求 → 验证 extension 管道执行
- 与 `claude-code-cache-fix` 的 JS 版本做行为对比测试（相同输入 → 相同输出）
