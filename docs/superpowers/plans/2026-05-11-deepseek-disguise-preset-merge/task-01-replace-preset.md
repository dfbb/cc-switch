# T01 — 替换 disguise 预设

**可并行：** 否（链路起点）
**依赖：** 无

**Files:**
- Modify: `src/config/claudeProviderPresets.ts:1030-1073`（两条 disguise 预设替换为一条）

## 目标

把 `DeepSeek (Claude Disguise · Flash)` 和 `DeepSeek (Claude Disguise · Pro)` 两条预设替换为单条 `DeepSeek (Claude Disguise)`，默认主模型 = opus、Haiku/Sonnet 路由 = sonnet、Opus 路由 = opus。

## 步骤

- [ ] **Step 1：定位现有两条预设**

  打开 `src/config/claudeProviderPresets.ts`，确认 1030-1073 行是这两条 disguise 预设：

  ```
  Line 1031: name: "DeepSeek (Claude Disguise · Flash)"
  ...
  Line 1053: name: "DeepSeek (Claude Disguise · Pro)"
  ...
  Line 1073: },   // 数组的最后一个元素的尾部
  Line 1074: ];
  ```

- [ ] **Step 2：用 Edit 工具替换两条预设为一条**

  把 1030-1073 行（从 `{` Flash 开头到 `},` Pro 结尾）整体替换为：

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
  },
  ```

- [ ] **Step 3：检查无残留旧名引用**

  全局搜索：

  ```bash
  rg "Claude Disguise · Flash|Claude Disguise · Pro" src/
  ```

  期望：无任何匹配（如有匹配在测试 / i18n / 文档以外的源码中，需要按搜索结果同步处理；本计划假设没有引用 — 实际 spec 已确认）。

- [ ] **Step 4：commit**

  ```bash
  git add src/config/claudeProviderPresets.ts
  git commit -m "feat: merge DeepSeek disguise Flash/Pro presets into single preset"
  ```

## 验收

- 文件中只剩一条 disguise 预设
- 字段值与 spec 一致（opus 主、sonnet Haiku/Sonnet、opus Opus）
- 全局无旧名残留
