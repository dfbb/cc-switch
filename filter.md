# DeepSeek 伪装 + Claude 内容过滤 使用指南

CC Switch 提供两个互补的特性，让你用更低的成本和更高的缓存命中率使用 Claude Code：

1. **DeepSeek (Claude Disguise)** — 让 Claude Code 直接调用 DeepSeek API
2. **Claude 内容过滤** — 23 个 extension 自动稳定缓存前缀、清理冗余内容、剥离图片，显著降低 Token 消耗

---

## 一、DeepSeek 伪装：用 DeepSeek 跑 Claude Code

### 作用

把 Claude Code 的请求直接转发到 DeepSeek 的 Anthropic 兼容接口。模型名映射如下：

| Claude 模型 | DeepSeek 实际调用 |
|------------|------------------|
| Opus / Sonnet | `deepseek-v4-pro`（max 推理模式） |
| Haiku | `deepseek-v4-flash` |

### 如何配置

1. 打开 CC Switch，左侧切换到 **Claude** 应用
2. 点击 **新建供应商** 按钮（+）
3. 在预设下拉中选择 **DeepSeek (Claude Disguise)**
4. 填入 DeepSeek API Key（[点此获取](https://platform.deepseek.com/api_keys)）
5. 点击 **保存**

### 启用

- 在供应商列表中点击刚创建的 DeepSeek 卡片，将其切换为当前供应商
- CC Switch 会自动启用代理接管，无需额外操作

### 注意事项

- API Key 字段固定为 `ANTHROPIC_API_KEY`（不是 `ANTHROPIC_AUTH_TOKEN`）
- 切换到 DeepSeek 后，本地代理自动开启；切换到其它供应商时自动恢复
- DeepSeek 不支持 Claude 的部分功能（如图像、某些 server tools），代理会自动清理这些字段

---

## 二、Claude 内容过滤：23 个 Extension 自动优化请求

### 作用

在请求发往 Claude API（或 DeepSeek 伪装）之前，自动执行 23 个内容过滤器，主要解决：

- **缓存命中率低**：稳定 `cc_version` 指纹、规范化 cache_control 标记位置
- **Token 浪费**：剥离冗余的系统提醒、记账文本、"Continue" 尾随
- **图片成本**：移除历史消息中的 base64 图片
- **TTL 错误**：自动注入正确的 5m/1h TTL
- **使用监控**：统计 Token 用量、缓存命中率，超量自动告警

### 如何打开配置菜单

1. 打开 CC Switch，切换到 **Claude** 应用
2. 在供应商列表中找到目标供应商（任意 Claude 供应商均支持，包括 DeepSeek 伪装）
3. 点击该供应商的 **编辑** 按钮
4. 滚动到表单最下方，展开 **高级选项** 折叠面板
5. 折叠面板最底部即 **内容过滤扩展** 区域

### 启用与预设

- 顶部 **启用内容过滤** 总开关：默认关闭，开启后才会运行过滤管道
- 三个预设按钮（一键切换）：

| 预设 | 说明 |
|------|------|
| **完整模式** | 23 个 extension 全部启用 — 最大化 Token 节省 |
| **仅缓存修复** | 启用 15 个核心 extension — 平衡稳定性与节省 |
| **最小模式** | 仅 `fingerprint-strip` + `identity-normalization` — 保守起步 |

也可以在下方列表中独立开关每个 extension。

### 推荐配置场景

| 场景 | 推荐预设 |
|------|---------|
| 首次使用，想稳妥试用 | **最小模式** |
| 日常使用 Claude Code | **仅缓存修复** |
| 长会话、多图片、想最大化省钱 | **完整模式** |
| 配合 DeepSeek 伪装使用 | **完整模式** |

### 主要 Extension 速查

**缓存稳定性（推荐全开）**

- `fingerprint-strip`：稳定 cc_version 指纹，防止 cache 同会话失效
- `sort-stabilization`：按字母序排列 skills/tools，保证请求前缀稳定
- `fresh-session-sort`：新会话首轮请求确定性排序
- `cache-control-normalize`：规范化 cache_control 标记位置
- `messages-cache-breakpoint`：注入第 3 个缓存断点，最大化命中率

**身份标准化（推荐全开）**

- `identity-normalization`：清理 SessionStart、Last active 等易变标识
- `smoosh-split`：分离 tool_result 中粘合的系统提醒
- `content-strip`：移除 "Continue from where you left off" 和记账文本
- `tool-input-normalize`：按 schema 排序 tool_use 参数

**图像 / 内容（按需开启）**

- `image-strip`：剥离历史消息中的 base64 图片 — 可省 60%+ Token
- `thinking-display`：Opus 4.7 自动注入 thinking 显示

**TTL 管理（推荐全开）**

- `ttl-tier-detect` + `ttl-management`：自动识别并注入 5m/1h TTL

**监控诊断（按需开启）**

- `cache-telemetry`：持久化缓存命中率到 `~/.claude/quota-status/`
- `overage-warning`：超量阈值时 stderr 警告
- `usage-log`：每请求 token 用量记录到 `~/.claude/usage.jsonl`
- `request-log`：请求计时 NDJSON 日志

### 主界面 Token 显示

启用过滤管道后，主界面顶部 **CC Switch** 标题旁会显示当前 Token 用量：

```
CC Switch (1.2K/45.6K)
         ↑       ↑
       今日   全部
```

点击该数字会跳转到 **设置 → 使用统计** 页面，查看详细的 Token 消耗趋势、缓存命中率、按供应商分布等。

### 高级：环境变量

部分 extension 需要 env var 激活才会运行（即使在 UI 中开启）：

| Extension | 环境变量 |
|-----------|---------|
| `image-strip` | `CACHE_FIX_IMAGE_GUARD=1` |
| `messages-cache-breakpoint` | `CACHE_FIX_INJECT_MESSAGES_BREAKPOINT=1` |
| `microcompact-stability` | `CACHE_FIX_NORMALIZE_MICROCOMPACT=1` |
| `upstream-change-detection` | `CACHE_FIX_UPSTREAM_DETECTION=1` |
| `prefix-diff` | `CACHE_FIX_PREFIXDIFF=1` |
| `output-efficiency-rewrite` | `CACHE_FIX_OUTPUT_EFFICIENCY_REPLACEMENT="<text>"` |
| `thinking-display` | `CACHE_FIX_THINKING_DISPLAY="summarized"`（默认） |
| `ttl-management` | `CACHE_FIX_TTL_MAIN`/`CACHE_FIX_TTL_SUBAGENT`（默认 `1h`） |

设置方式：在系统环境或 CC Switch 启动脚本中导出对应变量。

---

## 三、组合使用：DeepSeek 伪装 + 内容过滤

最推荐的组合：用 **DeepSeek 伪装** 接管 Claude Code，同时启用 **内容过滤完整模式**。这样可以：

- 用 DeepSeek 极低的价格调用 Claude Code
- 通过内容过滤进一步压缩请求体，省 30-60% Token
- 保留完整的缓存命中率优化
- 自动剥离 DeepSeek 不支持的字段（图片、server tools 等）

### 配置步骤

1. 按 **一、DeepSeek 伪装** 创建并切换到 DeepSeek 供应商
2. 编辑该供应商，按 **二、内容过滤** 启用过滤面板
3. 选择 **完整模式** 预设
4. 保存即可

完成后主界面会显示绿色 "CC Switch (XXX/XXX)" — 代表代理接管激活，token badge 实时刷新。

---

## 四、故障排查

| 现象 | 排查方向 |
|------|---------|
| 切换到 DeepSeek 后请求失败 | 检查 API Key 是否填错；检查端点是否为 `https://api.deepseek.com/anthropic` |
| 内容过滤面板找不到 | 必须在 **高级选项** 折叠面板中，向下滚动到底部 |
| 开关启用了但似乎没生效 | 部分 extension 需要 env var 激活，见上表 |
| Token badge 不显示 | 全部 token 总数为 0 时不显示；先发几次请求 |
| 缓存命中率仍很低 | 启用 `messages-cache-breakpoint`（需 env var）和完整模式 |
