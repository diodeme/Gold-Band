import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CircleCheck, UploadCloud, X } from "lucide-react";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { getRuntimeApi } from "@/api/client";
import type { FeedbackInput, SessionRef } from "@/types";

const MAX_DESCRIPTION_CHARS = 2000;
const MAX_SCREENSHOTS = 4;
const MAX_SCREENSHOT_BYTES = 5 * 1024 * 1024;

interface ScreenshotItem {
  name: string;
  size: number;
  dataBase64: string;
  previewUrl: string;
}

interface SessionOption {
  value: string;
  label: string;
  sessionRef: SessionRef;
}

interface FeedbackDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  sessionOptions?: SessionOption[];
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("file read failed"));
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("file read failed"));
        return;
      }
      resolve(result.split(",", 2)[1] ?? result);
    };
    reader.readAsDataURL(file);
  });
}

export function FeedbackDialog({ open, onOpenChange, sessionOptions = [] }: FeedbackDialogProps) {
  const { t } = useTranslation();
  const [description, setDescription] = useState("");
  const [sessionValue, setSessionValue] = useState("none");
  const [screenshots, setScreenshots] = useState<ScreenshotItem[]>([]);
  const [includeLogs, setIncludeLogs] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const pasteZoneRef = useRef<HTMLDivElement>(null);

  const reset = useCallback(() => {
    setDescription("");
    setSessionValue("none");
    setScreenshots([]);
    setIncludeLogs(true);
    setErrorKey(null);
    setDone(false);
    setSubmitting(false);
  }, []);

  useEffect(() => {
    if (open) reset();
  }, [open, reset]);

  const addFiles = useCallback(async (files: File[]) => {
    const images = files.filter((f) => f.type.startsWith("image/"));
    setScreenshots((prev) => {
      const remaining = MAX_SCREENSHOTS - prev.length;
      if (remaining <= 0) return prev;
      return [...prev, ...images.slice(0, remaining).map((f) => ({
        name: f.name,
        size: f.size,
        dataBase64: "",
        previewUrl: URL.createObjectURL(f),
        file: f,
      } as ScreenshotItem & { file: File }))];
    });
  }, []);

  useEffect(() => {
    if (!open) return;
    const zone = pasteZoneRef.current;
    if (!zone) return;
    const onPaste = (event: ClipboardEvent) => {
      const items = event.clipboardData?.items;
      if (!items) return;
      const files: File[] = [];
      for (const item of items) {
        if (item.kind === "file") {
          const f = item.getAsFile();
          if (f) files.push(f);
        }
      }
      if (files.length) {
        event.preventDefault();
        void addFiles(files);
      }
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, [open, addFiles]);

  const removeScreenshot = useCallback((index: number) => {
    setScreenshots((prev) => {
      const next = [...prev];
      const [removed] = next.splice(index, 1);
      if (removed) URL.revokeObjectURL(removed.previewUrl);
      return next;
    });
  }, []);

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
    if (!trimmed) {
      setErrorKey("common.feedbackErrorValidation");
      return;
    }
    if (trimmed.length > MAX_DESCRIPTION_CHARS) {
      setErrorKey("common.feedbackErrorValidation");
      return;
    }
    const oversized = screenshots.find((s) => s.size > MAX_SCREENSHOT_BYTES);
    if (oversized) {
      setErrorKey("common.feedbackErrorValidation");
      return;
    }

    setSubmitting(true);
    try {
      const materialized = screenshots.length > 0
        ? await getRuntimeApi().materializeConversationAttachments(
            await Promise.all(screenshots.map(async (s) => {
              const raw = (s as ScreenshotItem & { file?: File }).file;
              const dataBase64 = raw ? await fileToBase64(raw) : s.dataBase64;
              return { name: s.name, mime: null, size: s.size, dataBase64 };
            })),
          )
        : [];
      const sessionRef = sessionValue !== "none"
        ? sessionOptions.find((o) => o.value === sessionValue)?.sessionRef ?? null
        : null;
      const input: FeedbackInput = {
        description: trimmed,
        sessionRef,
        screenshotPaths: materialized.map((m) => m.path),
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
  }, [description, screenshots, sessionValue, sessionOptions, includeLogs, onOpenChange, mapErrorCode]);

  const onPickFiles = useCallback(async () => {
    const files = await getRuntimeApi().pickAttachmentFiles();
    if (!files.length) return;
    const fetched: File[] = [];
    for (const ref of files) {
      try {
        const res = await fetch(`asset://localhost/${encodeURIComponent(ref.path)}`);
        const blob = await res.blob();
        fetched.push(new File([blob], ref.name, { type: blob.type || "image/png" }));
      } catch {
        // best-effort
      }
    }
    void addFiles(fetched);
  }, [addFiles]);

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

        <div className="flex flex-col gap-4">
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
            </div>
          ) : null}

          <div className="flex flex-col gap-1.5">
            <label className="text-sm font-medium text-foreground">{t("common.feedbackScreenshots")}</label>
            <div
              ref={pasteZoneRef}
              className="flex min-h-20 cursor-pointer flex-wrap items-center gap-2 rounded-md border border-dashed border-border p-3 text-xs text-muted-foreground"
              onClick={onPickFiles}
            >
              <UploadCloud className="size-4 shrink-0" />
              <span>{t("common.feedbackScreenshotHint")}</span>
            </div>
            {screenshots.length > 0 ? (
              <div className="flex flex-wrap gap-2">
                {screenshots.map((s, i) => (
                  <div key={i} className="relative size-16 overflow-hidden rounded-md border border-border">
                    <img src={s.previewUrl} alt={s.name} className="size-full object-cover" />
                    <button
                      type="button"
                      className="absolute right-0.5 top-0.5 grid size-5 place-items-center rounded-full bg-background/80 text-foreground hover:bg-background"
                      onClick={(e) => { e.stopPropagation(); removeScreenshot(i); }}
                      disabled={submitting}
                    >
                      <X className="size-3" />
                    </button>
                  </div>
                ))}
              </div>
            ) : null}
          </div>

          <label className="flex items-center gap-2 text-sm text-foreground">
            <Switch checked={includeLogs} onCheckedChange={setIncludeLogs} disabled={submitting} />
            {t("common.feedbackIncludeLogs")}
          </label>

          <p className="rounded-md bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
            {t("common.feedbackPrivacyNotice")}
          </p>

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
    </Dialog>
  );
}