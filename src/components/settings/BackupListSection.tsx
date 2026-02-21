import { useState } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { HardDriveDownload, RotateCcw } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useBackupManager } from "@/hooks/useBackupManager";
import { extractErrorMessage } from "@/utils/errorUtils";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatBackupDate(isoString: string): string {
  try {
    const date = new Date(isoString);
    return date.toLocaleString();
  } catch {
    return isoString;
  }
}

export function BackupListSection() {
  const { t } = useTranslation();
  const { backups, isLoading, restore, isRestoring } = useBackupManager();
  const [confirmFilename, setConfirmFilename] = useState<string | null>(null);

  const handleRestore = async () => {
    if (!confirmFilename) return;
    try {
      const safetyId = await restore(confirmFilename);
      setConfirmFilename(null);
      toast.success(
        t("settings.backupManager.restoreSuccess", {
          defaultValue: "Restore successful! Safety backup created",
        }),
        {
          description: safetyId
            ? `${t("settings.backupManager.safetyBackupId", { defaultValue: "Safety Backup ID" })}: ${safetyId}`
            : undefined,
          duration: 6000,
          closeButton: true,
        },
      );
    } catch (error) {
      const detail =
        extractErrorMessage(error) ||
        t("settings.backupManager.restoreFailed", {
          defaultValue: "Restore failed",
        });
      toast.error(detail);
    }
  };

  return (
    <div className="mt-4 pt-4 border-t border-border/50">
      <div className="flex items-center gap-2 mb-3">
        <HardDriveDownload className="h-4 w-4 text-muted-foreground" />
        <h4 className="text-sm font-medium">
          {t("settings.backupManager.title", {
            defaultValue: "Database Backups",
          })}
        </h4>
      </div>
      <p className="text-xs text-muted-foreground mb-3">
        {t("settings.backupManager.description", {
          defaultValue:
            "Automatic database snapshots for restoring to a previous state",
        })}
      </p>

      {isLoading ? (
        <div className="text-sm text-muted-foreground py-2">Loading...</div>
      ) : backups.length === 0 ? (
        <div className="text-sm text-muted-foreground py-2">
          {t("settings.backupManager.empty", {
            defaultValue: "No backups yet",
          })}
        </div>
      ) : (
        <div className="space-y-1.5 max-h-48 overflow-y-auto">
          {backups.map((backup) => (
            <div
              key={backup.filename}
              className="flex items-center justify-between gap-2 px-3 py-2 rounded-lg bg-muted/30 hover:bg-muted/50 transition-colors text-sm"
            >
              <div className="flex-1 min-w-0">
                <div className="font-mono text-xs truncate">
                  {formatBackupDate(backup.createdAt)}
                </div>
                <div className="text-xs text-muted-foreground">
                  {formatBytes(backup.sizeBytes)}
                </div>
              </div>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 px-2 text-xs shrink-0"
                disabled={isRestoring}
                onClick={() => setConfirmFilename(backup.filename)}
              >
                <RotateCcw className="h-3 w-3 mr-1" />
                {isRestoring
                  ? t("settings.backupManager.restoring", {
                      defaultValue: "Restoring...",
                    })
                  : t("settings.backupManager.restore", {
                      defaultValue: "Restore",
                    })}
              </Button>
            </div>
          ))}
        </div>
      )}

      {/* Confirmation Dialog */}
      <Dialog
        open={!!confirmFilename}
        onOpenChange={(open) => !open && setConfirmFilename(null)}
      >
        <DialogContent className="max-w-md" zIndex="alert">
          <DialogHeader>
            <DialogTitle>
              {t("settings.backupManager.confirmTitle", {
                defaultValue: "Confirm Restore",
              })}
            </DialogTitle>
            <DialogDescription>
              {t("settings.backupManager.confirmMessage", {
                defaultValue:
                  "Restoring this backup will overwrite the current database. A safety backup will be created first.",
              })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={() => setConfirmFilename(null)}
              disabled={isRestoring}
            >
              {t("common.cancel", { defaultValue: "Cancel" })}
            </Button>
            <Button onClick={handleRestore} disabled={isRestoring}>
              {isRestoring
                ? t("settings.backupManager.restoring", {
                    defaultValue: "Restoring...",
                  })
                : t("settings.backupManager.restore", {
                    defaultValue: "Restore",
                  })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
