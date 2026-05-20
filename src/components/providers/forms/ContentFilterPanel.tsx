import { useState } from "react";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { ExtensionFilterConfig } from "@/types";

interface ExtensionDef {
  key: string;
  label: string;
  group: string;
  defaultEnabled: boolean;
}

const ALL_EXTENSIONS: ExtensionDef[] = [
  // 缓存修复
  { key: "upstream-change-detection", label: "上游模型变更检测", group: "缓存修复", defaultEnabled: true },
  { key: "ttl-tier-detect", label: "TTL 层级检测", group: "缓存修复", defaultEnabled: true },
  { key: "fingerprint-strip", label: "指纹移除", group: "缓存修复", defaultEnabled: true },
  { key: "sort-stabilization", label: "排序稳定化", group: "缓存修复", defaultEnabled: true },
  { key: "fresh-session-sort", label: "新会话排序", group: "缓存修复", defaultEnabled: true },
  { key: "identity-normalization", label: "身份规范化", group: "缓存修复", defaultEnabled: true },
  { key: "cache-control-normalize", label: "缓存控制规范化", group: "缓存修复", defaultEnabled: true },
  { key: "messages-cache-breakpoint", label: "消息缓存断点", group: "缓存修复", defaultEnabled: true },
  { key: "ttl-management", label: "TTL 管理", group: "缓存修复", defaultEnabled: true },
  { key: "microcompact-stability", label: "微压缩稳定", group: "缓存修复", defaultEnabled: true },
  // 内容处理
  { key: "image-strip", label: "图片移除", group: "内容处理", defaultEnabled: true },
  { key: "content-strip", label: "内容移除", group: "内容处理", defaultEnabled: true },
  { key: "tool-input-normalize", label: "工具输入规范化", group: "内容处理", defaultEnabled: true },
  { key: "smoosh-split", label: "Smoosh 分割", group: "内容处理", defaultEnabled: true },
  { key: "thinking-display", label: "思考显示", group: "内容处理", defaultEnabled: true },
  { key: "output-efficiency-rewrite", label: "输出效率优化", group: "内容处理", defaultEnabled: false },
  { key: "prefix-diff", label: "前缀差异", group: "内容处理", defaultEnabled: true },
  { key: "deferred-tools-restore", label: "延迟工具恢复", group: "内容处理", defaultEnabled: true },
  // 监控日志
  { key: "cache-telemetry", label: "缓存遥测", group: "监控日志", defaultEnabled: true },
  { key: "overage-warning", label: "超额警告", group: "监控日志", defaultEnabled: true },
  { key: "rate-limit-log", label: "速率限制日志", group: "监控日志", defaultEnabled: false },
  { key: "usage-log", label: "用量日志", group: "监控日志", defaultEnabled: false },
  { key: "request-log", label: "请求日志", group: "监控日志", defaultEnabled: false },
];

/** 完整模式：全部 23 个 extension */
const FULL_PRESET: Record<string, boolean> = Object.fromEntries(
  ALL_EXTENSIONS.map((ext) => [ext.key, true]),
);

/** 仅缓存修复：15 个核心缓存相关 extension */
const CACHE_ONLY_KEYS = new Set([
  "upstream-change-detection",
  "ttl-tier-detect",
  "fingerprint-strip",
  "sort-stabilization",
  "fresh-session-sort",
  "identity-normalization",
  "cache-control-normalize",
  "messages-cache-breakpoint",
  "ttl-management",
  "microcompact-stability",
  "image-strip",
  "content-strip",
  "tool-input-normalize",
  "smoosh-split",
  "thinking-display",
]);
const CACHE_ONLY_PRESET: Record<string, boolean> = Object.fromEntries(
  ALL_EXTENSIONS.map((ext) => [ext.key, CACHE_ONLY_KEYS.has(ext.key)]),
);

/** 最小模式：仅 fingerprint + identity */
const MINIMAL_KEYS = new Set(["fingerprint-strip", "identity-normalization"]);
const MINIMAL_PRESET: Record<string, boolean> = Object.fromEntries(
  ALL_EXTENSIONS.map((ext) => [ext.key, MINIMAL_KEYS.has(ext.key)]),
);

function buildPresetExtensions(
  preset: "full" | "cache-only" | "minimal",
): Record<string, boolean> {
  switch (preset) {
    case "full":
      return { ...FULL_PRESET };
    case "cache-only":
      return { ...CACHE_ONLY_PRESET };
    case "minimal":
      return { ...MINIMAL_PRESET };
  }
}

function derivePresetFromExtensions(
  extensions: Record<string, boolean>,
): "full" | "cache-only" | "minimal" | null {
  const keys = Object.keys(extensions);
  if (keys.length === 0) return null;
  const fullMatch = keys.every((k) => extensions[k] === FULL_PRESET[k]);
  if (fullMatch) return "full";
  const cacheMatch =
    keys.every((k) => extensions[k] === CACHE_ONLY_PRESET[k]) &&
    keys.some((k) => extensions[k] === true);
  if (cacheMatch) return "cache-only";
  const minimalMatch = keys.every((k) => extensions[k] === MINIMAL_PRESET[k]);
  if (minimalMatch) return "minimal";
  return null;
}

interface Props {
  value: ExtensionFilterConfig | undefined;
  onChange: (v: ExtensionFilterConfig) => void;
}

export function ContentFilterPanel({ value, onChange }: Props) {
  const enabled = value?.enabled ?? false;
  const extensions = value?.extensions ?? {};
  const preset = value?.preset ?? null;

  const [expandedGroups, setExpandedGroups] = useState<Record<string, boolean>>({
    "缓存修复": true,
    "内容处理": true,
    "监控日志": false,
  });

  const handleEnabledChange = (checked: boolean) => {
    const nextExtensions =
      checked && Object.keys(extensions).length === 0
        ? { ...FULL_PRESET }
        : extensions;
    onChange({
      enabled: checked,
      extensions: nextExtensions,
      preset: checked ? derivePresetFromExtensions(nextExtensions) : null,
    });
  };

  const handlePresetSelect = (nextPreset: "full" | "cache-only" | "minimal") => {
    if (preset === nextPreset) return;
    const nextExtensions = buildPresetExtensions(nextPreset);
    onChange({
      enabled: true,
      extensions: nextExtensions,
      preset: nextPreset,
    });
  };

  const handleExtensionToggle = (key: string, checked: boolean) => {
    const nextExtensions = { ...extensions, [key]: checked };
    onChange({
      enabled,
      extensions: nextExtensions,
      preset: derivePresetFromExtensions(nextExtensions),
    });
  };

  const toggleGroup = (group: string) => {
    setExpandedGroups((prev) => ({ ...prev, [group]: !prev[group] }));
  };

  const grouped = new Map<string, ExtensionDef[]>();
  for (const ext of ALL_EXTENSIONS) {
    const list = grouped.get(ext.group) ?? [];
    list.push(ext);
    grouped.set(ext.group, list);
  }

  const presetButtons: Array<{
    key: "full" | "cache-only" | "minimal";
    label: string;
    desc: string;
  }> = [
    { key: "full", label: "完整模式", desc: "启用全部 23 个扩展" },
    { key: "cache-only", label: "仅缓存修复", desc: "启用 15 个核心缓存扩展" },
    { key: "minimal", label: "最小模式", desc: "仅指纹 + 身份规范化" },
  ];

  return (
    <div className="space-y-4 rounded-lg border p-4">
      {/* 全局开关 */}
      <div className="flex items-center justify-between">
        <div>
          <Label className="text-sm font-medium">Extension 内容过滤</Label>
          <p className="text-xs text-muted-foreground">
            对通过本地代理的请求/响应执行内容变换管线
          </p>
        </div>
        <Switch checked={enabled} onCheckedChange={handleEnabledChange} />
      </div>

      {enabled && (
        <>
          {/* 预设选择 */}
          <div className="space-y-2">
            <Label className="text-xs font-medium text-muted-foreground">
              快速预设
            </Label>
            <div className="flex gap-2">
              {presetButtons.map((btn) => (
                <button
                  key={btn.key}
                  type="button"
                  onClick={() => handlePresetSelect(btn.key)}
                  className={`flex-1 rounded-md border px-3 py-2 text-left text-xs transition-colors ${
                    preset === btn.key
                      ? "border-emerald-500 bg-emerald-50 dark:border-emerald-600 dark:bg-emerald-950"
                      : "border-input hover:bg-muted"
                  }`}
                >
                  <div className="font-medium">{btn.label}</div>
                  <div className="text-muted-foreground">{btn.desc}</div>
                </button>
              ))}
            </div>
          </div>

          {/* 按分组展示 extension 开关 */}
          <div className="space-y-1">
            {Array.from(grouped.entries()).map(([group, exts]) => {
              const groupExpanded = expandedGroups[group] ?? false;
              const enabledCount = exts.filter(
                (e) => extensions[e.key] ?? e.defaultEnabled,
              ).length;
              return (
                <Collapsible
                  key={group}
                  open={groupExpanded}
                  onOpenChange={() => toggleGroup(group)}
                >
                  <CollapsibleTrigger asChild>
                    <button
                      type="button"
                      className="flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-sm font-medium hover:bg-muted"
                    >
                      {groupExpanded ? (
                        <ChevronDown className="h-3.5 w-3.5" />
                      ) : (
                        <ChevronRight className="h-3.5 w-3.5" />
                      )}
                      {group}
                      <span className="text-xs text-muted-foreground">
                        ({enabledCount}/{exts.length})
                      </span>
                    </button>
                  </CollapsibleTrigger>
                  <CollapsibleContent>
                    <div className="space-y-0.5 pl-6 pt-0.5">
                      {exts.map((ext) => (
                        <div
                          key={ext.key}
                          className="flex items-center justify-between rounded px-2 py-1 hover:bg-muted/50"
                        >
                          <Label
                            htmlFor={`ext-${ext.key}`}
                            className="cursor-pointer text-xs"
                          >
                            {ext.label}
                          </Label>
                          <Switch
                            id={`ext-${ext.key}`}
                            checked={extensions[ext.key] ?? ext.defaultEnabled}
                            onCheckedChange={(checked) =>
                              handleExtensionToggle(ext.key, checked)
                            }
                            className="scale-75"
                          />
                        </div>
                      ))}
                    </div>
                  </CollapsibleContent>
                </Collapsible>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
}
