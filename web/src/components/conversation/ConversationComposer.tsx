import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, Paperclip, Workflow, Bot, X, FileText, Image as ImageIcon, Loader2 } from 'lucide-react';
import type { AgentRegistryVm, ConversationCreateInput, ConversationRunModeVm, ConversationWorkspaceVm } from '../../types';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';

interface AttachmentItem {
  id: string;
  name: string;
  path?: string;
  size: number;
  type: string;
  previewUrl?: string;
  error?: string;
  file?: File;
}

function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const size = bytes / 1024 ** i;
  return `${size < 10 ? size.toFixed(1) : Math.round(size)} ${units[i]}`;
}

function isImageType(mime: string): boolean {
  return mime.startsWith('image/') && !mime.includes('svg');
}

const ALLOWED_ATTACHMENT_EXTS = new Set([
  'txt', 'md', 'json', 'jsonl', 'csv',
  'png', 'jpg', 'jpeg', 'webp',
  'rs', 'ts', 'tsx', 'js', 'jsx', 'py',
  'go', 'java', 'c', 'cpp', 'h', 'hpp',
  'html', 'css', 'xml', 'yaml', 'yml', 'toml',
]);

function attachmentExt(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot === -1 ? '' : name.slice(dot + 1).toLowerCase();
}

function isAllowedAttachment(name: string): boolean {
  const ext = attachmentExt(name);
  return ext !== '' && ALLOWED_ATTACHMENT_EXTS.has(ext);
}

function hasFileTransfer(dataTransfer: DataTransfer | null): boolean {
  if (!dataTransfer) return false;
  return Array.from(dataTransfer.types ?? []).includes('Files')
    || Array.from(dataTransfer.items ?? []).some((item) => item.kind === 'file')
    || dataTransfer.files.length > 0;
}

function extractTransferFiles(dataTransfer: DataTransfer | null): File[] {
  if (!dataTransfer) return [];
  const itemFiles = Array.from(dataTransfer.items ?? [])
    .filter((item) => item.kind === 'file')
    .map((item) => item.getAsFile())
    .filter((file): file is File => !!file);
  if (itemFiles.length > 0) return itemFiles;
  return Array.from(dataTransfer.files ?? []);
}

function isAttachmentDropTarget(target: EventTarget | null): boolean {
  return target instanceof Element && !!target.closest('[data-attachment-dropzone="true"]');
}

interface ConversationComposerProps {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  busy: boolean;
  onRunModeChange: (mode: ConversationRunModeVm) => void;
  onSubmit: (input: ConversationCreateInput) => void;
  onOpenRunModeSettings: () => void;
  onWorkspaceChange: (projectId: string) => void;
}

export function ConversationComposer({
  projectId,
  workspaceName,
  workspaces,
  runMode,
  agentRegistry,
  busy,
  onRunModeChange,
  onSubmit,
  onOpenRunModeSettings,
  onWorkspaceChange,
}: ConversationComposerProps) {
  const { t } = useTranslation();
  const [content, setContent] = useState('');
  const [selectedAgent, setSelectedAgent] = useState(runMode.autoConfig?.agentType ?? '');
  const [selectedModel, setSelectedModel] = useState(runMode.autoConfig?.modelId ?? '');
  const [attachments, setAttachments] = useState<AttachmentItem[]>([]);
  const [previewImage, setPreviewImage] = useState<AttachmentItem | null>(null);
  const [textPreview, setTextPreview] = useState<{ name: string; content: string } | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const dropCounterRef = useRef(0);

  const isAuto = runMode.mode === 'auto';
  const canSubmit = content.trim().length > 0 && !busy;
  const agents = agentRegistry?.agents.filter((a) => a.supported) ?? [];
  const selectedAgentObj = agents.find((a) => a.agentType === selectedAgent);
  const models: { id: string; name: string }[] = [];

  const addFiles = useCallback((files: FileList | File[]) => {
    const items: AttachmentItem[] = [];
    const rejected: string[] = [];
    for (const file of files) {
      if (!isAllowedAttachment(file.name)) {
        rejected.push(file.name);
        continue;
      }
      const mime = file.type || 'application/octet-stream';
      const previewUrl = isImageType(mime) ? URL.createObjectURL(file) : undefined;
      items.push({
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        name: file.name,
        path: (file as any).path as string | undefined,
        size: file.size,
        type: mime,
        previewUrl,
        file,
      });
    }
    if (rejected.length > 0) {
      setFileError(t('conversation.attachmentUnsupportedFile', { names: rejected.join(', ') }));
      setTimeout(() => setFileError(null), 4000);
    }
    setAttachments((prev) => [...prev, ...items]);
    if (fileInputRef.current) fileInputRef.current.value = '';
  }, [t]);

  const removeAttachment = useCallback((id: string) => {
    setAttachments((prev) => {
      const next = prev.filter((a) => a.id !== id);
      const removed = prev.find((a) => a.id === id);
      if (removed?.previewUrl) URL.revokeObjectURL(removed.previewUrl);
      return next;
    });
  }, []);

  // Cleanup preview URLs on unmount
  useEffect(() => {
    return () => {
      for (const a of attachments) {
        if (a.previewUrl) URL.revokeObjectURL(a.previewUrl);
      }
    };
  }, [attachments]);

  useEffect(() => {
    const handleWindowDragOver = (event: DragEvent) => {
      if (!hasFileTransfer(event.dataTransfer)) return;
      event.preventDefault();
      if (event.dataTransfer) event.dataTransfer.dropEffect = isAttachmentDropTarget(event.target) ? 'copy' : 'none';
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

  const handleFilesFromInput = () => {
    const input = fileInputRef.current;
    if (!input?.files?.length) return;
    addFiles(input.files);
  };

  const handleSubmit = () => {
    if (!canSubmit) return;
    const paths = attachments.map((a) => a.path).filter((p): p is string => !!p);
    onSubmit({
      projectId,
      content: content.trim(),
      runMode: runMode.mode,
      workflowTemplateId: isAuto ? undefined : runMode.workflowTemplateId ?? undefined,
      autoConfig: isAuto
        ? { agentType: selectedAgent, modelId: selectedModel || undefined }
        : undefined,
      attachmentPaths: paths.length > 0 ? paths : undefined,
    });
    setContent('');
    setAttachments([]);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  // ── Drag & drop ──
  const handleDragEnter = (e: React.DragEvent) => {
    if (!hasFileTransfer(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    dropCounterRef.current += 1;
  };

  const handleDragLeave = (e: React.DragEvent) => {
    if (!hasFileTransfer(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    dropCounterRef.current -= 1;
  };

  const handleDragOver = (e: React.DragEvent) => {
    if (!hasFileTransfer(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'copy';
  };

  const handleDrop = (e: React.DragEvent) => {
    if (!hasFileTransfer(e.dataTransfer)) return;
    e.preventDefault();
    e.stopPropagation();
    dropCounterRef.current = 0;
    const files = extractTransferFiles(e.dataTransfer);
    if (files.length > 0) addFiles(files);
  };

  // ── Paste ──
  const handlePaste = (e: React.ClipboardEvent) => {
    const items = e.clipboardData?.items;
    if (!items) return;
    const files: File[] = [];
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (item.kind === 'file') {
        const file = item.getAsFile();
        if (file) files.push(file);
      }
    }
    if (files.length > 0) {
      e.preventDefault();
      addFiles(files);
    }
    // Otherwise let native text paste through
  };

  const handlePreviewAttachment = useCallback((item: AttachmentItem) => {
    if (isImageType(item.type)) {
      setPreviewImage(item);
    } else if (item.file) {
      const reader = new FileReader();
      reader.onload = () => {
        setTextPreview({ name: item.name, content: reader.result as string });
      };
      reader.readAsText(item.file);
    }
  }, []);

  const clearAttachments = () => {
    for (const a of attachments) {
      if (a.previewUrl) URL.revokeObjectURL(a.previewUrl);
    }
    setAttachments([]);
  };

  return (
    <>
      <div
        data-attachment-dropzone="true"
        className="flex flex-col gap-4"
        onDragEnter={handleDragEnter}
        onDragLeave={handleDragLeave}
        onDragOver={handleDragOver}
        onDrop={handleDrop}
      >
        {/* Main text input */}
        <div className="rounded-2xl border border-border/60 bg-card/60 p-4 shadow-sm transition-colors focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/10">
          <textarea
            className="w-full min-h-24 resize-none bg-transparent text-sm leading-6 text-foreground placeholder:text-muted-foreground outline-none"
            placeholder={t('conversation.home.inputPlaceholder')}
            value={content}
            onChange={(e) => setContent(e.target.value)}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            onDragEnter={handleDragEnter}
            onDragOver={handleDragOver}
            onDrop={handleDrop}
            disabled={busy}
          />
          <span className="mt-1 text-xs text-muted-foreground">{t('acp.promptInputHint')}</span>
          <div className="mt-3 flex items-center justify-between border-t border-border/50 pt-3">
            <div className="flex items-center gap-3">
              <input
                ref={fileInputRef}
                type="file"
                multiple
                className="hidden"
                onChange={handleFilesFromInput}
              />
              <Button
                variant="ghost"
                size="icon"
                className="size-8 text-muted-foreground"
                onClick={() => fileInputRef.current?.click()}
                disabled={busy}
              >
                <Paperclip className="size-4" />
              </Button>
              {attachments.length > 0 ? (
                <span className="text-xs text-muted-foreground">{attachments.length} file(s)</span>
              ) : workspaces.length > 1 ? (
                <Select value={projectId} onValueChange={onWorkspaceChange}>
                  <SelectTrigger className="h-7 w-auto min-w-[140px] gap-1 border-0 bg-transparent px-1 text-xs text-muted-foreground hover:text-foreground focus:ring-0">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {workspaces.map((w) => (
                      <SelectItem key={w.projectId} value={w.projectId}>{w.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <span className="text-xs text-muted-foreground">{workspaceName}</span>
              )}
            </div>
            <Button size="sm" className="h-8 gap-1.5 rounded-full px-3" disabled={!canSubmit} onClick={handleSubmit}>
              <Send className="size-3.5" />
              {t('acp.send')}
            </Button>
          </div>
        </div>

        {/* Attachment chips */}
        {attachments.length > 0 ? (
          <div className="flex flex-wrap items-center gap-2 rounded-xl border border-border/40 bg-card/30 px-3 py-2">
            {attachments.map((a) => (
              <AttachmentChip
                key={a.id}
                item={a}
                onRemove={() => removeAttachment(a.id)}
                onPreview={() => handlePreviewAttachment(a)}
              />
            ))}
            <Button variant="ghost" size="sm" className="h-7 text-xs text-muted-foreground" onClick={clearAttachments}>
              {t('common.clear') ?? 'Clear all'}
            </Button>
          </div>
        ) : null}

        {/* File error */}
        {fileError ? (
          <div className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">{fileError}</div>
        ) : null}

        {/* Run mode selector */}
        <div className="flex items-center gap-3 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
          <span className="text-xs font-medium text-muted-foreground">{t('conversation.home.runMode')}</span>
          <div className="flex rounded-lg bg-muted p-0.5">
            <button
              type="button"
              className={cn(
                'rounded-md px-3 py-1 text-xs font-medium transition-colors',
                isAuto ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
              )}
              onClick={() => onRunModeChange({ mode: 'auto', autoConfig: runMode.autoConfig })}
            >
              {t('conversation.home.auto')}
            </button>
            <button
              type="button"
              className={cn(
                'rounded-md px-3 py-1 text-xs font-medium transition-colors',
                !isAuto ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
              )}
              onClick={() => onRunModeChange({ mode: 'workflow', workflowTemplateId: runMode.workflowTemplateId })}
            >
              {t('conversation.home.workflow')}
            </button>
          </div>

          {/* Configure button */}
          <Button variant="ghost" size="sm" className="ml-auto h-7 gap-1 text-xs" onClick={onOpenRunModeSettings}>
            <Workflow className="size-3" />
            {t('conversation.home.configureNow')}
          </Button>
        </div>

        {/* AUTO options: agent + model */}
        {isAuto ? (
          <div className="flex items-center gap-3 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
            <Bot className="size-4 text-muted-foreground" />
            <div className="flex items-center gap-3 flex-1">
              <Select value={selectedAgent} onValueChange={(v) => { setSelectedAgent(v); setSelectedModel(''); }}>
                <SelectTrigger className="h-8 w-[180px] text-xs">
                  <SelectValue placeholder={t('conversation.home.selectAgent')} />
                </SelectTrigger>
                <SelectContent>
                  {agents.map((a) => (
                    <SelectItem key={a.agentType} value={a.agentType}>{a.displayName}</SelectItem>
                  ))}
                  {agents.length === 0 ? (
                    <div className="px-2 py-3 text-xs text-muted-foreground">{t('conversation.home.noAgent')}</div>
                  ) : null}
                </SelectContent>
              </Select>
              {selectedAgentObj && models.length > 0 ? (
                <Select value={selectedModel} onValueChange={setSelectedModel}>
                  <SelectTrigger className="h-8 w-[180px] text-xs">
                    <SelectValue placeholder={t('conversation.home.selectModel')} />
                  </SelectTrigger>
                  <SelectContent>
                    {models.map((m) => (
                      <SelectItem key={m.id} value={m.id}>{m.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : null}
            </div>
          </div>
        ) : null}
      </div>

      {/* Image preview dialog */}
      <Dialog open={!!previewImage} onOpenChange={(open) => { if (!open) setPreviewImage(null); }}>
        <DialogContent
          showCloseButton={false}
          overlayClassName="bg-black/70"
          className="!w-auto !max-w-[calc(100vw-4rem)] !gap-0 border-0 bg-transparent p-0 shadow-none sm:!max-w-[calc(100vw-4rem)]"
        >
          <DialogTitle className="sr-only">{previewImage?.name ?? 'Image Preview'}</DialogTitle>
          {previewImage?.previewUrl ? (
            <img
              src={previewImage.previewUrl}
              alt={previewImage.name}
              draggable={false}
              className="block max-h-[calc(100vh-4rem)] max-w-[calc(100vw-4rem)] object-contain"
            />
          ) : null}
        </DialogContent>
      </Dialog>

      {/* Text preview dialog */}
      <Dialog open={!!textPreview} onOpenChange={(open) => { if (!open) setTextPreview(null); }}>
        <DialogContent className="max-h-[86vh] max-w-4xl gap-0 overflow-hidden p-0">
          <DialogHeader className="border-b px-5 py-3">
            <DialogTitle className="truncate text-sm">{textPreview?.name}</DialogTitle>
          </DialogHeader>
          <pre className="max-h-[70vh] overflow-auto p-5 font-mono text-xs leading-relaxed text-foreground/85 whitespace-pre-wrap break-words">{textPreview?.content}</pre>
        </DialogContent>
      </Dialog>
    </>
  );
}

function AttachmentChip({ item, onRemove, onPreview }: { item: AttachmentItem; onRemove: () => void; onPreview?: () => void }) {
  const isImage = isImageType(item.type);
  const Icon = isImage ? ImageIcon : FileText;

  return (
    <div
      className="group relative flex cursor-pointer items-center gap-2 rounded-lg border border-border/60 bg-background/70 px-2 py-1.5 text-xs shadow-sm transition-colors hover:border-border"
      onClick={onPreview}
      title={item.name}
    >
      {isImage && item.previewUrl ? (
        <img src={item.previewUrl} alt={item.name} className="size-8 shrink-0 rounded object-cover" />
      ) : (
        <span className="flex size-8 shrink-0 items-center justify-center rounded bg-muted/50 text-muted-foreground">
          <Icon className="size-4" />
        </span>
      )}
      <span className="min-w-0 max-w-[140px] truncate font-medium">{item.name}</span>
      <Badge variant="secondary" className="shrink-0 rounded-full px-1.5 py-0 text-[10px] font-normal">
        {formatSize(item.size)}
      </Badge>
      <Button
        variant="ghost"
        size="icon"
        className="size-5 shrink-0 rounded-full opacity-0 group-hover:opacity-100 transition-opacity"
        onClick={(e) => { e.stopPropagation(); onRemove(); }}
        title="Remove"
      >
        <X className="size-3" />
      </Button>
    </div>
  );
}

