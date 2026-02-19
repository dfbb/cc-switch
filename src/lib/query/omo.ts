import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { omoApi, omoSlimApi } from "@/lib/api/omo";
import * as configApi from "@/lib/api/config";
import type { OmoGlobalConfig } from "@/types/omo";

export const omoKeys = {
  all: ["omo"] as const,
  globalConfig: () => [...omoKeys.all, "global-config"] as const,
  currentProviderId: () => [...omoKeys.all, "current-provider-id"] as const,
  providerCount: () => [...omoKeys.all, "provider-count"] as const,
};

function invalidateOmoQueries(queryClient: ReturnType<typeof useQueryClient>) {
  queryClient.invalidateQueries({ queryKey: omoKeys.globalConfig() });
  queryClient.invalidateQueries({ queryKey: ["providers"] });
  queryClient.invalidateQueries({ queryKey: omoKeys.currentProviderId() });
  queryClient.invalidateQueries({ queryKey: omoKeys.providerCount() });
}

export function useOmoGlobalConfig(enabled = true) {
  return useQuery({
    queryKey: omoKeys.globalConfig(),
    enabled,
    queryFn: async (): Promise<OmoGlobalConfig> => {
      const raw = await configApi.getCommonConfigSnippet("omo");
      if (!raw) {
        return {
          id: "global",
          disabledAgents: [],
          disabledMcps: [],
          disabledHooks: [],
          disabledSkills: [],
          updatedAt: new Date().toISOString(),
        };
      }
      try {
        return JSON.parse(raw) as OmoGlobalConfig;
      } catch (error) {
        console.warn(
          "[omo] invalid global config json, fallback to defaults",
          error,
        );
        return {
          id: "global",
          disabledAgents: [],
          disabledMcps: [],
          disabledHooks: [],
          disabledSkills: [],
          updatedAt: new Date().toISOString(),
        };
      }
    },
  });
}

export function useCurrentOmoProviderId(enabled = true) {
  return useQuery({
    queryKey: omoKeys.currentProviderId(),
    queryFn: omoApi.getCurrentOmoProviderId,
    enabled,
  });
}

export function useOmoProviderCount(enabled = true) {
  return useQuery({
    queryKey: omoKeys.providerCount(),
    queryFn: omoApi.getOmoProviderCount,
    enabled,
  });
}

export function useSaveOmoGlobalConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: OmoGlobalConfig) => {
      const jsonStr = JSON.stringify(input);
      await configApi.setCommonConfigSnippet("omo", jsonStr);
    },
    onSuccess: () => invalidateOmoQueries(queryClient),
  });
}

export function useReadOmoLocalFile() {
  return useMutation({
    mutationFn: () => omoApi.readLocalFile(),
  });
}

export function useDisableCurrentOmo() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => omoApi.disableCurrentOmo(),
    onSuccess: () => invalidateOmoQueries(queryClient),
  });
}

// ============================================================================
// OMO Slim query hooks
// ============================================================================

export const omoSlimKeys = {
  all: ["omo-slim"] as const,
  globalConfig: () => [...omoSlimKeys.all, "global-config"] as const,
  currentProviderId: () => [...omoSlimKeys.all, "current-provider-id"] as const,
  providerCount: () => [...omoSlimKeys.all, "provider-count"] as const,
};

function invalidateOmoSlimQueries(
  queryClient: ReturnType<typeof useQueryClient>,
) {
  queryClient.invalidateQueries({ queryKey: omoSlimKeys.globalConfig() });
  queryClient.invalidateQueries({ queryKey: ["providers"] });
  queryClient.invalidateQueries({
    queryKey: omoSlimKeys.currentProviderId(),
  });
  queryClient.invalidateQueries({ queryKey: omoSlimKeys.providerCount() });
}

export function useOmoSlimGlobalConfig(enabled = true) {
  return useQuery({
    queryKey: omoSlimKeys.globalConfig(),
    enabled,
    queryFn: async (): Promise<OmoGlobalConfig> => {
      const raw = await configApi.getCommonConfigSnippet("omo_slim");
      if (!raw) {
        return {
          id: "global",
          disabledAgents: [],
          disabledMcps: [],
          disabledHooks: [],
          disabledSkills: [],
          updatedAt: new Date().toISOString(),
        };
      }
      try {
        return JSON.parse(raw) as OmoGlobalConfig;
      } catch (error) {
        console.warn(
          "[omo-slim] invalid global config json, fallback to defaults",
          error,
        );
        return {
          id: "global",
          disabledAgents: [],
          disabledMcps: [],
          disabledHooks: [],
          disabledSkills: [],
          updatedAt: new Date().toISOString(),
        };
      }
    },
  });
}

export function useCurrentOmoSlimProviderId(enabled = true) {
  return useQuery({
    queryKey: omoSlimKeys.currentProviderId(),
    queryFn: omoSlimApi.getCurrentProviderId,
    enabled,
  });
}

export function useOmoSlimProviderCount(enabled = true) {
  return useQuery({
    queryKey: omoSlimKeys.providerCount(),
    queryFn: omoSlimApi.getProviderCount,
    enabled,
  });
}

export function useSaveOmoSlimGlobalConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: OmoGlobalConfig) => {
      const jsonStr = JSON.stringify(input);
      await configApi.setCommonConfigSnippet("omo_slim", jsonStr);
    },
    onSuccess: () => invalidateOmoSlimQueries(queryClient),
  });
}

export function useReadOmoSlimLocalFile() {
  return useMutation({
    mutationFn: () => omoSlimApi.readLocalFile(),
  });
}

export function useDisableCurrentOmoSlim() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => omoSlimApi.disableCurrent(),
    onSuccess: () => invalidateOmoSlimQueries(queryClient),
  });
}
