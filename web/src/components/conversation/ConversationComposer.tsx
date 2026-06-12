import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, Paperclip, Workflow, Bot, X, FileText, Folders, Image as ImageIcon } from 'lucide-react';
import type { AgentRegistryVm, ConversationAutoConfigVm, ConversationCreateInput, ConversationRunModeVm, ConversationWorkspaceVm, WorkflowTemplateStore } from '../../types';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Badge } from '@/components/ui/badge';
import { selectableAgentOptions, validateAutoConfig } from '@/lib/run-mode-validation';
import { cn } from '@/lib/utils';
import { isAllowedAttachment, isImageMime, useAttachmentExtensions } from '@/lib/attachments';

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
  workflowTemplates: WorkflowTemplateStore | null;
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
  workflowTemplates,
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
  const [selectedPermissionMode, setSelectedPermissionMode] = useState(runMode.autoConfig?.permissionMode ?? '');
  const [globalGoal, setGlobalGoal] = useState(runMode.autoConfig?.globalGoal ?? '');
  const [workflowTemplateId, setWorkflowTemplateId] = useState(runMode.workflowTemplateId ?? '');
  const [attachments, setAttachments] = useState<AttachmentItem[]>([]);
  const [runModeError, setRunModeError] = useState<string | null>(null);
  const [previewImage, setPreviewImage] = useState<AttachmentItem | null>(null);
  const [textPreview, setTextPreview] = useState<{ name: string; content: string } | null>(null);
  const [fileError, setFileError] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const dropCounterRef = useRef(0);
  const allowedExts = useAttachmentExtensions();
  const MAX_ATTACHMENT_COUNT = 20;
  const MAX_ATTACHMENT_TOTAL = 50 * 1024 * 1024;

  const isAuto = runMode.mode === 'auto';
  const autoStrategy = runMode.autoConfig?.agentStrategy ?? 'fixed';
  const isDynamicAuto = autoStrategy === 'dynamic';
  const canSubmit = content.trim().length > 0 && !busy;
  const agentOptions = selectableAgentOptions(agentRegistry, t);
  const agents = agentOptions.filter((item) => item.selectable).map((item) => item.agent);
  const selectedAgentObj = agents.find((a) => a.agentType === selectedAgent);
  const models = selectedAgentObj?.supportedModels ?? [];
  const permissionModes = selectedAgentObj?.supportedModes ?? [];
  const templates = workflowTemplates?.templates ?? [];

  useEffect(() => {
    setSelectedAgent(runMode.autoConfig?.agentType ?? '');
    setSelectedModel(runMode.autoConfig?.modelId ?? '');
    setSelectedPermissionMode(runMode.autoConfig?.permissionMode ?? '');
    setGlobalGoal(runMode.autoConfig?.globalGoal ?? '');
    setWorkflowTemplateId(runMode.workflowTemplateId ?? workflowTemplates?.lastUsedTemplateId ?? templates[0]?.id ?? '');
  }, [runMode, workflowTemplates]);

  const autoConfigWithSession = (patch: Partial<ConversationAutoConfigVm> = {}): ConversationAutoConfigVm => {
    const base = runMode.autoConfig ?? { agentType: selectedAgent };
    if (isDynamicAuto) {
      return {
        ...base,
        agentStrategy: 'dynamic',
        agentType: base.agentType || base.bootstrapAgentType || selectedAgent,
        permissionMode: selectedPermissionMode || undefined,
        globalGoal: globalGoal.trim() || undefined,
        ...patch,
      };
    }
    return {
      ...base,
      agentStrategy: 'fixed',
      agentType: selectedAgent,
      modelId: selectedModel || undefined,
      permissionMode: selectedPermissionMode || undefined,
      globalGoal: globalGoal.trim() || undefined,
      ...patch,
    };
  };

  const updateAutoSession = (patch: Partial<ConversationAutoConfigVm>) => {
    onRunModeChange({ mode: 'auto', autoConfig: autoConfigWithSession(patch) });
  };

  const addFiles = useCallback((files: FileList | File[]) => {
    const items: AttachmentItem[] = [];
    const rejected: string[] = [];
    let err: string | null = null;
    for (const file of files) {
      if (allowedExts && !isAllowedAttachment(file.name, allowedExts)) {
        rejected.push(file.name);
        continue;
      }
      const mime = file.type || 'application/octet-stream';
      const previewUrl = isImageMime(mime) ? URL.createObjectURL(file) : undefined;
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
      err = t('conversation.attachmentUnsupportedFile', { names: rejected.join(', ') });
    }
    if (!err) {
      const total = attachments.length + items.length;
      if (total > MAX_ATTACHMENT_COUNT) {
        err = t('conversation.attachmentCountExceeded', { max: MAX_ATTACHMENT_COUNT });
        items.length = Math.max(0, MAX_ATTACHMENT_COUNT - attachments.length);
      }
    }
    if (!err) {
      const totalSize = attachments.reduce((s, a) => s + a.size, 0)
        + items.reduce((s, a) => s + a.size, 0);
      if (totalSize > MAX_ATTACHMENT_TOTAL) {
        err = t('conversation.attachmentTotalTooLarge');
      }
    }
    if (err) {
      setFileError(err);
      setTimeout(() => setFileError(null), 4000);
    }
    setAttachments((prev) => [...prev, ...items]);
    if (fileInputRef.current) fileInputRef.current.value = '';
  }, [t, attachments, allowedExts]);

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
    const input: ConversationCreateInput = {
      projectId,
      content: content.trim(),
      runMode: runMode.mode,
      workflowTemplateId: isAuto ? undefined : workflowTemplateId || runMode.workflowTemplateId || undefined,
      autoConfig: isAuto
        ? autoConfigWithSession()
        : undefined,
      attachmentPaths: paths.length > 0 ? paths : undefined,
    };
    const localIssues = isAuto
      ? validateAutoConfig(input.autoConfig, agentRegistry, workflowTemplates, t)
      : !input.workflowTemplateId
        ? [t('conversation.home.selectWorkflowTemplate')]
        : [];
    if (localIssues.length > 0) {
      setRunModeError(localIssues.join('\n'));
      return;
    }
    setRunModeError(null);
    onSubmit(input);
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
    if (isImageMime(item.type)) {
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
          <div className="mt-3 flex items-center justify-between gap-3 border-t border-border/40 pt-3">
            <div className="flex min-w-0 items-center gap-2">
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
                className="size-9 rounded-full border border-border/50 bg-gold-surface-high/25 text-muted-foreground hover:bg-gold-surface-high/55 hover:text-foreground"
                onClick={() => fileInputRef.current?.click()}
                disabled={busy}
              >
                <Paperclip className="size-4" />
              </Button>
              {attachments.length > 0 ? (
                <span className="shrink-0 rounded-full border border-border/50 bg-gold-surface-high/30 px-2.5 py-1 text-[11px] font-medium text-muted-foreground">
                  {attachments.length} file(s)
                </span>
              ) : null}
              {workspaces.length > 1 ? (
                <Select value={projectId} onValueChange={onWorkspaceChange}>
                  <SelectTrigger className="h-9 min-w-[170px] max-w-[240px] gap-2 rounded-full border-border/50 bg-gold-surface-high/35 px-3 text-sm text-foreground shadow-none hover:bg-gold-surface-high/55 focus-visible:border-primary/30 focus-visible:ring-2 focus-visible:ring-primary/10 dark:bg-gold-surface-high/35 dark:hover:bg-gold-surface-high/55">
                    <span className="flex min-w-0 items-center gap-2">
                      <Folders className="size-3.5 shrink-0 text-muted-foreground/80" />
                      <SelectValue />
                    </span>
                  </SelectTrigger>
                  <SelectContent position="popper" align="start">
                    {workspaces.map((w) => (
                      <SelectItem key={w.projectId} value={w.projectId}>{w.name}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              ) : (
                <div className="flex h-9 min-w-[170px] max-w-[240px] items-center gap-2 rounded-full border border-border/50 bg-gold-surface-high/35 px-3 text-sm text-foreground">
                  <Folders className="size-3.5 shrink-0 text-muted-foreground/80" />
                  <span className="truncate">{workspaceName}</span>
                </div>
              )}
            </div>
            <Button size="sm" className="h-8 shrink-0 gap-1.5 rounded-full px-3" disabled={!canSubmit} onClick={handleSubmit}>
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
              onClick={() => onRunModeChange({ mode: 'auto', autoConfig: autoConfigWithSession() })}
            >
              {t('conversation.home.auto')}
            </button>
            <button
              type="button"
              className={cn(
                'rounded-md px-3 py-1 text-xs font-medium transition-colors',
                !isAuto ? 'bg-background text-foreground shadow-sm' : 'text-muted-foreground hover:text-foreground',
              )}
              onClick={() => onRunModeChange({ mode: 'workflow', workflowTemplateId: workflowTemplateId || runMode.workflowTemplateId })}
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

        {isAuto ? (
          <div className="space-y-3 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
            <div className="flex items-center gap-3">
              <Bot className="size-4 text-muted-foreground" />
              <div className="flex min-w-0 flex-1 flex-wrap items-center gap-3">
                {isDynamicAuto ? (
                  <div className="flex h-8 min-w-0 items-center rounded-md border border-border/60 bg-background/40 px-3 text-xs text-muted-foreground">
                    <span className="truncate">{t('conversation.home.dynamicAgent')}</span>
                  </div>
                ) : (
                  <Select value={selectedAgent} onValueChange={(v) => { setSelectedAgent(v); setSelectedModel(''); updateAutoSession({ agentType: v, modelId: undefined }); }}>
                    <SelectTrigger className="h-8 w-[180px] min-w-0 text-xs">
                      <SelectValue placeholder={t('conversation.home.selectAgent')} />
                    </SelectTrigger>
                    <SelectContent position="popper" align="start">
                      {agentOptions.map(({ agent: a, selectable, reason }) => (
                        <SelectItem key={a.agentType} value={a.agentType} disabled={!selectable}>
                          <span className="block min-w-0">
                            <span className="block truncate">{a.displayName}</span>
                            {!selectable && reason ? <span className="mt-0.5 block whitespace-normal text-[11px] text-destructive">{reason}</span> : null}
                          </span>
                        </SelectItem>
                      ))}
                      {agentOptions.length === 0 ? (
                        <div className="px-2 py-3 text-xs text-muted-foreground">{t('conversation.home.noAgent')}</div>
                      ) : null}
                    </SelectContent>
                  </Select>
                )}
                {!isDynamicAuto && selectedAgentObj && models.length > 0 ? (
                  <Select value={selectedModel} onValueChange={(modelId) => { setSelectedModel(modelId); updateAutoSession({ modelId }); }}>
                    <SelectTrigger className="h-8 w-[200px] min-w-0 text-xs">
                      <span className="min-w-0 flex-1 truncate text-left">{models.find((m) => m.id === selectedModel)?.name ?? t('conversation.home.selectModel')}</span>
                    </SelectTrigger>
                    <SelectContent position="popper" align="start" className="w-[min(26rem,calc(100vw-2rem))]">
                      {models.map((m) => (
                        <SelectItem key={m.id} value={m.id} className="items-start py-2">
                          <span className="block min-w-0">
                            <span className="block truncate font-medium">{m.name}</span>
                            {m.description ? <span className="mt-0.5 block whitespace-normal break-words text-[11px] leading-4 text-muted-foreground">{m.description}</span> : null}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                ) : null}
                <Select value={selectedPermissionMode || '__default__'} onValueChange={(value) => { const next = value === '__default__' ? '' : value; setSelectedPermissionMode(next); updateAutoSession({ permissionMode: next || undefined }); }}>
                  <SelectTrigger className="h-8 w-[180px] min-w-0 text-xs">
                    <SelectValue placeholder={t('runMode.permissionMode')} />
                  </SelectTrigger>
                  <SelectContent position="popper" align="start">
                    <SelectItem value="__default__">{t('workflowEditor.permissionModeDefault')}</SelectItem>
                    {isDynamicAuto ? (
                      <>
                        <SelectItem value="read_only">{t('workflowEditor.permissionModeReadOnly')}</SelectItem>
                        <SelectItem value="ask">{t('workflowEditor.permissionModeAsk')}</SelectItem>
                        <SelectItem value="full_access">{t('workflowEditor.permissionModeFullAccess')}</SelectItem>
                      </>
                    ) : permissionModes.map((mode) => <SelectItem value={mode.id} key={mode.id}>{mode.name}</SelectItem>)}
                  </SelectContent>
                </Select>
                <Button variant="ghost" size="sm" className="h-7 gap-1 text-xs" onClick={onOpenRunModeSettings}>
                  <Workflow className="size-3" />
                  {t('conversation.home.configureAuto')}
                </Button>
              </div>
            </div>
            <textarea
              className="w-full min-h-14 resize-y rounded-md border border-border/60 bg-background/35 px-3 py-2 text-xs leading-5 text-foreground outline-none placeholder:text-muted-foreground focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/10"
              value={globalGoal}
              placeholder={t('runMode.globalGoalPlaceholder')}
              onChange={(event) => {
                setGlobalGoal(event.target.value);
                updateAutoSession({ globalGoal: event.target.value.trim() || undefined });
              }}
            />
          </div>
        ) : (
          <div className="flex items-center gap-3 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
            <Workflow className="size-4 text-muted-foreground" />
            <Select value={workflowTemplateId} onValueChange={(id) => { setWorkflowTemplateId(id); onRunModeChange({ mode: 'workflow', workflowTemplateId: id }); }}>
              <SelectTrigger className="h-8 min-w-0 flex-1 text-xs">
                <SelectValue placeholder={t('conversation.home.selectWorkflowTemplate')} />
              </SelectTrigger>
              <SelectContent position="popper" align="start">
                {templates.map((tpl) => (
                  <SelectItem key={tpl.id} value={tpl.id}>{tpl.name}</SelectItem>
                ))}
                {templates.length === 0 ? (
                  <div className="px-2 py-3 text-xs text-muted-foreground">{t('conversation.home.noWorkflowTemplate')}</div>
                ) : null}
              </SelectContent>
            </Select>
            <Button variant="ghost" size="sm" className="h-7 gap-1 text-xs" onClick={onOpenRunModeSettings}>
              <Workflow className="size-3" />
              {t('conversation.home.configureWorkflow')}
            </Button>
          </div>
        )}
        {runModeError ? (
          <div className="flex items-start gap-3 whitespace-pre-wrap rounded-xl border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive">
            <span className="min-w-0 flex-1">{runModeError}</span>
            <Button variant="outline" size="sm" className="h-7 shrink-0 border-destructive/30 bg-background/40 px-2 text-xs text-destructive hover:text-destructive" onClick={onOpenRunModeSettings}>
              <Workflow className="mr-1 size-3" />
              {t('conversation.runtime.repairAction')}
            </Button>
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
  const isImage = isImageMime(item.type);
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
