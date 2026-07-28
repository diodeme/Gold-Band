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
import type { FeedbackInput } from "@/types";

const MAX_DESCRIPTION_CHARS = 2000;
const MAX_SCREENSHOTS = 4;
const MAX_SCREENSHOT_BYTES = 5 * 1024 * 1024;

interface SessionOption {
  value: string;
  label: string;
  workspace: string;
  taskId: string;
}

interface FeedbackDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  sessionOptions?: SessionOption[];
}

export function FeedbackDialog({ open, onOpenChange, sessionOptions = [] }: FeedbackDialogProps) {
  const { t } = useTranslation();
  const [description, setDescription] = useState("");
  const [sessionValue, setSessionValue] = useState("none");
  const [includeLogs, setIncludeLogs] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  // Screenshots reuse the same proven attachment pipeline as the conversation
  // composer (clipboard paste on the textarea, native file picker, drag & drop,
  // and materialize-on-submit). The previous hand-rolled path listened at the
  // window level (unreliable inside a Radix portal) and fetched `asset://`
  // URLs that never resolve, so both paste and file pick were silently broken.
  const {
    attachments,
    fileError,
    fileInputRef,
    pickFiles,
    handleFilesFromInput,
    removeAttachment,
    clearAttachments,
    resolveAttachmentPaths,
    dropZoneHandlers,
    extractPasteFiles,
    previewImage,
    setPreviewImage,
    textPreview,
    setTextPreview,
    handlePreviewAttachment,
  } = useAttachmentPicker({
    maxCount: MAX_SCREENSHOTS,
    maxTotalSize: MAX_SCREENSHOTS * MAX_SCREENSHOT_BYTES,
    acceptMimePrefix: "image/",
  });
  useWindowDragGuard();

  const reset = useCallback(() => {
    setDescription("");
    setSessionValue("none");
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
      // Materialize any clipboard/browser-sourced screenshots to disk first
      // (dialog-picked files already carry a real path).
      const screenshotPaths = await resolveAttachmentPaths();
      const sessionOption = sessionValue !== "none"
        ? sessionOptions.find((o) => o.value === sessionValue) ?? null
        : null;
      const input: FeedbackInput = {
        description: trimmed,
        sessionWorkspace: sessionOption?.workspace ?? null,
        sessionTaskId: sessionOption?.taskId ?? null,
        screenshotPaths,
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
  }, [description, resolveAttachmentPaths, sessionValue, sessionOptions, includeLogs, onOpenChange, mapErrorCode]);

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
              onPaste={(e) => { void extractPasteFiles(e); }}
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
            </div>
          ) : null}

          <div className="flex flex-col gap-1.5">
            <label className="text-sm font-medium text-foreground">{t("common.feedbackScreenshots")}</label>
            <input
              ref={fileInputRef}
              type="file"
              accept="image/*"
              multiple
              className="hidden"
              onChange={handleFilesFromInput}
            />
            <div
              className="flex min-h-20 cursor-pointer flex-wrap items-center gap-2 rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground"
              onClick={() => { if (!submitting) void pickFiles(); }}
              onPaste={(e) => { void extractPasteFiles(e); }}
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
          <Button onClick={handleSubmit} disabled={submitting}>
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