import { useTranslation } from "react-i18next";
import { FormLabel } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Plus, Trash2 } from "lucide-react";
import { ApiKeySection } from "./shared";
import { opencodeNpmPackages } from "@/config/opencodeProviderPresets";
import type { ProviderCategory, OpenCodeModel } from "@/types";

interface OpenCodeFormFieldsProps {
  // NPM Package
  npm: string;
  onNpmChange: (value: string) => void;

  // API Key
  apiKey: string;
  onApiKeyChange: (value: string) => void;
  category?: ProviderCategory;
  shouldShowApiKeyLink: boolean;
  websiteUrl: string;

  // Base URL
  baseUrl: string;
  onBaseUrlChange: (value: string) => void;

  // Models
  models: Record<string, OpenCodeModel>;
  onModelsChange: (models: Record<string, OpenCodeModel>) => void;
}

export function OpenCodeFormFields({
  npm,
  onNpmChange,
  apiKey,
  onApiKeyChange,
  category,
  shouldShowApiKeyLink,
  websiteUrl,
  baseUrl,
  onBaseUrlChange,
  models,
  onModelsChange,
}: OpenCodeFormFieldsProps) {
  const { t } = useTranslation();

  // Add a new model entry
  const handleAddModel = () => {
    const newKey = `model-${Date.now()}`;
    onModelsChange({
      ...models,
      [newKey]: { name: "" },
    });
  };

  // Remove a model entry
  const handleRemoveModel = (key: string) => {
    const newModels = { ...models };
    delete newModels[key];
    onModelsChange(newModels);
  };

  // Update model ID (key)
  const handleModelIdChange = (oldKey: string, newKey: string) => {
    if (oldKey === newKey || !newKey.trim()) return;
    const newModels: Record<string, OpenCodeModel> = {};
    for (const [k, v] of Object.entries(models)) {
      if (k === oldKey) {
        newModels[newKey] = v;
      } else {
        newModels[k] = v;
      }
    }
    onModelsChange(newModels);
  };

  // Update model name
  const handleModelNameChange = (key: string, name: string) => {
    onModelsChange({
      ...models,
      [key]: { ...models[key], name },
    });
  };

  return (
    <>
      {/* NPM Package Selector */}
      <div className="space-y-2">
        <FormLabel htmlFor="opencode-npm">
          {t("opencode.npmPackage", {
            defaultValue: "接口格式",
          })}
        </FormLabel>
        <Select value={npm} onValueChange={onNpmChange}>
          <SelectTrigger id="opencode-npm">
            <SelectValue
              placeholder={t("opencode.selectPackage", {
                defaultValue: "Select a package",
              })}
            />
          </SelectTrigger>
          <SelectContent>
            {opencodeNpmPackages.map((pkg) => (
              <SelectItem key={pkg.value} value={pkg.value}>
                {pkg.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {t("opencode.npmPackageHint", {
            defaultValue:
              "Select the AI SDK package that matches your provider.",
          })}
        </p>
      </div>

      {/* API Key */}
      <ApiKeySection
        value={apiKey}
        onChange={onApiKeyChange}
        category={category}
        shouldShowLink={shouldShowApiKeyLink}
        websiteUrl={websiteUrl}
      />

      {/* Base URL (only for compatible providers) */}
      {npm === "@ai-sdk/openai-compatible" && (
        <div className="space-y-2">
          <FormLabel htmlFor="opencode-baseurl">
            {t("opencode.baseUrl", { defaultValue: "Base URL" })}
          </FormLabel>
          <Input
            id="opencode-baseurl"
            value={baseUrl}
            onChange={(e) => onBaseUrlChange(e.target.value)}
            placeholder="https://api.example.com/v1"
          />
          <p className="text-xs text-muted-foreground">
            {t("opencode.baseUrlHint", {
              defaultValue:
                "The base URL for OpenAI-compatible API endpoints.",
            })}
          </p>
        </div>
      )}

      {/* Models Editor */}
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <FormLabel>
            {t("opencode.models", { defaultValue: "Models" })}
          </FormLabel>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleAddModel}
            className="h-7 gap-1"
          >
            <Plus className="h-3.5 w-3.5" />
            {t("opencode.addModel", { defaultValue: "Add" })}
          </Button>
        </div>

        {Object.keys(models).length === 0 ? (
          <p className="text-sm text-muted-foreground py-2">
            {t("opencode.noModels", {
              defaultValue: "No models configured. Click Add to add a model.",
            })}
          </p>
        ) : (
          <div className="space-y-2">
            {Object.entries(models).map(([key, model]) => (
              <div key={key} className="flex items-center gap-2">
                <Input
                  value={key}
                  onChange={(e) => handleModelIdChange(key, e.target.value)}
                  placeholder={t("opencode.modelId", {
                    defaultValue: "Model ID",
                  })}
                  className="flex-1"
                />
                <Input
                  value={model.name}
                  onChange={(e) => handleModelNameChange(key, e.target.value)}
                  placeholder={t("opencode.modelName", {
                    defaultValue: "Display Name",
                  })}
                  className="flex-1"
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  onClick={() => handleRemoveModel(key)}
                  className="h-9 w-9 text-muted-foreground hover:text-destructive"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            ))}
          </div>
        )}

        <p className="text-xs text-muted-foreground">
          {t("opencode.modelsHint", {
            defaultValue:
              "Configure available models. Model ID is the API identifier, Display Name is shown in the UI.",
          })}
        </p>
      </div>
    </>
  );
}
