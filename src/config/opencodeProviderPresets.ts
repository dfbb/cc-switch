/**
 * OpenCode 预设供应商配置模板
 * OpenCode 使用 AI SDK npm 包，配置结构与其他应用不同
 */
import type { ProviderCategory, OpenCodeProviderConfig } from "../types";
import type { PresetTheme, TemplateValueConfig } from "./claudeProviderPresets";

export interface OpenCodeProviderPreset {
  name: string;
  websiteUrl: string;
  apiKeyUrl?: string;
  /** OpenCode settings_config 结构 */
  settingsConfig: OpenCodeProviderConfig;
  isOfficial?: boolean;
  isPartner?: boolean;
  partnerPromotionKey?: string;
  category?: ProviderCategory;
  /** 模板变量定义 */
  templateValues?: Record<string, TemplateValueConfig>;
  /** 视觉主题配置 */
  theme?: PresetTheme;
  /** 图标名称 */
  icon?: string;
  /** 图标颜色 */
  iconColor?: string;
  /** 标记为自定义模板（用于 UI 区分） */
  isCustomTemplate?: boolean;
}

/**
 * OpenCode npm 包选项（AI SDK 生态）
 */
export const opencodeNpmPackages = [
  { value: "@ai-sdk/openai", label: "OpenAI" },
  { value: "@ai-sdk/openai-compatible", label: "OpenAI Compatible" },
  { value: "@ai-sdk/anthropic", label: "Anthropic" },
  { value: "@ai-sdk/google", label: "Google (Gemini)" },
] as const;

/**
 * OpenCode 供应商预设列表
 */
export const opencodeProviderPresets: OpenCodeProviderPreset[] = [
  // ========== 官方供应商 ==========
  {
    name: "OpenAI",
    websiteUrl: "https://platform.openai.com",
    apiKeyUrl: "https://platform.openai.com/api-keys",
    settingsConfig: {
      npm: "@ai-sdk/openai",
      options: {
        apiKey: "{env:OPENAI_API_KEY}",
      },
      models: {
        "gpt-4o": { name: "GPT-4o" },
        "gpt-4o-mini": { name: "GPT-4o Mini" },
        "o1": { name: "o1" },
        "o1-mini": { name: "o1 Mini" },
        "o3-mini": { name: "o3 Mini" },
      },
    },
    isOfficial: true,
    category: "official",
    icon: "openai",
    iconColor: "#00A67E",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-...",
        editorValue: "",
      },
    },
  },
  {
    name: "Anthropic",
    websiteUrl: "https://console.anthropic.com",
    apiKeyUrl: "https://console.anthropic.com/settings/keys",
    settingsConfig: {
      npm: "@ai-sdk/anthropic",
      options: {
        apiKey: "{env:ANTHROPIC_API_KEY}",
      },
      models: {
        "claude-sonnet-4-20250514": { name: "Claude Sonnet 4" },
        "claude-3-5-sonnet-20241022": { name: "Claude 3.5 Sonnet" },
        "claude-3-5-haiku-20241022": { name: "Claude 3.5 Haiku" },
        "claude-3-opus-20240229": { name: "Claude 3 Opus" },
      },
    },
    isOfficial: true,
    category: "official",
    icon: "anthropic",
    iconColor: "#D4915D",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "sk-ant-...",
        editorValue: "",
      },
    },
  },
  {
    name: "Google (Gemini)",
    websiteUrl: "https://ai.google.dev",
    apiKeyUrl: "https://aistudio.google.com/app/apikey",
    settingsConfig: {
      npm: "@ai-sdk/google",
      options: {
        apiKey: "{env:GOOGLE_GENERATIVE_AI_API_KEY}",
      },
      models: {
        "gemini-2.0-flash": { name: "Gemini 2.0 Flash" },
        "gemini-2.0-flash-lite": { name: "Gemini 2.0 Flash Lite" },
        "gemini-1.5-pro": { name: "Gemini 1.5 Pro" },
        "gemini-1.5-flash": { name: "Gemini 1.5 Flash" },
      },
    },
    isOfficial: true,
    category: "official",
    icon: "gemini",
    iconColor: "#4285F4",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "AIza...",
        editorValue: "",
      },
    },
  },

  // ========== 国产官方 ==========
  {
    name: "DeepSeek",
    websiteUrl: "https://platform.deepseek.com",
    apiKeyUrl: "https://platform.deepseek.com/api_keys",
    settingsConfig: {
      npm: "@ai-sdk/deepseek",
      options: {
        apiKey: "{env:DEEPSEEK_API_KEY}",
      },
      models: {
        "deepseek-chat": { name: "DeepSeek V3" },
        "deepseek-reasoner": { name: "DeepSeek R1" },
      },
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
  },
  {
    name: "Mistral",
    websiteUrl: "https://mistral.ai",
    apiKeyUrl: "https://console.mistral.ai/api-keys",
    settingsConfig: {
      npm: "@ai-sdk/mistral",
      options: {
        apiKey: "{env:MISTRAL_API_KEY}",
      },
      models: {
        "mistral-large-latest": { name: "Mistral Large" },
        "mistral-small-latest": { name: "Mistral Small" },
        "codestral-latest": { name: "Codestral" },
      },
    },
    category: "official",
    icon: "mistral",
    iconColor: "#FF7000",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "",
        editorValue: "",
      },
    },
  },
  {
    name: "Groq",
    websiteUrl: "https://groq.com",
    apiKeyUrl: "https://console.groq.com/keys",
    settingsConfig: {
      npm: "@ai-sdk/groq",
      options: {
        apiKey: "{env:GROQ_API_KEY}",
      },
      models: {
        "llama-3.3-70b-versatile": { name: "Llama 3.3 70B" },
        "llama-3.1-8b-instant": { name: "Llama 3.1 8B" },
        "mixtral-8x7b-32768": { name: "Mixtral 8x7B" },
      },
    },
    category: "official",
    icon: "groq",
    iconColor: "#F55036",
    templateValues: {
      apiKey: {
        label: "API Key",
        placeholder: "gsk_...",
        editorValue: "",
      },
    },
  },

  // ========== 自定义模板 ==========
  {
    name: "OpenAI Compatible",
    websiteUrl: "",
    settingsConfig: {
      npm: "@ai-sdk/openai-compatible",
      options: {
        baseURL: "",
        apiKey: "",
      },
      models: {},
    },
    category: "custom",
    isCustomTemplate: true,
    icon: "generic",
    iconColor: "#6B7280",
    templateValues: {
      baseURL: {
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
