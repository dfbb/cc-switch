# Task 13: 前端 UI — Claude endpoint 内容过滤面板

**可并行**: 是 — 与 Task 11, 12 并行

**依赖**: Task 03（ExtensionFilterConfig 类型定义）

## 目标

在 Claude endpoint 配置表单（`ClaudeFormFields.tsx`）中新增「内容过滤」折叠面板，包含总开关、三种预设快速切换、23 个 extension 独立开关。

## 文件

- Modify: `src/types.ts` — 添加 `ExtensionFilterConfig` 类型
- Modify: `src/components/providers/forms/ClaudeFormFields.tsx` — 新增 UI 面板
- Create: `src/components/providers/forms/ContentFilterPanel.tsx` — 面板组件

---

### Step 1: 在 types.ts 中添加前端类型

`src/types.ts`:

```typescript
export interface ExtensionFilterConfig {
  enabled?: boolean;
  extensions?: Record<string, boolean>;
  preset?: "full" | "cache-only" | "minimal" | null;
}
```

在 `ProviderMeta` 类型中添加：

```typescript
  extensionFilterConfig?: ExtensionFilterConfig;
```

### Step 2: 创建 ContentFilterPanel 组件

`src/components/providers/forms/ContentFilterPanel.tsx`:

```tsx
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import type { ExtensionFilterConfig } from "@/types";

// 23 个 extension 的元数据
const EXTENSIONS = [
  // 缓存稳定性
  { key: "fingerprint-strip", label: "指纹移除", group: "缓存稳定性" },
  { key: "sort-stabilization", label: "排序稳定化", group: "缓存稳定性" },
  { key: "fresh-session-sort", label: "新会话排序", group: "缓存稳定性" },
  { key: "cache-control-normalize", label: "缓存控制规范化", group: "缓存稳定性" },
  { key: "messages-cache-breakpoint", label: "断点注入", group: "缓存稳定性" },
  // 身份标准化
  { key: "identity-normalization", label: "身份标准化", group: "身份标准化" },
  { key: "smoosh-split", label: "粘合分离", group: "身份标准化" },
  { key: "content-strip", label: "内容剥离", group: "身份标准化" },
  { key: "tool-input-normalize", label: "工具输入标准化", group: "身份标准化" },
  // 图像/内容
  { key: "image-strip", label: "图片剥离", group: "图像处理" },
  { key: "thinking-display", label: "Thinking 显示", group: "图像处理" },
  // TTL
  { key: "ttl-tier-detect", label: "TTL 检测", group: "TTL 管理" },
  { key: "ttl-management", label: "TTL 注入", group: "TTL 管理" },
  // 会话
  { key: "deferred-tools-restore", label: "延迟工具恢复", group: "会话持久化" },
  // 监控
  { key: "cache-telemetry", label: "缓存遥测", group: "监控" },
  { key: "overage-warning", label: "超额警告", group: "监控" },
  // 诊断（默认禁用）
  { key: "upstream-change-detection", label: "上游变更检测", group: "诊断" },
  { key: "output-efficiency-rewrite", label: "输出效率重写", group: "诊断" },
  { key: "microcompact-stability", label: "微压缩稳定性", group: "诊断" },
  { key: "rate-limit-log", label: "限流日志", group: "诊断" },
  { key: "usage-log", label: "用量日志", group: "诊断" },
  { key: "request-log", label: "请求日志", group: "诊断" },
  { key: "prefix-diff", label: "前缀差异", group: "诊断" },
];

const PRESETS = [
  { key: "full" as const, label: "完整模式", desc: "23 个全开" },
  { key: "cache-only" as const, label: "仅缓存修复", desc: "15 个核心" },
  { key: "minimal" as const, label: "最小模式", desc: "fingerprint + identity" },
];

interface Props {
  value: ExtensionFilterConfig | undefined;
  onChange: (v: ExtensionFilterConfig) => void;
}

export function ContentFilterPanel({ value, onChange }: Props) {
  const { t } = useTranslation();
  const config = value ?? {};

  const totalEnabled = config.enabled ?? false;

  return (
    <Collapsible className="mt-4 border rounded-lg p-3">
      <CollapsibleTrigger className="flex items-center gap-2 w-full">
        <span className="text-sm font-medium">内容过滤扩展</span>
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-3 pt-3">
        {/* 总开关 */}
        <div className="flex items-center justify-between">
          <Label>启用内容过滤</Label>
          <Switch
            checked={totalEnabled}
            onCheckedChange={(enabled) =>
              onChange({ ...config, enabled, preset: null })
            }
          />
        </div>

        {totalEnabled && (
          <>
            {/* 预设快速切换 */}
            <div className="flex gap-1 flex-wrap">
              {PRESETS.map(({ key, label, desc }) => (
                <button
                  key={key}
                  type="button"
                  className={`px-2 py-1 text-xs rounded border ${
                    config.preset === key
                      ? "bg-blue-100 border-blue-400"
                      : "bg-gray-50"
                  }`}
                  onClick={() => {
                    const extensions = EXTENSIONS.reduce(
                      (acc, ext) => {
                        if (key === "full") acc[ext.key] = true;
                        else if (key === "cache-only")
                          acc[ext.key] = [
                            "fingerprint-strip", "sort-stabilization",
                            "fresh-session-sort", "identity-normalization",
                            "smoosh-split", "content-strip",
                            "tool-input-normalize", "image-strip",
                            "thinking-display", "ttl-tier-detect",
                            "ttl-management", "deferred-tools-restore",
                            "cache-control-normalize",
                            "messages-cache-breakpoint", "cache-telemetry",
                          ].includes(ext.key);
                        else if (key === "minimal")
                          acc[ext.key] = [
                            "fingerprint-strip", "identity-normalization",
                          ].includes(ext.key);
                        return acc;
                      },
                      {} as Record<string, boolean>,
                    );
                    onChange({ enabled: true, extensions, preset: key });
                  }}
                >
                  {label}
                </button>
              ))}
            </div>

            {/* 每个 extension 独立开关 */}
            <div className="space-y-1 max-h-64 overflow-y-auto">
              {EXTENSIONS.map((ext) => (
                <div
                  key={ext.key}
                  className="flex items-center justify-between py-1"
                >
                  <span className="text-xs text-gray-600">{ext.label}</span>
                  <Switch
                    checked={
                      config.extensions?.[ext.key] ?? true
                    }
                    onCheckedChange={(enabled) =>
                      onChange({
                        ...config,
                        extensions: {
                          ...config.extensions,
                          [ext.key]: enabled,
                        },
                        preset: null,
                      })
                    }
                  />
                </div>
              ))}
            </div>
          </>
        )}
      </CollapsibleContent>
    </Collapsible>
  );
}
```

### Step 3: 在 ClaudeFormFields 中集成

在 `ClaudeFormFields.tsx` 的高级选项折叠面板末尾添加：

```tsx
import { ContentFilterPanel } from "./ContentFilterPanel";

// 在表单中：
<ContentFilterPanel
  value={form.getValues("meta.extensionFilterConfig")}
  onChange={(v) => form.setValue("meta.extensionFilterConfig", v)}
/>
```

### Step 4: 在 ProviderForm submit 中序列化

确保 `extensionFilterConfig` 字段在 `performSubmit` 函数中被正确序列化到 `meta` 中（默认行为——已作为 ProviderMeta 的一部分）。

### Step 5: 提交

```bash
git add src/types.ts src/components/providers/forms/
git commit -m "feat(extensions): add ContentFilterPanel UI with presets and per-extension toggles"
```
