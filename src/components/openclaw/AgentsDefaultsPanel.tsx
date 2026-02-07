import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Save } from "lucide-react";
import { toast } from "sonner";
import { openclawApi } from "@/lib/api/openclaw";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { OpenClawAgentsDefaults } from "@/types";

const AgentsDefaultsPanel: React.FC = () => {
  const { t } = useTranslation();
  const [defaults, setDefaults] = useState<OpenClawAgentsDefaults | null>(null);
  const [primaryModel, setPrimaryModel] = useState("");
  const [fallbacks, setFallbacks] = useState("");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  // Extra known fields from agents.defaults
  const [workspace, setWorkspace] = useState("");
  const [timeout, setTimeout_] = useState("");
  const [contextTokens, setContextTokens] = useState("");
  const [maxConcurrent, setMaxConcurrent] = useState("");

  const loadDefaults = useCallback(async () => {
    try {
      setLoading(true);
      const data = await openclawApi.getAgentsDefaults();
      setDefaults(data);

      if (data) {
        setPrimaryModel(data.model?.primary ?? "");
        setFallbacks((data.model?.fallbacks ?? []).join(", "));

        // Extract known extra fields
        setWorkspace(String(data.workspace ?? ""));
        setTimeout_(String(data.timeout ?? ""));
        setContextTokens(String(data.contextTokens ?? ""));
        setMaxConcurrent(String(data.maxConcurrent ?? ""));
      }
    } catch (err) {
      toast.error(t("openclaw.agents.loadFailed"));
      console.error("Failed to load agents defaults:", err);
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void loadDefaults();
  }, [loadDefaults]);

  const handleSave = async () => {
    try {
      setSaving(true);

      // Preserve all unknown fields from original data
      const updated: OpenClawAgentsDefaults = { ...defaults };

      // Model configuration
      const fallbackList = fallbacks
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);

      if (primaryModel.trim()) {
        updated.model = {
          primary: primaryModel.trim(),
          ...(fallbackList.length > 0 ? { fallbacks: fallbackList } : {}),
        };
      }

      // Optional numeric fields
      if (workspace.trim()) updated.workspace = workspace.trim();
      else delete updated.workspace;

      if (timeout.trim()) updated.timeout = Number(timeout);
      else delete updated.timeout;

      if (contextTokens.trim()) updated.contextTokens = Number(contextTokens);
      else delete updated.contextTokens;

      if (maxConcurrent.trim()) updated.maxConcurrent = Number(maxConcurrent);
      else delete updated.maxConcurrent;

      await openclawApi.setAgentsDefaults(updated);
      toast.success(t("openclaw.agents.saveSuccess"));
      await loadDefaults();
    } catch (err) {
      toast.error(t("openclaw.agents.saveFailed"));
      console.error("Failed to save agents defaults:", err);
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="px-6 pt-4 pb-8 flex items-center justify-center min-h-[200px]">
        <div className="text-sm text-muted-foreground">
          {t("common.loading")}
        </div>
      </div>
    );
  }

  return (
    <div className="px-6 pt-4 pb-8">
      <p className="text-sm text-muted-foreground mb-6">
        {t("openclaw.agents.description")}
      </p>

      {/* Model Configuration Card */}
      <div className="rounded-xl border border-border bg-card p-5 mb-4">
        <h3 className="text-sm font-medium mb-4">
          {t("openclaw.agents.modelSection")}
        </h3>

        <div className="space-y-4">
          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.primaryModel")}
            </Label>
            <Input
              value={primaryModel}
              onChange={(e) => setPrimaryModel(e.target.value)}
              placeholder="provider/model-id"
              className="font-mono text-xs"
            />
            <p className="text-xs text-muted-foreground mt-1">
              {t("openclaw.agents.primaryModelHint")}
            </p>
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.fallbackModels")}
            </Label>
            <Input
              value={fallbacks}
              onChange={(e) => setFallbacks(e.target.value)}
              placeholder="provider/model-a, provider/model-b"
              className="font-mono text-xs"
            />
            <p className="text-xs text-muted-foreground mt-1">
              {t("openclaw.agents.fallbackModelsHint")}
            </p>
          </div>
        </div>
      </div>

      {/* Runtime Parameters Card */}
      <div className="rounded-xl border border-border bg-card p-5 mb-4">
        <h3 className="text-sm font-medium mb-4">
          {t("openclaw.agents.runtimeSection")}
        </h3>

        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.workspace")}
            </Label>
            <Input
              value={workspace}
              onChange={(e) => setWorkspace(e.target.value)}
              placeholder="~/projects"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.timeout")}
            </Label>
            <Input
              type="number"
              value={timeout}
              onChange={(e) => setTimeout_(e.target.value)}
              placeholder="300"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.contextTokens")}
            </Label>
            <Input
              type="number"
              value={contextTokens}
              onChange={(e) => setContextTokens(e.target.value)}
              placeholder="200000"
              className="font-mono text-xs"
            />
          </div>

          <div>
            <Label className="mb-1.5 block">
              {t("openclaw.agents.maxConcurrent")}
            </Label>
            <Input
              type="number"
              value={maxConcurrent}
              onChange={(e) => setMaxConcurrent(e.target.value)}
              placeholder="4"
              className="font-mono text-xs"
            />
          </div>
        </div>
      </div>

      {/* Save button */}
      <div className="flex justify-end">
        <Button size="sm" onClick={handleSave} disabled={saving}>
          <Save className="w-4 h-4 mr-1" />
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default AgentsDefaultsPanel;
