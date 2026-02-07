import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2, Save } from "lucide-react";
import { toast } from "sonner";
import { openclawApi } from "@/lib/api/openclaw";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { OpenClawToolsConfig } from "@/types";

const PROFILE_OPTIONS = ["default", "strict", "permissive", "custom"];

const ToolsPanel: React.FC = () => {
  const { t } = useTranslation();
  const [config, setConfig] = useState<OpenClawToolsConfig>({});
  const [allowList, setAllowList] = useState<string[]>([]);
  const [denyList, setDenyList] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  const loadTools = useCallback(async () => {
    try {
      setLoading(true);
      const tools = await openclawApi.getTools();
      setConfig(tools);
      setAllowList(tools.allow ?? []);
      setDenyList(tools.deny ?? []);
    } catch (err) {
      toast.error(t("openclaw.tools.loadFailed"));
      console.error("Failed to load tools config:", err);
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void loadTools();
  }, [loadTools]);

  const handleSave = async () => {
    try {
      setSaving(true);
      const { profile, allow, deny, ...other } = config;
      const newConfig: OpenClawToolsConfig = {
        ...other,
        profile: config.profile,
        allow: allowList.filter((s) => s.trim()),
        deny: denyList.filter((s) => s.trim()),
      };
      await openclawApi.setTools(newConfig);
      toast.success(t("openclaw.tools.saveSuccess"));
      await loadTools();
    } catch (err) {
      toast.error(t("openclaw.tools.saveFailed"));
      console.error("Failed to save tools config:", err);
    } finally {
      setSaving(false);
    }
  };

  const updateListItem = (
    list: string[],
    setList: React.Dispatch<React.SetStateAction<string[]>>,
    index: number,
    value: string,
  ) => {
    setList(list.map((item, i) => (i === index ? value : item)));
  };

  const removeListItem = (
    list: string[],
    setList: React.Dispatch<React.SetStateAction<string[]>>,
    index: number,
  ) => {
    setList(list.filter((_, i) => i !== index));
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
        {t("openclaw.tools.description")}
      </p>

      {/* Profile selector */}
      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.profile")}</Label>
        <Select
          value={config.profile ?? "default"}
          onValueChange={(val) =>
            setConfig((prev) => ({ ...prev, profile: val }))
          }
        >
          <SelectTrigger className="w-[200px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PROFILE_OPTIONS.map((opt) => (
              <SelectItem key={opt} value={opt}>
                {t(`openclaw.tools.profiles.${opt}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Allow list */}
      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.allowList")}</Label>
        <div className="space-y-2">
          {allowList.map((item, index) => (
            <div key={index} className="flex items-center gap-2">
              <Input
                value={item}
                onChange={(e) =>
                  updateListItem(allowList, setAllowList, index, e.target.value)
                }
                placeholder={t("openclaw.tools.patternPlaceholder")}
                className="font-mono text-xs"
              />
              <Button
                variant="ghost"
                size="icon"
                className="flex-shrink-0 h-9 w-9 text-muted-foreground hover:text-destructive"
                onClick={() => removeListItem(allowList, setAllowList, index)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            size="sm"
            onClick={() => setAllowList((prev) => [...prev, ""])}
          >
            <Plus className="w-4 h-4 mr-1" />
            {t("openclaw.tools.addAllow")}
          </Button>
        </div>
      </div>

      {/* Deny list */}
      <div className="mb-6">
        <Label className="mb-2 block">{t("openclaw.tools.denyList")}</Label>
        <div className="space-y-2">
          {denyList.map((item, index) => (
            <div key={index} className="flex items-center gap-2">
              <Input
                value={item}
                onChange={(e) =>
                  updateListItem(denyList, setDenyList, index, e.target.value)
                }
                placeholder={t("openclaw.tools.patternPlaceholder")}
                className="font-mono text-xs"
              />
              <Button
                variant="ghost"
                size="icon"
                className="flex-shrink-0 h-9 w-9 text-muted-foreground hover:text-destructive"
                onClick={() => removeListItem(denyList, setDenyList, index)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          ))}
          <Button
            variant="outline"
            size="sm"
            onClick={() => setDenyList((prev) => [...prev, ""])}
          >
            <Plus className="w-4 h-4 mr-1" />
            {t("openclaw.tools.addDeny")}
          </Button>
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

export default ToolsPanel;
