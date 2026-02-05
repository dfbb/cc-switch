import { invoke } from "@tauri-apps/api/core";
import type { OpenClawDefaultModel, OpenClawModelCatalogEntry } from "@/types";

/**
 * OpenClaw agents configuration API
 *
 * Manages agents.defaults configuration in ~/.openclaw/openclaw.json
 */
export const openclawApi = {
  /**
   * Get default model configuration (agents.defaults.model)
   */
  async getDefaultModel(): Promise<OpenClawDefaultModel | null> {
    return await invoke("get_openclaw_default_model");
  },

  /**
   * Set default model configuration (agents.defaults.model)
   */
  async setDefaultModel(model: OpenClawDefaultModel): Promise<void> {
    return await invoke("set_openclaw_default_model", { model });
  },

  /**
   * Get model catalog/allowlist (agents.defaults.models)
   */
  async getModelCatalog(): Promise<Record<
    string,
    OpenClawModelCatalogEntry
  > | null> {
    return await invoke("get_openclaw_model_catalog");
  },

  /**
   * Set model catalog/allowlist (agents.defaults.models)
   */
  async setModelCatalog(
    catalog: Record<string, OpenClawModelCatalogEntry>,
  ): Promise<void> {
    return await invoke("set_openclaw_model_catalog", { catalog });
  },

  /**
   * Import providers from live config (openclaw.json) to database
   */
  async importProvidersFromLive(): Promise<number> {
    return await invoke("import_openclaw_providers_from_live");
  },

  /**
   * Get provider IDs that exist in live config (openclaw.json)
   */
  async getLiveProviderIds(): Promise<string[]> {
    return await invoke("get_openclaw_live_provider_ids");
  },
};
