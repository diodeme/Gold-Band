import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, UploadCloud } from "lucide-react";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useAttachmentPicker, useWindowDragGuard } from "@/lib/attachment-service";
import { AttachmentChipsList, AttachmentPreviewDialogs } from "@/components/shared/AttachmentComponents";
import { getRuntimeApi } from "@/api/client";
import type { FeedbackInput, FeedbackArchivePreview } from "@/types";

const MAX_DESCRIPTION_CHARS = 2000;
const MAX_SCREENSHOTS = 4;
const MAX_SCREENSHOT_BYTES = 5 * 1024 * 1024;
const FEEDBACK_IMAGE_MIMES = ["image/png", "image/jpeg", "image/webp"];

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface SessionOption {
  value: string;
  label: string;
  projectId: string;
  taskId: string;
}

interface FeedbackDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function FeedbackDialog({ open, onOpenChange }: FeedbackDialogProps) {
  const { t } = useTranslation();
  const [description, setDescription] = useState("");
  const [sessionValue, setSessionValue] = useState("none");
  const [includeLogs, setIncludeLogs] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const [sessionOptions, setSessionOptions] = useState<SessionOption[]>([]);
  const [archivePreview, setArchivePreview] = useState<FeedbackArchivePreview | null>(null);

  // Load the cross-workspace conversation list when the dialog opens so the
  // user can optionally associate a session. Stays empty if there are none.
  useEffect(() => {
    if (!open) return;
    let active = true;
    void getRuntimeApi().getConversationSidebar().then((sidebar) => {
      if (!active) return;
      const workspaceByProject = new Map(
        sidebar.workspaces.map((w) => [w.projectId, w] as const),
      );
      const multiWorkspace = sidebar.workspaces.length > 1;
      const options: SessionOption[] = [];
      for (const [projectId, tasks] of Object.entries(sidebar.tasksByWorkspace)) {
        const ws = workspaceByProject.get(projectId);
        if (!ws) continue;
        for (const task of tasks) {
          const label = multiWorkspace && ws.name
            ? `${ws.name} / ${task.title || task.taskId}`
            : (task.title || task.taskId);
          options.push({
            value: `${projectId}::${task.taskId}`,
            label,
            projectId,
            taskId: task.taskId,
          });
        }
      }
      setSessionOptions(options);
    }).catch(() => {
      // Sidebar load is best-effort; absence just hides the dropdown.
      if (active) setSessionOptions([]);
    });
    return () => { active = false; };
  }, [open]);


  // Screenshots reuse the same proven attachment pipeline as the conversation
  // composer (clipboard paste on the textarea, native file picker, drag & drop,
  // and materialize-on-submit). The previous hand-rolled path listened at the
  // window level (unreliable inside a Radix portal) and fetched `asset://`
  // URLs that never resolve, so both paste and file pick were silently broken.
  const {
    attachments,
    fileError,
    fileInputRef,
    addFiles,
    handleFilesFromInput,
    removeAttachment,
    clearAttachments,
    resolveAttachmentInputs,
    dropZoneHandlers,
    previewImage,
    setPreviewImage,
    textPreview,
    setTextPreview,
    handlePreviewAttachment,
  } = useAttachmentPicker({
    maxCount: MAX_SCREENSHOTS,
    maxTotalSize: MAX_SCREENSHOTS * MAX_SCREENSHOT_BYTES,
    maxFileSize: MAX_SCREENSHOT_BYTES,
    acceptedMimes: FEEDBACK_IMAGE_MIMES,
  });

  // When the user selects a session, preview the archive size so they know how
  // much will be uploaded before committing. This is an informed-consent affordance.
  useEffect(() => {
    if (sessionValue === "none" || !open) {
      setArchivePreview(null);
      return;
    }
    const option = sessionOptions.find((o) => o.value === sessionValue);
    if (!option) {
      setArchivePreview(null);
      return;
    }
    let active = true;
    void getRuntimeApi().previewFeedbackSessionArchive(option.projectId, option.taskId).then((preview) => {
      if (active) setArchivePreview(preview);
    }).catch(() => {
      if (active) setArchivePreview(null);
    });
    return () => { active = false; };
  }, [sessionValue, open, sessionOptions]);
  // Global paste for screenshots: Ctrl+V works anywhere while the dialog is
  // open, regardless of focus. This replaces the previous per-region onPaste
  // which conflicted with the click-to-pick-files affordance on the same area.
  useEffect(() => {
    if (!open) return;
    const onPaste = (event: ClipboardEvent) => {
      const items = event.clipboardData?.items;
      if (!items) return;
      const files: File[] = [];
      for (let i = 0; i < items.length; i++) {
        if (items[i].kind === "file") {
          const file = items[i].getAsFile();
          if (file) files.push(file);
        }
      }
      if (files.length > 0) {
        event.preventDefault();
        void addFiles(files);
      }
    };
    document.addEventListener("paste", onPaste);
    return () => document.removeEventListener("paste", onPaste);
  }, [open, addFiles]);
  useWindowDragGuard();

  const reset = useCallback(() => {
    setDescription("");
    setSessionValue("none");
    setArchivePreview(null);
    setIncludeLogs(true);
    setErrorKey(null);
    setDone(false);
    setSubmitting(false);
    clearAttachments();
  }, [clearAttachments]);

  useEffect(() => {
    if (open) reset();
  }, [open, reset]);

  const mapErrorCode = useCallback((code: string): string => {
    if (code === "feedback.network-failed") return "common.feedbackErrorNetwork";
    if (code === "feedback.server-error") return "common.feedbackErrorServer";
    if (code === "feedback.validation-failed") return "common.feedbackErrorValidation";
    if (code === "feedback.endpoint-unconfigured") return "common.feedbackErrorUnconfigured";
    if (code === "feedback.disabled") return "common.feedbackErrorUnconfigured";
    if (code === "feedback.session-not-found") return "common.feedbackErrorSession";
    if (code === "feedback.attachment-invalid") return "common.feedbackErrorAttachment";
    if (code === "feedback.payload-too-large") return "common.feedbackErrorTooLarge";
    return "common.feedbackErrorServer";
  }, []);

  const handleSubmit = useCallback(async () => {
    setErrorKey(null);
    const trimmed = description.trim();
    if (!trimmed || trimmed.length > MAX_DESCRIPTION_CHARS) {
      setErrorKey("common.feedbackErrorValidation");
      return;
    }

    setSubmitting(true);
    try {
      const screenshots = await resolveAttachmentInputs();
      const sessionOption = sessionValue !== "none"
        ? sessionOptions.find((o) => o.value === sessionValue) ?? null
        : null;
      const input: FeedbackInput = {
        description: trimmed,
        projectId: sessionOption?.projectId ?? null,
        taskId: sessionOption?.taskId ?? null,
        screenshots: screenshots.map((screenshot) => ({
          ...screenshot,
          mime: screenshot.mime ?? "application/octet-stream",
        })),
        includeLogs,
      };
      await getRuntimeApi().submitFeedback(input);
      setDone(true);
      setTimeout(() => onOpenChange(false), 1500);
    } catch (err) {
      const code = (err as { code?: string })?.code ?? "feedback.server-error";
      setErrorKey(mapErrorCode(code));
    } finally {
      setSubmitting(false);
    }
  }, [description, resolveAttachmentInputs, sessionValue, sessionOptions, includeLogs, onOpenChange, mapErrorCode]);

  if (done) {
    return (
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-md">
          <DialogTitle className="sr-only">{t("common.feedbackTitle")}</DialogTitle>
          <div className="flex flex-col items-center gap-3 py-8 text-center">
            <CircleCheck className="size-10 text-emerald-500" />
            <div className="text-base font-medium text-foreground">{t("common.feedbackSubmitted")}</div>
          </div>
        </DialogContent>
      </Dialog>
    );
  }

  return (
    <Dialog open={open} onOpenChange={(v) => { if (!submitting) onOpenChange(v); }}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("common.feedbackTitle")}</DialogTitle>
          <DialogDescription>{t("common.feedbackSubtitle")}</DialogDescription>
        </DialogHeader>

        <div
          data-attachment-dropzone="true"
          className="flex flex-col gap-4"
          {...dropZoneHandlers}
        >
          <div className="flex flex-col gap-1.5">
            <label className="text-sm font-medium text-foreground">
              {t("common.feedbackDescription")} <span className="text-destructive">*</span>
            </label>
            <Textarea
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder={t("common.feedbackDescriptionPlaceholder")}
              maxLength={MAX_DESCRIPTION_CHARS}
              rows={4}
              disabled={submitting}
            />
            <span className="text-xs text-muted-foreground">{description.length}/{MAX_DESCRIPTION_CHARS}</span>
          </div>

          {sessionOptions.length > 0 ? (
            <div className="flex flex-col gap-1.5">
              <label className="text-sm font-medium text-foreground">{t("common.feedbackRelatedSession")}</label>
              <Select value={sessionValue} onValueChange={setSessionValue} disabled={submitting}>
                <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">{t("common.feedbackNoSession")}</SelectItem>
                  {sessionOptions.map((o) => (
                    <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {archivePreview ? (
                <span className={archivePreview.withinLimits ? "text-xs text-muted-foreground" : "text-xs text-destructive"}>
                  {archivePreview.withinLimits
                    ? t("common.feedbackArchiveHint", {
                        size: formatBytes(archivePreview.uncompressedBytes),
                        count: archivePreview.fileCount,
                      })
                    : t("common.feedbackArchiveTooLarge", {
                        size: formatBytes(archivePreview.uncompressedBytes),
                        max: formatBytes(archivePreview.maxUncompressedBytes),
                      })}
                </span>
              ) : null}
            </div>
          ) : null}

          <div className="flex flex-col gap-1.5">
            <label className="text-sm font-medium text-foreground">{t("common.feedbackScreenshots")}</label>
            <input
              ref={fileInputRef}
              type="file"
              accept={FEEDBACK_IMAGE_MIMES.join(",")}
              multiple
              className="hidden"
              onChange={handleFilesFromInput}
            />
            <div
              className="flex min-h-20 cursor-pointer flex-wrap items-center gap-2 rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground"
              onClick={() => { if (!submitting) fileInputRef.current?.click(); }}
            >
              <UploadCloud className="size-4 shrink-0" />
              <span>{t("common.feedbackScreenshotHint")}</span>
            </div>
            <AttachmentChipsList
              attachments={attachments}
              compact
              onRemove={removeAttachment}
              onPreview={handlePreviewAttachment}
              onClear={clearAttachments}
              clearLabel={t("common.clear")}
            />
          </div>

          <label className="flex items-center gap-2 text-sm text-foreground">
            <Switch checked={includeLogs} onCheckedChange={setIncludeLogs} disabled={submitting} />
            {t("common.feedbackIncludeLogs")}
          </label>

          <p className="rounded-md bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
            {t("common.feedbackPrivacyNotice")}
          </p>

          {fileError ? (
            <p className="text-sm text-destructive">{fileError}</p>
          ) : null}
          {errorKey ? (
            <p className="text-sm text-destructive">{t(errorKey)}</p>
          ) : null}
        </div>

        <DialogFooter>
          <Button variant="ghost" onClick={() => onOpenChange(false)} disabled={submitting}>
            {t("common.feedbackCancel")}
          </Button>
          <Button onClick={handleSubmit} disabled={submitting || archivePreview?.withinLimits === false}>
            {submitting ? t("common.feedbackSubmitting") : t("common.feedbackSubmit")}
          </Button>
        </DialogFooter>
      </DialogContent>

      <AttachmentPreviewDialogs
        previewImage={previewImage}
        textPreview={textPreview}
        onCloseImage={() => setPreviewImage(null)}
        onCloseText={() => setTextPreview(null)}
      />
    </Dialog>
  );
}
