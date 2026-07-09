import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { isAllowedAttachment, isImageMime, useAttachmentExtensions } from './attachments';
import { materializeConversationAttachments, pickAttachmentFiles } from '@/api';
import { isTauriRuntime } from '@/api/shared';

// ── Types ──

export interface AttachmentItem {
  id: string;
  name: string;
  size: number;
  mime: string;
  /** Real filesystem path (Tauri dialog/drag-drop). Only present on desktop. */
  path?: string;
  /** Object URL for browser-mode image preview. Call URL.revokeObjectURL when done. */
  previewUrl?: string;
  /** Raw File object for browser-mode content reading. */
  file?: File;
  source: 'dialog' | 'drag-drop' | 'paste' | 'browser-file';
}

// ── Constants ──

export const MAX_ATTACHMENT_COUNT = 10;
export const MAX_ATTACHMENT_TOTAL = 50 * 1024 * 1024; // 50 MB

// ── Helpers ──

export function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const size = bytes / 1024 ** i;
  return `${size < 10 ? size.toFixed(1) : Math.round(size)} ${units[i]}`;
}

export function hasFileTransfer(dataTransfer: DataTransfer | null): boolean {
  if (!dataTransfer) return false;
  return (
    Array.from(dataTransfer.types ?? []).includes('Files') ||
    Array.from(dataTransfer.items ?? []).some((item) => item.kind === 'file') ||
    dataTransfer.files.length > 0
  );
}

export function extractTransferFiles(dataTransfer: DataTransfer | null): File[] {
  if (!dataTransfer) return [];
  const itemFiles = Array.from(dataTransfer.items ?? [])
    .filter((item) => item.kind === 'file')
    .map((item) => item.getAsFile())
    .filter((file): file is File => !!file);
  if (itemFiles.length > 0) return itemFiles;
  return Array.from(dataTransfer.files ?? []);
}

export function isAttachmentDropTarget(target: EventTarget | null): boolean {
  return target instanceof Element && !!target.closest('[data-attachment-dropzone="true"]');
}

function guessMimeFromExtension(name: string): string {
  const ext = name.slice(name.lastIndexOf('.') + 1).toLowerCase();
  const mimeMap: Record<string, string> = {
    png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg',
    gif: 'image/gif', webp: 'image/webp', bmp: 'image/bmp', ico: 'image/x-icon',
    svg: 'image/svg+xml',
    txt: 'text/plain', md: 'text/markdown', log: 'text/plain',
    json: 'application/json', jsonl: 'application/jsonl',
    csv: 'text/csv', xml: 'application/xml',
    html: 'text/html', css: 'text/css', htm: 'text/html',
    js: 'text/javascript', ts: 'text/typescript',
    jsx: 'text/javascript', tsx: 'text/typescript',
    yaml: 'text/yaml', yml: 'text/yaml',
    pdf: 'application/pdf',
    zip: 'application/zip', tar: 'application/x-tar', gz: 'application/gzip',
    py: 'text/x-python', rs: 'text/x-rust', go: 'text/x-go',
    java: 'text/x-java', rb: 'text/x-ruby',
    c: 'text/x-c', cpp: 'text/x-c++', h: 'text/x-c', hpp: 'text/x-c++',
    sql: 'text/x-sql', sh: 'text/x-shellscript',
    ini: 'text/plain', cfg: 'text/plain', env: 'text/plain',
    bat: 'text/plain', ps1: 'text/plain',
    toml: 'text/plain',
  };
  return mimeMap[ext] || 'application/octet-stream';
}

function generateId(): string {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

function filesToItems(files: File[], source: AttachmentItem['source']): AttachmentItem[] {
  return files.map((file) => ({
    ...fileToItem(file, source),
  }));
}

function fileToItem(file: File, source: AttachmentItem['source']): AttachmentItem {
  const mime = file.type || guessMimeFromExtension(file.name);
  return {
    id: generateId(),
    name: file.name,
    size: file.size,
    mime,
    path: isTauriRuntime() ? ((file as any).path as string | undefined) : undefined,
    previewUrl: isImageMime(mime) ? URL.createObjectURL(file) : undefined,
    file,
    source,
  };
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error('attachment read failed'));
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== 'string') {
        reject(new Error('attachment read failed'));
        return;
      }
      resolve(result.split(',', 2)[1] ?? result);
    };
    reader.readAsDataURL(file);
  });
}

// ── Hook ──

export interface UseAttachmentPickerOptions {
  maxCount?: number;
  maxTotalSize?: number;
  attachments?: AttachmentStateController;
}

export type AttachmentStateController = [
  AttachmentItem[],
  (next: AttachmentItem[] | ((prev: AttachmentItem[]) => AttachmentItem[])) => void,
];

export function revokeAttachmentPreviewUrls(attachments: AttachmentItem[]): void {
  for (const attachment of attachments) {
    if (attachment.previewUrl) URL.revokeObjectURL(attachment.previewUrl);
  }
}

export function useAttachmentPicker(options: UseAttachmentPickerOptions = {}) {
  const { t } = useTranslation();
  const allowedExts = useAttachmentExtensions();
  const [internalAttachments, setInternalAttachments] = useState<AttachmentItem[]>([]);
  const externalAttachments = options.attachments;
  const attachments = externalAttachments?.[0] ?? internalAttachments;
  const setAttachments = externalAttachments?.[1] ?? setInternalAttachments;
  const ownsAttachmentState = !externalAttachments;
  const [fileError, setFileError] = useState<string | null>(null);
  const [previewImage, setPreviewImage] = useState<AttachmentItem | null>(null);
  const [textPreview, setTextPreview] = useState<{ name: string; content: string } | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const latestAttachmentsRef = useRef<AttachmentItem[]>(attachments);
  const dropCounterRef = useRef(0);

  const maxCount = options.maxCount ?? MAX_ATTACHMENT_COUNT;
  const maxTotalSize = options.maxTotalSize ?? MAX_ATTACHMENT_TOTAL;

  // ── Internal: validate & add items ──
  const validateAndAdd = useCallback(
    (items: AttachmentItem[]) => {
      const rejected: string[] = [];
      let err: string | null = null;

      const validItems = items.filter((item) => {
        if (allowedExts && !isAllowedAttachment(item.name, allowedExts)) {
          rejected.push(item.name);
          return false;
        }
        return true;
      });

      if (rejected.length > 0) {
        err = t('conversation.attachmentUnsupportedFile', { names: rejected.join(', ') });
      }

      setAttachments((prev) => {
        const next = [...prev, ...validItems];

        if (!err && next.length > maxCount) {
          err = t('conversation.attachmentCountExceeded', { max: maxCount });
          // Truncate to max
          const dropped = next.slice(maxCount);
          revokeAttachmentPreviewUrls(dropped);
          return next.slice(0, maxCount);
        }

        if (!err) {
          const totalSize = next.reduce((s, a) => s + a.size, 0);
          if (totalSize > maxTotalSize) {
            err = t('conversation.attachmentTotalTooLarge');
          }
        }

        return next;
      });

      if (err) {
        setFileError(err);
        setTimeout(() => setFileError(null), 4000);
      }
      if (fileInputRef.current) fileInputRef.current.value = '';
    },
    [t, allowedExts, maxCount, maxTotalSize],
  );

  // ── File picker (Tauri dialog on desktop, file input otherwise) ──
  const pickFiles = useCallback(async () => {
    if (isTauriRuntime()) {
      try {
        const files = await pickAttachmentFiles();
        if (files.length === 0) return;
        const items: AttachmentItem[] = files.map((f) => ({
          id: generateId(),
          name: f.name,
          size: f.size,
          mime: guessMimeFromExtension(f.name),
          path: f.path,
          source: 'dialog' as const,
        }));
        validateAndAdd(items);
        return;
      } catch {
        // fallback to file input
      }
    }
    fileInputRef.current?.click();
  }, [validateAndAdd]);

  // ── File input handler (browser fallback) ──
  const handleFilesFromInput = useCallback(() => {
    const input = fileInputRef.current;
    if (!input?.files?.length) return;
    validateAndAdd(filesToItems(Array.from(input.files), 'browser-file'));
  }, [validateAndAdd]);

  // ── Drag & drop ──
  const handleDragEnter = useCallback((e: React.DragEvent) => {
    if (!hasFileTransfer(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    dropCounterRef.current += 1;
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    if (!hasFileTransfer(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    dropCounterRef.current -= 1;
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    if (!hasFileTransfer(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'copy';
  }, []);

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      if (!hasFileTransfer(e.dataTransfer)) return;
      e.preventDefault();
      e.stopPropagation();
      dropCounterRef.current = 0;
      const files = extractTransferFiles(e.dataTransfer);
      if (files.length > 0) validateAndAdd(filesToItems(files, 'drag-drop'));
    },
    [validateAndAdd],
  );

  const dropZoneHandlers = {
    onDragEnter: handleDragEnter,
    onDragLeave: handleDragLeave,
    onDragOver: handleDragOver,
    onDrop: handleDrop,
  };

  // ── Clipboard paste ──
  const extractPasteFiles = useCallback(
    (e: React.ClipboardEvent): boolean => {
      const items = e.clipboardData?.items;
      if (!items) return false;
      const files: File[] = [];
      for (let i = 0; i < items.length; i++) {
        if (items[i].kind === 'file') {
          const file = items[i].getAsFile();
          if (file) files.push(file);
        }
      }
      if (files.length > 0) {
        e.preventDefault();
        validateAndAdd(filesToItems(files, 'paste'));
        return true;
      }
      return false;
    },
    [validateAndAdd],
  );

  // ── Remove / Clear ──
  const removeAttachment = useCallback((id: string) => {
    setAttachments((prev) => {
      const removed = prev.find((a) => a.id === id);
      if (removed) revokeAttachmentPreviewUrls([removed]);
      return prev.filter((a) => a.id !== id);
    });
  }, []);

  const clearAttachments = useCallback(() => {
    setAttachments((prev) => {
      revokeAttachmentPreviewUrls(prev);
      return [];
    });
  }, []);

  useEffect(() => {
    latestAttachmentsRef.current = attachments;
  }, [attachments]);

  // Only the component that owns attachment state may release remaining preview URLs.
  // External controllers, such as the conversation composer draft, survive this hook.
  useEffect(() => {
    if (!ownsAttachmentState) return undefined;
    return () => {
      revokeAttachmentPreviewUrls(latestAttachmentsRef.current);
    };
  }, [ownsAttachmentState]);

  const showTransientFileError = useCallback((message: string) => {
    setFileError(message);
    setTimeout(() => setFileError(null), 4000);
  }, []);

  // ── Resolve paths for sending ──
  const getAttachmentPaths = useCallback((): string[] => {
    return attachments.filter((a) => !!a.path).map((a) => a.path!);
  }, [attachments]);

  const resolveAttachmentPaths = useCallback(async (): Promise<string[]> => {
    const pendingFiles = attachments.filter((a) => !a.path && !!a.file);
    const unresolved = attachments.filter((a) => !a.path && !a.file);
    if (unresolved.length > 0) {
      const message = t('conversation.attachmentMaterializeFailed');
      showTransientFileError(message);
      throw new Error(message);
    }
    if (pendingFiles.length === 0) {
      return attachments.filter((a) => !!a.path).map((a) => a.path!);
    }
    try {
      const files = await Promise.all(
        pendingFiles.map(async (item) => ({
          name: item.name,
          mime: item.mime,
          size: item.size,
          dataBase64: await fileToBase64(item.file!),
        })),
      );
      const materialized = await materializeConversationAttachments(files);
      let materializedIndex = 0;
      return attachments.flatMap((item) => {
        if (item.path) return [item.path];
        if (!item.file) return [];
        const path = materialized[materializedIndex]?.path;
        materializedIndex += 1;
        return path ? [path] : [];
      });
    } catch (error) {
      const message = t('conversation.attachmentMaterializeFailed');
      showTransientFileError(message);
      throw error;
    }
  }, [attachments, showTransientFileError, t]);

  // ── Preview ──
  const handlePreviewAttachment = useCallback((item: AttachmentItem) => {
    if (isImageMime(item.mime)) {
      setPreviewImage(item);
    } else if (item.file) {
      const reader = new FileReader();
      reader.onload = () => {
        setTextPreview({ name: item.name, content: reader.result as string });
      };
      reader.readAsText(item.file);
    }
    // For dialog-sourced non-image files without a File object, text preview not available
    // (would require backend file-read — deferred to a follow-up)
  }, []);

  return {
    attachments,
    fileError,
    fileInputRef,
    pickFiles,
    handleFilesFromInput,
    removeAttachment,
    clearAttachments,
    getAttachmentPaths,
    resolveAttachmentPaths,
    dropZoneHandlers,
    extractPasteFiles,
    previewImage,
    setPreviewImage,
    textPreview,
    setTextPreview,
    handlePreviewAttachment,
    MAX_ATTACHMENT_COUNT: maxCount,
    MAX_ATTACHMENT_TOTAL: maxTotalSize,
  };
}

/** Prevents the browser from navigating to dragged files. Call once per component. */
export function useWindowDragGuard() {
  useEffect(() => {
    const handleWindowDragOver = (event: DragEvent) => {
      if (!hasFileTransfer(event.dataTransfer)) return;
      event.preventDefault();
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = isAttachmentDropTarget(event.target) ? 'copy' : 'none';
      }
    };
    const handleWindowDrop = (event: DragEvent) => {
      if (!hasFileTransfer(event.dataTransfer)) return;
      if (isAttachmentDropTarget(event.target)) return;
      event.preventDefault();
    };
    window.addEventListener('dragover', handleWindowDragOver);
    window.addEventListener('drop', handleWindowDrop);
    return () => {
      window.removeEventListener('dragover', handleWindowDragOver);
      window.removeEventListener('drop', handleWindowDrop);
    };
  }, []);
}
