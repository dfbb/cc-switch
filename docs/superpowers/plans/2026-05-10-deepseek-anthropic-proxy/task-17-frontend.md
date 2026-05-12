# T17: 前端 — types.ts + claudeProviderPresets.ts + ClaudeFormFields + i18n

> **并行：** ✅ 完全独立，可与所有其它 task 并行。

**Goal:** 在 TypeScript 类型、provider presets、表单 UI 及 i18n 文件中加入 `deepseek_anthropic` 支持。

**Files:**
- Modify: `src/types.ts`
- Modify: `src/config/claudeProviderPresets.ts`
- Modify: `src/components/providers/forms/ClaudeFormFields.tsx`
- Modify: `src/i18n/locales/zh.json`
- Modify: `src/i18n/locales/en.json`
- Modify: `src/i18n/locales/ja.json`

---

- [ ] **Step 1: 写失败测试（TypeScript 类型检查即为测试）**

先在 `src/types.ts` 中不修改、在 `src/config/claudeProviderPresets.ts` 末尾临时添加使用 `deepseek_anthropic` 的 preset，验证 TypeScript 会报类型错误：

```bash
cd /Users/dfbb/Sites/myidea/ccswitch/cc-switch
# 临时在 claudeProviderPresets.ts 末尾 presets 数组里加一条含 apiFormat: "deepseek_anthropic"
# 然后运行类型检查
pnpm typecheck 2>&1 | grep deepseek
```

Expected: `Type '"deepseek_anthropic"' is not assignable to type ...`（两处：ProviderPreset.apiFormat + ClaudeApiFormat）

- [ ] **Step 2: 修改 `src/types.ts`**

找到（约第 169-173 行）：

```ts
  apiFormat?:
    | "anthropic"
    | "openai_chat"
    | "openai_responses"
    | "gemini_native";
```

改为：

```ts
  apiFormat?:
    | "anthropic"
    | "openai_chat"
    | "openai_responses"
    | "gemini_native"
    | "deepseek_anthropic";
```

找到（约第 201-207 行）`export type ClaudeApiFormat =`，同样追加 `| "deepseek_anthropic"` 到联合类型末尾。

- [ ] **Step 3: 修改 `src/config/claudeProviderPresets.ts`**

**3a. 扩展 `ProviderPreset` 接口的 `apiFormat` 联合**（约第 53-57 行）：

```ts
  apiFormat?:
    | "anthropic"
    | "openai_chat"
    | "openai_responses"
    | "gemini_native"
    | "deepseek_anthropic";
```

**3b. 在 presets 数组末尾（现有 DeepSeek 预设之后）新增两条 preset：**

```ts
{
  name: "DeepSeek (Claude Disguise · Flash)",
  websiteUrl: "https://platform.deepseek.com",
  apiKeyUrl: "https://platform.deepseek.com/api_keys",
  apiKeyField: "ANTHROPIC_API_KEY",
  settingsConfig: {
    env: {
      ANTHROPIC_BASE_URL: "https://api.deepseek.com/anthropic",
      ANTHROPIC_API_KEY: "",
      ANTHROPIC_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_HAIKU_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_SONNET_MODEL: "claude-sonnet-4-6",
      ANTHROPIC_DEFAULT_OPUS_MODEL: "claude-sonnet-4-6",
    },
  },
  apiFormat: "deepseek_anthropic",
  category: "cn_official",
  modelsUrl: "https://api.deepseek.com/models",
  endpointCandidates: ["https://api.deepseek.com/anthropic"],
  icon: "deepseek",
  iconColor: "#1E88E5",
},
{
  name: "DeepSeek (Claude Disguise · Pro)",
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

- [ ] **Step 4: 修改 `src/components/providers/forms/ClaudeFormFields.tsx`**

找到（约第 545-549 行，`gemini_native` SelectItem 之后）：

```tsx
                    <SelectItem value="gemini_native">
                      {t("providerForm.apiFormatGeminiNative", {
                        defaultValue: "Gemini Native generateContent (需转换)",
                      })}
                    </SelectItem>
                  </SelectContent>
```

在 `gemini_native` SelectItem 关闭标签 `</SelectItem>` 后、`</SelectContent>` 前插入：

```tsx
                    <SelectItem value="deepseek_anthropic">
                      {t("providerForm.apiFormatDeepseekAnthropic", {
                        defaultValue: "DeepSeek (Anthropic Compatibility)",
                      })}
                    </SelectItem>
```

- [ ] **Step 5: 修改 i18n 文件**

**`src/i18n/locales/zh.json`** — 在 `providerForm` 对象中（搜索 `"apiFormatGeminiNative"`），在其后新增：

```json
"apiFormatDeepseekAnthropic": "DeepSeek（Anthropic 兼容）"
```

**`src/i18n/locales/en.json`** — 同上位置新增：

```json
"apiFormatDeepseekAnthropic": "DeepSeek (Anthropic Compatibility)"
```

**`src/i18n/locales/ja.json`** — 同上位置新增：

```json
"apiFormatDeepseekAnthropic": "DeepSeek（Anthropic 互換）"
```

- [ ] **Step 6: 运行验证通过**

```bash
pnpm typecheck 2>&1 | tail -5
pnpm test:unit 2>&1 | tail -10
```

Expected: 类型检查无错误，单元测试全通过。

- [ ] **Step 7: 提交**

```bash
git add src/types.ts src/config/claudeProviderPresets.ts \
        src/components/providers/forms/ClaudeFormFields.tsx \
        src/i18n/locales/zh.json src/i18n/locales/en.json src/i18n/locales/ja.json
git commit -m "feat(deepseek): add deepseek_anthropic to frontend types, presets, form, and i18n"
```
