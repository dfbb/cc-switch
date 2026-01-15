# OpenCode 第四应用支持实现计划

> **范围说明**：本计划暂不包含统一供应商（UniversalProvider）对 OpenCode 的支持，以降低初期实现复杂度。

## 概述

为 CC Switch 添加 OpenCode 支持，这是第四个受管理的 CLI 应用。OpenCode 的核心差异在于采用**累加式**供应商管理（多供应商共存，应用内热切换），而非现有三应用的**替换式**管理。

## 关键设计决策

| 特性 | Claude/Codex/Gemini | OpenCode |
|------|---------------------|----------|
| 供应商模式 | 替换式（单一活跃） | 累加式（多供应商共存） |
| UI 按钮 | 启用/切换 | 添加/删除 |
| is_current | 需要 | 不需要 |
| 代理/故障转移 | 支持 | 不支持 |
| API 格式字段 | 无 | 需要（npm 包名） |
| 配置文件 | 各自独立 | `~/.config/opencode/opencode.json` |

## 配置文件格式

### 供应商配置
```json
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "provider-id": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Provider Name",
      "options": {
        "baseURL": "https://api.example.com/v1",
        "apiKey": "{env:API_KEY}"
      },
      "models": {
        "model-id": { "name": "Model Name" }
      }
    }
  }
}
```

### MCP 配置
```json
{
  "mcp": {
    "remote-server": {
      "type": "remote",
      "url": "https://example.com/mcp",
      "enabled": true
    },
    "local-server": {
      "type": "local",
      "command": ["npx", "-y", "my-mcp-command"],
      "enabled": true,
      "environment": { "KEY": "value" }
    }
  }
}
```

---

## 实现步骤

### Phase 1: 后端数据结构扩展

#### 1.1 AppType 枚举扩展
**文件**: `src-tauri/src/app_config.rs`

```rust
pub enum AppType {
    Claude,
    Codex,
    Gemini,
    OpenCode,  // 新增
}
```

#### 1.2 McpApps / SkillApps 扩展
**文件**: `src-tauri/src/app_config.rs`

```rust
pub struct McpApps {
    pub claude: bool,
    pub codex: bool,
    pub gemini: bool,
    pub opencode: bool,  // 新增
}

pub struct SkillApps {
    pub claude: bool,
    pub codex: bool,
    pub gemini: bool,
    pub opencode: bool,  // 新增
}
```

#### 1.3 数据库 Schema 迁移
**文件**: `src-tauri/src/database/schema.rs`

- `SCHEMA_VERSION` 递增
- 添加迁移：
  ```sql
  ALTER TABLE mcp_servers ADD COLUMN enabled_opencode BOOLEAN NOT NULL DEFAULT 0;
  ALTER TABLE skills ADD COLUMN enabled_opencode BOOLEAN NOT NULL DEFAULT 0;
  ```

### Phase 2: OpenCode 供应商数据结构

#### 2.1 OpenCode 专属配置结构
**文件**: `src-tauri/src/provider.rs`（或新建 `opencode_provider.rs`）

```rust
/// OpenCode 供应商的 settings_config 结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeProviderConfig {
    /// AI SDK 包名，如 "@ai-sdk/openai-compatible"
    pub npm: String,
    /// 供应商选项
    pub options: OpenCodeProviderOptions,
    /// 模型定义
    pub models: HashMap<String, OpenCodeModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeProviderOptions {
    #[serde(rename = "baseURL", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(rename = "apiKey", skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeModel {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<OpenCodeModelLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeModelLimit {
    pub context: Option<u64>,
    pub output: Option<u64>,
}
```

### Phase 3: OpenCode Live 配置读写

#### 3.1 新建 OpenCode 配置模块
**文件**: `src-tauri/src/opencode_config.rs`

核心功能：
- `get_opencode_config_path()` → `~/.config/opencode/opencode.json`
- `read_opencode_config()` → 读取整个配置文件
- `write_opencode_config()` → 原子写入配置文件
- `get_providers()` → 获取 `provider` 对象
- `set_provider(id, config)` → 添加/更新供应商
- `remove_provider(id)` → 删除供应商
- `get_mcp_servers()` → 获取 `mcp` 对象
- `set_mcp_server(id, config)` → 添加/更新 MCP 服务器
- `remove_mcp_server(id)` → 删除 MCP 服务器

### Phase 4: MCP 同步模块

#### 4.1 新建 OpenCode MCP 同步
**文件**: `src-tauri/src/mcp/opencode.rs`

```rust
/// 同步所有 enabled_opencode=true 的服务器到 OpenCode 配置
pub fn sync_enabled_to_opencode(config: &MultiAppConfig) -> Result<(), AppError>

/// 同步单个服务器
pub fn sync_single_server_to_opencode(
    config: &MultiAppConfig,
    id: &str,
    server_spec: &Value
) -> Result<(), AppError>

/// 从 OpenCode 配置移除服务器
pub fn remove_server_from_opencode(id: &str) -> Result<(), AppError>

/// 从 OpenCode 配置导入服务器
pub fn import_from_opencode(config: &mut MultiAppConfig) -> Result<usize, AppError>
```

**格式转换**：
| CC Switch 统一格式 | OpenCode 格式 |
|-------------------|---------------|
| `type: "stdio"` | `type: "local"` |
| `command` + `args` | `command: [cmd, ...args]` |
| `env` | `environment` |
| `type: "sse"/"http"` | `type: "remote"` |
| `url` | `url` |

### Phase 5: 供应商服务层

#### 5.1 OpenCode 供应商服务
**文件**: `src-tauri/src/services/provider/opencode.rs`

核心方法：
```rust
/// 获取所有 OpenCode 供应商
pub fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError>

/// 添加供应商（同时写入 live 配置）
pub fn add(state: &AppState, provider: Provider) -> Result<bool, AppError>

/// 更新供应商
pub fn update(state: &AppState, provider: Provider) -> Result<bool, AppError>

/// 删除供应商（同时从 live 配置移除）
pub fn delete(state: &AppState, id: &str) -> Result<(), AppError>

/// 从 live 配置导入供应商到数据库
pub fn import_from_live(state: &AppState) -> Result<usize, AppError>
```

**关键差异**：
- 不需要 `switch()` 方法
- 不需要 `is_current` 管理
- `add()` 自动写入 live
- `delete()` 自动从 live 移除

### Phase 6: Tauri 命令扩展

#### 6.1 更新现有命令
**文件**: `src-tauri/src/commands/providers.rs`

- 所有命令支持 `app_type = "opencode"`
- OpenCode 特定逻辑分支

#### 6.2 新增 OpenCode 专属命令（如需要）
```rust
#[tauri::command]
pub async fn opencode_sync_all_providers(state: State<'_, AppState>) -> Result<(), AppError>
```

### Phase 7: 前端类型定义

#### 7.1 TypeScript 类型扩展
**文件**: `src/types.ts`

```typescript
// AppId 扩展
type AppId = "claude" | "codex" | "gemini" | "opencode";

// OpenCode 专属配置
interface OpenCodeProviderConfig {
  npm: string;  // AI SDK 包名
  options: {
    baseURL?: string;
    apiKey?: string;
    headers?: Record<string, string>;
  };
  models: Record<string, OpenCodeModel>;
}

interface OpenCodeModel {
  name: string;
  limit?: {
    context?: number;
    output?: number;
  };
}
```

#### 7.2 MCP 应用状态扩展
**文件**: `src/types.ts`

```typescript
interface McpApps {
  claude: boolean;
  codex: boolean;
  gemini: boolean;
  opencode: boolean;  // 新增
}
```

### Phase 8: 前端预设配置

#### 8.1 新建 OpenCode 供应商预设
**文件**: `src/config/opencodeProviderPresets.ts`

```typescript
export const opencodeProviderPresets: ProviderPreset[] = [
  {
    name: "OpenAI",
    npmPackage: "@ai-sdk/openai",
    settingsConfig: {
      npm: "@ai-sdk/openai",
      options: { apiKey: "{env:OPENAI_API_KEY}" },
      models: {
        "gpt-4o": { name: "GPT-4o" },
        "gpt-4o-mini": { name: "GPT-4o Mini" },
      },
    },
    theme: { icon: "openai", iconColor: "#00A67E" },
  },
  {
    name: "Anthropic",
    npmPackage: "@ai-sdk/anthropic",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      options: { apiKey: "{env:ANTHROPIC_API_KEY}" },
      models: {
        "claude-sonnet-4-20250514": { name: "Claude Sonnet 4" },
      },
    },
  },
  {
    name: "OpenAI Compatible",
    npmPackage: "@ai-sdk/openai-compatible",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      options: {
        baseURL: "",
        apiKey: "{env:API_KEY}",
      },
      models: {},
    },
    isCustomTemplate: true,
  },
  // ... 更多预设
];

// npm 包选项
export const opencodeNpmPackages = [
  { value: "@ai-sdk/openai", label: "OpenAI" },
  { value: "@ai-sdk/anthropic", label: "Anthropic" },
  { value: "@ai-sdk/openai-compatible", label: "OpenAI Compatible" },
  { value: "@ai-sdk/google", label: "Google" },
  { value: "@ai-sdk/azure", label: "Azure OpenAI" },
  { value: "@ai-sdk/amazon-bedrock", label: "Amazon Bedrock" },
  // ... 更多选项
];
```

### Phase 9: 前端 UI 组件

#### 9.1 OpenCode 供应商表单
**文件**: `src/components/providers/forms/OpenCodeFormFields.tsx`

新增字段：
- npm 包选择器（下拉框 + 自定义输入）
- options 编辑器（baseURL, apiKey, headers）
- models 编辑器（动态添加/删除模型）

#### 9.2 供应商卡片按钮适配
**文件**: `src/components/providers/ProviderActions.tsx`

```tsx
// OpenCode 使用不同的主按钮
if (appId === "opencode") {
  return (
    <Button onClick={onAdd}>
      {isInConfig ? t("provider.removeFromConfig") : t("provider.addToConfig")}
    </Button>
  );
}
```

#### 9.3 隐藏 OpenCode 不需要的功能

在以下组件中检查 `appId !== "opencode"`：
- 代理设置面板
- 故障转移队列
- 供应商切换逻辑

### Phase 10: 国际化

#### 10.1 新增翻译 Key
**文件**: `src/locales/zh/translation.json` & `en/translation.json`

```json
{
  "app.opencode": "OpenCode",
  "provider.addToConfig": "添加到配置",
  "provider.removeFromConfig": "从配置移除",
  "provider.inConfig": "已添加",
  "provider.npmPackage": "AI SDK 包",
  "provider.models": "模型配置",
  // ...
}
```

---

## 关键文件清单

### 后端（Rust）
| 操作 | 文件路径 |
|------|---------|
| 修改 | `src-tauri/src/app_config.rs` |
| 修改 | `src-tauri/src/database/schema.rs` |
| 修改 | `src-tauri/src/database/dao/mcp.rs` |
| 修改 | `src-tauri/src/database/dao/providers.rs` |
| 修改 | `src-tauri/src/services/provider/mod.rs` |
| 修改 | `src-tauri/src/services/mcp.rs` |
| 修改 | `src-tauri/src/commands/providers.rs` |
| 修改 | `src-tauri/src/commands/mcp.rs` |
| 修改 | `src-tauri/src/mcp/mod.rs` |
| 新建 | `src-tauri/src/opencode_config.rs` |
| 新建 | `src-tauri/src/mcp/opencode.rs` |
| 新建 | `src-tauri/src/services/provider/opencode.rs` |

### 前端（TypeScript/React）
| 操作 | 文件路径 |
|------|---------|
| 修改 | `src/types.ts` |
| 修改 | `src/lib/api/types.ts` |
| 修改 | `src/lib/api/providers.ts` |
| 修改 | `src/components/providers/ProviderActions.tsx` |
| 修改 | `src/components/providers/ProviderCard.tsx` |
| 修改 | `src/components/providers/AddProviderDialog.tsx` |
| 修改 | `src/components/providers/forms/ProviderForm.tsx` |
| 修改 | `src/App.tsx` |
| 新建 | `src/config/opencodeProviderPresets.ts` |
| 新建 | `src/components/providers/forms/OpenCodeFormFields.tsx` |

### 国际化
| 操作 | 文件路径 |
|------|---------|
| 修改 | `src/locales/zh/translation.json` |
| 修改 | `src/locales/en/translation.json` |
| 修改 | `src/locales/ja/translation.json` |

---

## 验证计划

### 单元测试
1. OpenCode 配置读写测试
2. MCP 格式转换测试（stdio ↔ local, sse ↔ remote）
3. 供应商 CRUD 操作测试

### 集成测试
1. 添加 OpenCode 供应商 → 验证写入 `~/.config/opencode/opencode.json`
2. 删除供应商 → 验证从配置文件移除
3. MCP 同步测试 → 验证格式正确转换
4. 从 live 配置导入 → 验证正确解析

### 手动测试
1. UI 流程：添加预设 → 编辑 → 删除
2. 切换应用 Tab → OpenCode 显示正确的 UI（无代理/故障转移）
3. 托盘菜单正确显示 OpenCode 供应商
4. 深链接导入 OpenCode 供应商

---

## 风险评估

1. **数据库迁移**：需要在升级时自动执行 `ALTER TABLE` 语句
2. **配置文件冲突**：OpenCode 可能有自己的配置，需要合并而非覆盖
3. **MCP 格式差异**：`stdio` → `local` 转换需要处理边界情况
4. **UI 一致性**：OpenCode 的"添加/删除"模式需要与其他应用的"启用/切换"清晰区分

---

## 补充说明

### 托盘菜单特殊处理

由于 OpenCode 采用累加式管理，托盘菜单行为需要调整：

- **现有三应用**：托盘菜单显示 `CheckMenuItem`（单选，切换当前供应商）
- **OpenCode**：显示当前所有启用的供应商（普通 MenuItem，无勾选逻辑），点击打开主界面

**修改文件**：`src-tauri/src/tray.rs`（`TRAY_SECTIONS` 常量）

### 数据库约束更新

`proxy_config` 表的 CHECK 约束需要扩展：
```sql
CHECK (app_type IN ('claude','codex','gemini','opencode'))
```

### Settings 结构体扩展

**文件**：`src-tauri/src/settings.rs`

需要添加：
- `current_provider_opencode: Option<String>` - 对 OpenCode 可能无意义，但保持结构一致
- `opencode_config_dir: Option<String>` - 自定义配置目录
