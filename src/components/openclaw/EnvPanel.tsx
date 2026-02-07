import React, { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2, Save, Eye, EyeOff } from "lucide-react";
import { toast } from "sonner";
import { openclawApi } from "@/lib/api/openclaw";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { OpenClawEnvConfig } from "@/types";

interface EnvEntry {
  key: string;
  value: string;
  isNew?: boolean;
}

const EnvPanel: React.FC = () => {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<EnvEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [visibleKeys, setVisibleKeys] = useState<Set<string>>(new Set());

  const loadEnv = useCallback(async () => {
    try {
      setLoading(true);
      const env = await openclawApi.getEnv();
      const items: EnvEntry[] = Object.entries(env).map(([key, value]) => ({
        key,
        value: String(value ?? ""),
      }));
      setEntries(items.length > 0 ? items : []);
    } catch (err) {
      toast.error(t("openclaw.env.loadFailed"));
      console.error("Failed to load env config:", err);
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void loadEnv();
  }, [loadEnv]);

  const handleSave = async () => {
    try {
      setSaving(true);
      const env: OpenClawEnvConfig = {};
      for (const entry of entries) {
        const trimmedKey = entry.key.trim();
        if (trimmedKey) {
          env[trimmedKey] = entry.value;
        }
      }
      await openclawApi.setEnv(env);
      toast.success(t("openclaw.env.saveSuccess"));
      // Reload to normalize
      await loadEnv();
    } catch (err) {
      toast.error(t("openclaw.env.saveFailed"));
      console.error("Failed to save env config:", err);
    } finally {
      setSaving(false);
    }
  };

  const addEntry = () => {
    setEntries((prev) => [...prev, { key: "", value: "", isNew: true }]);
  };

  const removeEntry = (index: number) => {
    setEntries((prev) => prev.filter((_, i) => i !== index));
  };

  const updateEntry = (index: number, field: "key" | "value", val: string) => {
    setEntries((prev) =>
      prev.map((entry, i) =>
        i === index ? { ...entry, [field]: val } : entry,
      ),
    );
  };

  const toggleVisibility = (key: string) => {
    setVisibleKeys((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  };

  const isApiKey = (key: string) => /key|token|secret|password/i.test(key);

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
      <p className="text-sm text-muted-foreground mb-4">
        {t("openclaw.env.description")}
      </p>

      <div className="space-y-3">
        {entries.map((entry, index) => {
          const sensitive = isApiKey(entry.key);
          const visible = visibleKeys.has(`${index}`);

          return (
            <div key={index} className="flex items-center gap-2">
              <div className="w-[200px] flex-shrink-0">
                <Input
                  value={entry.key}
                  onChange={(e) => updateEntry(index, "key", e.target.value)}
                  placeholder={t("openclaw.env.keyPlaceholder")}
                  className="font-mono text-xs"
                  autoFocus={entry.isNew}
                />
              </div>
              <div className="flex-1 flex items-center gap-1">
                <Input
                  type={sensitive && !visible ? "password" : "text"}
                  value={entry.value}
                  onChange={(e) => updateEntry(index, "value", e.target.value)}
                  placeholder={t("openclaw.env.valuePlaceholder")}
                  className="font-mono text-xs"
                />
                {sensitive && (
                  <Button
                    variant="ghost"
                    size="icon"
                    className="flex-shrink-0 h-9 w-9 text-muted-foreground"
                    onClick={() => toggleVisibility(`${index}`)}
                  >
                    {visible ? (
                      <EyeOff className="w-4 h-4" />
                    ) : (
                      <Eye className="w-4 h-4" />
                    )}
                  </Button>
                )}
              </div>
              <Button
                variant="ghost"
                size="icon"
                className="flex-shrink-0 h-9 w-9 text-muted-foreground hover:text-destructive"
                onClick={() => removeEntry(index)}
              >
                <Trash2 className="w-4 h-4" />
              </Button>
            </div>
          );
        })}
      </div>

      <div className="flex items-center gap-2 mt-4">
        <Button variant="outline" size="sm" onClick={addEntry}>
          <Plus className="w-4 h-4 mr-1" />
          {t("openclaw.env.add")}
        </Button>
        <div className="flex-1" />
        <Button size="sm" onClick={handleSave} disabled={saving}>
          <Save className="w-4 h-4 mr-1" />
          {saving ? t("common.saving") : t("common.save")}
        </Button>
      </div>
    </div>
  );
};

export default EnvPanel;
