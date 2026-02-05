/**
 * OpenClaw provider presets configuration
 * OpenClaw uses models.providers structure with custom provider configs
 */
import type {
  ProviderCategory,
  OpenClawProviderConfig,
  OpenClawDefaultModel,
} from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

/** Suggested default model configuration for a preset */
export interface OpenClawSuggestedDefaults {
  /** Default model config to apply (agents.defaults.model) */
  model?: OpenClawDefaultModel;
  /** Model catalog entries to add (agents.defaults.models) */
  modelCatalog?: Record<string, { alias?: string }>;
}

export interface OpenClawProviderPreset {
  name: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  /** OpenClaw settings_config structure */
  settingsConfig: OpenClawProviderConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  /** Template variable definitions */
  templateValues?: Record<string, TemplateValueConfig>;
  /** Visual theme config */
  theme?: PresetTheme;
  /** Icon name */
  icon?: string;
  /** Icon color */
  iconColor?: string;
  /** Mark as custom template (for UI distinction) */
  isCustomTemplate?: boolean;
  /** Suggested default model configuration */
  suggestedDefaults?: OpenClawSuggestedDefaults;
}

/**
 * OpenClaw API protocol options
 * @see https://github.com/openclaw/openclaw/blob/main/docs/gateway/configuration.md
 */
export const openclawApiProtocols = [
  { value: "openai-completions", label: "OpenAI Completions" },
  { value: "openai-responses", label: "OpenAI Responses" },
  { value: "anthropic-messages", label: "Anthropic Messages" },
  { value: "google-generative-ai", label: "Google Generative AI" },
  { value: "bedrock-converse-stream", label: "AWS Bedrock" },
] as const;

/**
 * OpenClaw provider presets list
 */
export const openclawProviderPresets: OpenClawProviderPreset[] = [
  // ========== Chinese Officials ==========
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      baseUrl: "https://api.deepseek.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "deepseek-chat",
          name: "DeepSeek V3.2",
          contextWindow: 64000,
          cost: { input: 0.0005, output: 0.002 },
        },
        {
          id: "deepseek-reasoner",
          name: "DeepSeek R1",
          contextWindow: 64000,
          cost: { input: 0.0005, output: 0.002 },
        },
      ],
    },
    category: "cn_official",
    icon: "deepseek",
    iconColor: "#1E88E5",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "deepseek/deepseek-chat",
        fallbacks: ["deepseek/deepseek-reasoner"],
      },
      modelCatalog: {
        "deepseek/deepseek-chat": { alias: "DeepSeek" },
        "deepseek/deepseek-reasoner": { alias: "R1" },
      },
    },
  },
  {
    name: "Zhipu GLM",
    websiteUrl: "https://open.bigmodel.cn",
    apiKeyUrl: "https://www.bigmodel.cn/claude-code?ic=RRVJPB5SII",
    settingsConfig: {
      baseUrl: "https://open.bigmodel.cn/api/paas/v4",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "glm-4.7",
          name: "GLM-4.7",
          contextWindow: 128000,
          cost: { input: 0.001, output: 0.001 },
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "zhipu",
    icon: "zhipu",
    iconColor: "#0F62FE",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://open.bigmodel.cn/api/paas/v4",
        defaultValue: "https://open.bigmodel.cn/api/paas/v4",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "zhipu/glm-4.7" },
      modelCatalog: { "zhipu/glm-4.7": { alias: "GLM" } },
    },
  },
  {
    name: "Qwen Coder",
    websiteUrl: "https://bailian.console.aliyun.com",
    apiKeyUrl: "https://bailian.console.aliyun.com/#/api-key",
    settingsConfig: {
      baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "qwen3-max",
          name: "Qwen3 Max",
          contextWindow: 32000,
          cost: { input: 0.002, output: 0.006 },
        },
      ],
    },
    category: "cn_official",
    icon: "qwen",
    iconColor: "#FF6A00",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        defaultValue: "https://dashscope.aliyuncs.com/compatible-mode/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "qwen/qwen3-max" },
      modelCatalog: { "qwen/qwen3-max": { alias: "Qwen" } },
    },
  },
  {
    name: "Kimi k2.5",
    websiteUrl: "https://platform.moonshot.cn/console",
    apiKeyUrl: "https://platform.moonshot.cn/console/api-keys",
    settingsConfig: {
      baseUrl: "https://api.moonshot.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "kimi-k2.5",
          name: "Kimi K2.5",
          contextWindow: 131072,
          cost: { input: 0.002, output: 0.006 },
        },
      ],
    },
    category: "cn_official",
    icon: "kimi",
    iconColor: "#6366F1",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.moonshot.cn/v1",
        defaultValue: "https://api.moonshot.cn/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "kimi/kimi-k2.5" },
      modelCatalog: { "kimi/kimi-k2.5": { alias: "Kimi" } },
    },
  },
  {
    name: "MiniMax",
    websiteUrl: "https://platform.minimaxi.com",
    apiKeyUrl: "https://platform.minimaxi.com/subscribe/coding-plan",
    settingsConfig: {
      baseUrl: "https://api.minimaxi.com/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "MiniMax-M2.1",
          name: "MiniMax M2.1",
          contextWindow: 200000,
          cost: { input: 0.001, output: 0.004 },
        },
      ],
    },
    category: "cn_official",
    isPartner: true,
    partnerPromotionKey: "minimax_cn",
    theme: {
      backgroundColor: "#f64551",
      textColor: "#FFFFFF",
    },
    icon: "minimax",
    iconColor: "#FF6B6B",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "minimax/MiniMax-M2.1" },
      modelCatalog: { "minimax/MiniMax-M2.1": { alias: "MiniMax" } },
    },
  },

  // ========== Aggregators ==========
  {
    name: "AiHubMix",
    websiteUrl: "https://aihubmix.com",
    apiKeyUrl: "https://aihubmix.com",
    settingsConfig: {
      baseUrl: "https://aihubmix.com/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-5-20250929",
          name: "Claude Sonnet 4.5",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-5-20251101",
          name: "Claude Opus 4.5",
          contextWindow: 200000,
          cost: { input: 15, output: 75 },
        },
      ],
    },
    category: "aggregator",
    icon: "aihubmix",
    iconColor: "#006FFB",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "aihubmix/claude-sonnet-4-5-20250929",
        fallbacks: ["aihubmix/claude-opus-4-5-20251101"],
      },
      modelCatalog: {
        "aihubmix/claude-sonnet-4-5-20250929": { alias: "Sonnet" },
        "aihubmix/claude-opus-4-5-20251101": { alias: "Opus" },
      },
    },
  },
  {
    name: "DMXAPI",
    websiteUrl: "https://www.dmxapi.cn",
    apiKeyUrl: "https://www.dmxapi.cn",
    settingsConfig: {
      baseUrl: "https://www.dmxapi.cn/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-5-20250929",
          name: "Claude Sonnet 4.5",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-5-20251101",
          name: "Claude Opus 4.5",
          contextWindow: 200000,
          cost: { input: 15, output: 75 },
        },
      ],
    },
    category: "aggregator",
    isPartner: true,
    partnerPromotionKey: "dmxapi",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "dmxapi/claude-sonnet-4-5-20250929",
        fallbacks: ["dmxapi/claude-opus-4-5-20251101"],
      },
      modelCatalog: {
        "dmxapi/claude-sonnet-4-5-20250929": { alias: "Sonnet" },
        "dmxapi/claude-opus-4-5-20251101": { alias: "Opus" },
      },
    },
  },
  {
    name: "OpenRouter",
    websiteUrl: "https://openrouter.ai",
    apiKeyUrl: "https://openrouter.ai/keys",
    settingsConfig: {
      baseUrl: "https://openrouter.ai/api/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "anthropic/claude-sonnet-4.5",
          name: "Claude Sonnet 4.5",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "anthropic/claude-opus-4.5",
          name: "Claude Opus 4.5",
          contextWindow: 200000,
          cost: { input: 15, output: 75 },
        },
      ],
    },
    category: "aggregator",
    icon: "openrouter",
    iconColor: "#6566F1",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-or-...",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "openrouter/anthropic/claude-sonnet-4.5",
        fallbacks: ["openrouter/anthropic/claude-opus-4.5"],
      },
      modelCatalog: {
        "openrouter/anthropic/claude-sonnet-4.5": { alias: "Sonnet" },
        "openrouter/anthropic/claude-opus-4.5": { alias: "Opus" },
      },
    },
  },
  {
    name: "ModelScope",
    websiteUrl: "https://modelscope.cn",
    apiKeyUrl: "https://modelscope.cn/my/myaccesstoken",
    settingsConfig: {
      baseUrl: "https://api-inference.modelscope.cn/v1",
      apiKey: "",
      api: "openai-completions",
      models: [
        {
          id: "ZhipuAI/GLM-4.7",
          name: "GLM-4.7",
          contextWindow: 128000,
          cost: { input: 0.001, output: 0.001 },
        },
      ],
    },
    category: "aggregator",
    icon: "modelscope",
    iconColor: "#624AFF",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api-inference.modelscope.cn/v1",
        defaultValue: "https://api-inference.modelscope.cn/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: { primary: "modelscope/ZhipuAI/GLM-4.7" },
      modelCatalog: { "modelscope/ZhipuAI/GLM-4.7": { alias: "GLM" } },
    },
  },

  // ========== Third Party Partners ==========
  {
    name: "PackyCode",
    websiteUrl: "https://www.packyapi.com",
    apiKeyUrl: "https://www.packyapi.com/register?aff=cc-switch",
    settingsConfig: {
      baseUrl: "https://www.packyapi.com/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-5-20250929",
          name: "Claude Sonnet 4.5",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-5-20251101",
          name: "Claude Opus 4.5",
          contextWindow: 200000,
          cost: { input: 15, output: 75 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "packycode",
    icon: "packycode",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "packycode/claude-sonnet-4-5-20250929",
        fallbacks: ["packycode/claude-opus-4-5-20251101"],
      },
      modelCatalog: {
        "packycode/claude-sonnet-4-5-20250929": { alias: "Sonnet" },
        "packycode/claude-opus-4-5-20251101": { alias: "Opus" },
      },
    },
  },
  {
    name: "Cubence",
    websiteUrl: "https://cubence.com",
    apiKeyUrl: "https://cubence.com/signup?code=CCSWITCH&source=ccs",
    settingsConfig: {
      baseUrl: "https://api.cubence.com/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-5-20250929",
          name: "Claude Sonnet 4.5",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-5-20251101",
          name: "Claude Opus 4.5",
          contextWindow: 200000,
          cost: { input: 15, output: 75 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "cubence",
    icon: "cubence",
    iconColor: "#000000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "cubence/claude-sonnet-4-5-20250929",
        fallbacks: ["cubence/claude-opus-4-5-20251101"],
      },
      modelCatalog: {
        "cubence/claude-sonnet-4-5-20250929": { alias: "Sonnet" },
        "cubence/claude-opus-4-5-20251101": { alias: "Opus" },
      },
    },
  },
  {
    name: "AIGoCode",
    websiteUrl: "https://aigocode.com",
    apiKeyUrl: "https://aigocode.com/invite/CC-SWITCH",
    settingsConfig: {
      baseUrl: "https://api.aigocode.com/v1",
      apiKey: "",
      api: "anthropic-messages",
      models: [
        {
          id: "claude-sonnet-4-5-20250929",
          name: "Claude Sonnet 4.5",
          contextWindow: 200000,
          cost: { input: 3, output: 15 },
        },
        {
          id: "claude-opus-4-5-20251101",
          name: "Claude Opus 4.5",
          contextWindow: 200000,
          cost: { input: 15, output: 75 },
        },
      ],
    },
    category: "third_party",
    isPartner: true,
    partnerPromotionKey: "aigocode",
    icon: "aigocode",
    iconColor: "#5B7FFF",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
    suggestedDefaults: {
      model: {
        primary: "aigocode/claude-sonnet-4-5-20250929",
        fallbacks: ["aigocode/claude-opus-4-5-20251101"],
      },
      modelCatalog: {
        "aigocode/claude-sonnet-4-5-20250929": { alias: "Sonnet" },
        "aigocode/claude-opus-4-5-20251101": { alias: "Opus" },
      },
    },
  },

  // ========== Custom Template ==========
  {
    name: "OpenAI Compatible",
    websiteUrl: "",
    settingsConfig: {
      baseUrl: "",
      apiKey: "",
      api: "openai-completions",
      models: [],
    },
    category: "custom",
    isCustomTemplate: true,
    icon: "generic",
    iconColor: "#6B7280",
    templateValues: {
      baseUrl: {
        label: "Base URL",
        placeholder: "https://api.example.com/v1",
        editorValue: "",
      },
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },
];
