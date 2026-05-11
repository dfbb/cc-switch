# DeepSeek (Claude Disguise) 预设合并设计

## 目标

将 `src/config/claudeProviderPresets.ts` 中两个独立预设

- `DeepSeek (Claude Disguise · Flash)`
- `DeepSeek (Claude Disguise · Pro)`

合并为单个预设 `DeepSeek (Claude Disguise)`，由用户在表单里通过填写 `ANTHROPIC_MODEL` 等字段切换主模型路由。

## 背景

### 现状

两个预设的差异只在两个字段的默认值：

| 字段 | Flash 预设 | Pro 预设 |
|---|---|---|
| ANTHROPIC_MODEL | claude-sonnet-4-6 | claude-opus-4-7 |
| ANTHROPIC_DEFAULT_HAIKU_MODEL | claude-sonnet-4-6 | claude-sonnet-4-6 |
| ANTHROPIC_DEFAULT_SONNET_MODEL | claude-sonnet-4-6 | claude-sonnet-4-6 |
| ANTHROPIC_DEFAULT_OPUS_MODEL | claude-sonnet-4-6 | claude-opus-4-7 |

其他所有字段（`apiFormat`、`category`、`endpointCandidates`、`icon` 等）完全相同。

### 路由语义

`ANTHROPIC_MODEL` 等字段填的"Claude 模型名"由 cc-switch 后端 `deepseek_anthropic` 适配器映射成 DeepSeek 模型名：

- `claude-opus-*` → `deepseek-v4-pro`
- `claude-sonnet-*` / `claude-haiku-*` / 其他 → `deepseek-v4-flash`

用户填什么 Claude 模型名 = 选了哪个 DeepSeek 模型。所以"Flash 预设"与"Pro 预设"本质上不是后端两条路径，只是同一套适配器的两组默认值。

### 表单已支持调整

`src/components/providers/forms/ClaudeFormFields.tsx:663-722` 已经为这四个字段提供独立输入框，用户保存预设后可以随意改。合并预设不会丢失任何用户调整能力。

## 设计决策

### 合并后的预设定义

```ts
{
  name: "DeepSeek (Claude Disguise)",
  websiteUrl: "https://platform.deepseek.com",
  apiKeyUrl: "https://platform.deepseek.com/api_keys",
  apiKeyField: "ANTHROPIC_API_KEY",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
      ANTHROPIC_API_KEY: "",
      ANTHROPIC_MODEL: "claude-opus-4-7",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-opus-4-7",
    },
  },
  apiFormat: "deepseek_anthropic",
  category: "cn_official",
  modelsUrl: "https://api.deepseek.com/models",
  endpointCandidates: ["https://api.deepseek.com/anthropic"],
  icon: "deepseek",
  iconColor: "#1E88E5",
}
```

### 默认路由说明

| 调用端请求 | 填入字段 | 适配器映射结果 |
|---|---|---|
| 主模型 (未指定时)            | ANTHROPIC_MODEL = claude-opus-4-7   | deepseek-v4-pro   |
| 显式 haiku-*                  | ANTHROPIC_DEFAULT_HAIKU_MODEL = claude-sonnet-4-6 | deepseek-v4-flash |
| 显式 sonnet-*                 | ANTHROPIC_DEFAULT_SONNET_MODEL = claude-sonnet-4-6 | deepseek-v4-flash |
| 显式 opus-*                   | ANTHROPIC_DEFAULT_OPUS_MODEL = claude-opus-4-7    | deepseek-v4-pro   |

默认策略：**主模型 + Opus 路由走 Pro，Haiku/Sonnet 路由走 Flash**。如需切换：

- 全 Flash：把 `ANTHROPIC_MODEL` 与 `ANTHROPIC_DEFAULT_OPUS_MODEL` 改成 `claude-sonnet-4-6`
- 全 Pro：把 `ANTHROPIC_DEFAULT_HAIKU_MODEL` / `ANTHROPIC_DEFAULT_SONNET_MODEL` 改成 `claude-opus-4-7`

### 迁移策略

**直接合并，不做数据迁移。** 理由：

1. 预设只是"添加 Provider 时的初始模板"，已经创建的 Provider 拷贝了模板值后与原预设解耦，不会受预设删除影响。
2. 旧 Provider 名字里带 `· Flash` 或 `· Pro` 只是显示文案，不影响运行（路由完全由 `env.ANTHROPIC_MODEL` 决定）。
3. 数据库 / `meta.api_format` / `meta.preset_name` 都不依赖预设名做行为分支，没有需要清理的状态。

旧 Provider 用户后续若想统一显示名，可以自行在 cc-switch 里改名，不需要代码层迁移。

## 变更范围

### 必须修改

- `src/config/claudeProviderPresets.ts` —— 删除两条 disguise 预设，新增合并后的单条
- `/tmp/deepseekproxy.md` —— 第二步的预设表从两行改一行，并补充"主模型默认 Pro、辅助默认 Flash、可在表单切换"的说明（用户文档，不进 git）

### 不需要修改

- 后端 `src-tauri/src/proxy/providers/deepseek_anthropic/` 任何文件 —— 路由完全由 `env.ANTHROPIC_MODEL` 驱动
- `ClaudeFormFields.tsx` —— 四个模型字段已可独立编辑
- i18n —— 预设名是 `name` 字段直接显示，不走 i18n key
- 数据库 schema / 迁移脚本 —— 无 schema 影响

## 测试方案

仅手工验收（无新增自动化测试，预设是纯数据）：

1. 启动 cc-switch
2. **添加 Provider** → Claude 类型 → 选择 `DeepSeek (Claude Disguise)`
3. 确认表单字段：
   - ANTHROPIC_BASE_URL = `https://api.deepseek.com/anthropic`
   - ANTHROPIC_MODEL = `claude-opus-4-7`
   - ANTHROPIC_DEFAULT_HAIKU_MODEL = `claude-sonnet-4-6`
   - ANTHROPIC_DEFAULT_SONNET_MODEL = `claude-sonnet-4-6`
   - ANTHROPIC_DEFAULT_OPUS_MODEL = `claude-opus-4-7`
4. 填 API Key，保存，激活
5. 用 curl 验证（端口为 cc-switch 实际占用端口）：
   - `curl http://127.0.0.1:<port>/v1/messages -d '{"model":"claude-opus-4-7",...}'` → 返回 `model: claude-opus-4-7`，实际上游用 deepseek-v4-pro
   - `curl http://127.0.0.1:<port>/v1/messages -d '{"model":"claude-sonnet-4-6",...}'` → 返回 `model: claude-sonnet-4-6`，实际上游用 deepseek-v4-flash
6. 确认旧 Provider（如果之前用 Flash/Pro 预设建过）仍正常工作

## 非目标

- 不引入 templateValues 让"主模型"成为下拉选择 —— 现有表单已经足够
- 不删除根部那条原生 `DeepSeek` 预设（OpenAI 兼容、走 `deepseek-v4-*` 模型名直填）—— 那是另一套接入方式，与本次合并无关
- 不做 schema / Provider 迁移
