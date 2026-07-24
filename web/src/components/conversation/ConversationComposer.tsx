import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Send, Paperclip, Workflow, Bot, Folders, Plus } from 'lucide-react';
import type { AgentRegistryVm, ConversationAutoConfigVm, ConversationCreateInput, ConversationDirectConfigVm, ConversationRunModeVm, ConversationWorkspaceVm, ProfileVm, WorkflowTemplateStore } from '../../types';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { canOpenRunModeManagement, CONVERSATION_RUN_MODE_ORDER, directConfigForAgent, includeInterviewForSubmit, normalizeConversationAutoConfigForSubmit, normalizeConversationDirectConfigForSubmit, optionalRunModeText, shouldShowInterviewToggle } from '@/lib/conversation-run-mode-config';
import { selectableAgentOptions, validateAutoConfig, validateDirectConfig, validateWorkflowTemplateForConversationStartWithFreshProfiles } from '@/lib/run-mode-validation';
import { useAttachmentPicker, useWindowDragGuard } from '@/lib/attachment-service';
import { AttachmentChipsList, AttachmentPreviewDialogs } from '@/components/shared/AttachmentComponents';
import { useConversationComposerDraft } from '@/lib/conversation-composer-draft';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';

interface ConversationComposerProps {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  profiles: ProfileVm[];
  busy: boolean;
  onRunModeChange: (mode: ConversationRunModeVm) => void;
  onLoadProfiles: () => Promise<ProfileVm[]>;
  onSubmit: (input: ConversationCreateInput) => Promise<string | null | undefined> | string | null | undefined;
  onOpenAgentManagement: () => void;
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
  profiles,
  busy,
  onRunModeChange,
  onLoadProfiles,
  onSubmit,
  onOpenAgentManagement,
  onOpenRunModeSettings,
  onWorkspaceChange,
}: ConversationComposerProps) {
  const { t } = useTranslation();
  const composerDraft = useConversationComposerDraft();
  const content = composerDraft.draft.content;
  const setContent = composerDraft.setContent;
  const [selectedDirectAgent, setSelectedDirectAgent] = useState(runMode.directConfig?.agentType ?? '');
  const [selectedDirectModel, setSelectedDirectModel] = useState(runMode.directConfig?.modelId ?? '');
  const [selectedDirectPermissionMode, setSelectedDirectPermissionMode] = useState(runMode.directConfig?.permissionMode ?? '');
  const [selectedAgent, setSelectedAgent] = useState(runMode.autoConfig?.agentType ?? '');
  const [selectedModel, setSelectedModel] = useState(runMode.autoConfig?.modelId ?? '');
  const [selectedPermissionMode, setSelectedPermissionMode] = useState(runMode.autoConfig?.permissionMode ?? '');
  const [globalGoal, setGlobalGoal] = useState(runMode.autoConfig?.globalGoal ?? '');
  const [workflowTemplateId, setWorkflowTemplateId] = useState(runMode.workflowTemplateId ?? '');
  const [runModeError, setRunModeError] = useState<string | null>(null);
  const [submittingAttachments, setSubmittingAttachments] = useState(false);
  const [selectedProjectId, setSelectedProjectId] = useState(projectId);

  useEffect(() => {
    setSelectedProjectId(projectId);
  }, [projectId]);

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
  } = useAttachmentPicker({ attachments: [composerDraft.draft.attachments, composerDraft.setAttachments] });

  useWindowDragGuard();

  const isAuto = runMode.mode === 'auto';
  const isDirect = runMode.mode === 'direct';
  const showRunModeManagement = canOpenRunModeManagement(runMode.mode);
  const autoStrategy = runMode.autoConfig?.agentStrategy ?? 'fixed';
  const isDynamicAuto = autoStrategy === 'dynamic';
  const canSubmit = content.trim().length > 0 && !busy && !submittingAttachments;
  const agentOptions = useMemo(() => selectableAgentOptions(agentRegistry, t), [agentRegistry, t]);
  const agents = useMemo(
    () => agentOptions.filter((item) => item.selectable).map((item) => item.agent),
    [agentOptions],
  );
  const selectedAgentObj = agents.find((a) => a.agentType === selectedAgent);
  const selectedDirectAgentObj = agents.find((agent) => agent.agentType === selectedDirectAgent);
  const directModels = selectedDirectAgentObj?.supportedModels ?? [];
  const directPermissionModes = selectedDirectAgentObj?.supportedModes ?? [];
  const models = selectedAgentObj?.supportedModels ?? [];
  const permissionModes = selectedAgentObj?.supportedModes ?? [];
  const templates = workflowTemplates?.templates ?? [];
  const selectedWorkflowTemplateId = workflowTemplateId || runMode.workflowTemplateId || undefined;
  const showInterviewToggle = shouldShowInterviewToggle(runMode.mode, selectedWorkflowTemplateId);

  useEffect(() => {
    const fallbackAgent = runMode.directConfig?.agentType
      || agents[0]?.agentType
      || '';
    const directConfig = fallbackAgent ? directConfigForAgent(runMode, fallbackAgent) : undefined;
    setSelectedDirectAgent(fallbackAgent);
    setSelectedDirectModel(directConfig?.modelId ?? '');
    setSelectedDirectPermissionMode(directConfig?.permissionMode ?? '');
    setSelectedAgent(runMode.autoConfig?.agentType ?? '');
    setSelectedModel(runMode.autoConfig?.modelId ?? '');
    setSelectedPermissionMode(runMode.autoConfig?.permissionMode ?? '');
    setGlobalGoal(runMode.autoConfig?.globalGoal ?? '');
    setWorkflowTemplateId(runMode.workflowTemplateId ?? workflowTemplates?.lastUsedTemplateId ?? templates[0]?.id ?? '');
  }, [runMode, workflowTemplates, agents]);

  const updateDirectConfig = (config: ConversationDirectConfigVm) => {
    onRunModeChange({
      mode: 'direct',
      directConfig: config,
      directPreferences: {
        ...runMode.directPreferences,
        [config.agentType]: config,
      },
    });
  };

  const selectDirectAgent = (agentType: string) => {
    const remembered = directConfigForAgent(runMode, agentType);
    setSelectedDirectAgent(agentType);
    setSelectedDirectModel(remembered.modelId ?? '');
    setSelectedDirectPermissionMode(remembered.permissionMode ?? '');
    updateDirectConfig(remembered);
  };

  const patchedValue = <K extends keyof ConversationAutoConfigVm>(
    patch: Partial<ConversationAutoConfigVm>,
    key: K,
    fallback: ConversationAutoConfigVm[K] | undefined,
  ): ConversationAutoConfigVm[K] | undefined => (
    Object.prototype.hasOwnProperty.call(patch, key) ? patch[key] : fallback
  );

  const autoConfigWithSession = (patch: Partial<ConversationAutoConfigVm> = {}): ConversationAutoConfigVm => {
    const base = runMode.autoConfig ?? { agentType: selectedAgent };
    const nextAgent = patchedValue(patch, 'agentType', selectedAgent);
    const nextModel = patchedValue(patch, 'modelId', selectedModel);
    const nextPermissionMode = patchedValue(patch, 'permissionMode', selectedPermissionMode);
    const nextGlobalGoal = patchedValue(patch, 'globalGoal', globalGoal);
    if (isDynamicAuto) {
      return {
        ...base,
        agentStrategy: 'dynamic',
        agentType: base.agentType || base.bootstrapAgentType || nextAgent || '',
        ...patch,
        permissionMode: nextPermissionMode || undefined,
        globalGoal: optionalRunModeText(nextGlobalGoal),
      };
    }
    return {
      ...base,
      agentStrategy: 'fixed',
      ...patch,
      agentType: nextAgent || '',
      modelId: nextModel || undefined,
      permissionMode: nextPermissionMode || undefined,
      globalGoal: optionalRunModeText(nextGlobalGoal),
    };
  };

  const updateAutoSession = (patch: Partial<ConversationAutoConfigVm>) => {
    onRunModeChange({ mode: 'auto', autoConfig: autoConfigWithSession(patch) });
  };

  const handleSubmit = async () => {
    if (!canSubmit) return;
    const trimmed = content.trim();
    const inputBase: ConversationCreateInput = {
      projectId: selectedProjectId,
      content: trimmed,
      runMode: runMode.mode,
      workflowTemplateId: isAuto || isDirect ? undefined : selectedWorkflowTemplateId,
      includeInterview: includeInterviewForSubmit(runMode, selectedWorkflowTemplateId),
      directConfig: isDirect
        ? normalizeConversationDirectConfigForSubmit({
          agentType: selectedDirectAgent,
          modelId: selectedDirectModel || undefined,
          permissionMode: selectedDirectPermissionMode || undefined,
        })
        : undefined,
      autoConfig: isAuto
        ? normalizeConversationAutoConfigForSubmit(autoConfigWithSession())
        : undefined,
    };
    setSubmittingAttachments(true);
    try {
      const localIssues = isDirect
        ? validateDirectConfig(inputBase.directConfig, agentRegistry, t)
        : isAuto
          ? validateAutoConfig(inputBase.autoConfig, agentRegistry, workflowTemplates, t)
          : await validateWorkflowTemplateForConversationStartWithFreshProfiles(
          inputBase.workflowTemplateId,
          agentRegistry,
          profiles,
          onLoadProfiles,
          workflowTemplates,
          t,
        );
      if (localIssues.length > 0) {
        setRunModeError(localIssues.join('\n'));
        return;
      }
      const paths = await resolveAttachmentPaths();
      setRunModeError(null);
      const submitError = await onSubmit({
        ...inputBase,
        attachmentPaths: paths.length > 0 ? paths : undefined,
      });
      if (submitError) {
        setRunModeError(submitError);
        return;
      }
      composerDraft.reset();
    } catch {
      // Attachment hook owns the user-facing file error.
    } finally {
      setSubmittingAttachments(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void handleSubmit();
    }
  };

  return (
    <>
      <div
        data-attachment-dropzone="true"
        className="flex flex-col gap-4"
        {...dropZoneHandlers}
      >
        {/* Main text input */}
        <div className="rounded-2xl border border-border/60 bg-card/60 p-4 shadow-sm transition-colors focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/10">
          <textarea
            className="w-full min-h-24 resize-none bg-transparent text-sm leading-6 text-foreground placeholder:text-muted-foreground outline-none"
            placeholder={t('conversation.home.inputPlaceholder')}
            value={content}
            onChange={(e) => setContent(e.target.value)}
            onKeyDown={handleKeyDown}
            onPaste={(e) => { void extractPasteFiles(e); }}
            onDragEnter={dropZoneHandlers.onDragEnter}
            onDragOver={dropZoneHandlers.onDragOver}
            onDrop={dropZoneHandlers.onDrop}
            disabled={busy || submittingAttachments}
          />
          <span className="mt-1 text-xs text-muted-foreground">{t('acp.promptInputHint')}</span>
          <div
            data-slot="conversation-composer-toolbar"
            className="mt-3 flex flex-wrap items-center gap-3 border-t border-border/40 pt-3"
          >
            <div
              data-slot="conversation-composer-leading-actions"
              className="flex min-w-0 flex-1 basis-[15rem] flex-wrap items-center gap-2"
            >
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
                onClick={() => { void pickFiles(); }}
                disabled={busy || submittingAttachments}
              >
                <Paperclip className="size-4" />
              </Button>
              {attachments.length > 0 ? (
                <span className="shrink-0 rounded-full border border-border/50 bg-gold-surface-high/30 px-2.5 py-1 text-[11px] font-medium text-muted-foreground">
                  {attachments.length} file(s)
                </span>
              ) : null}
              {workspaces.length > 1 ? (
                <Select value={selectedProjectId} onValueChange={(id) => { setSelectedProjectId(id); onWorkspaceChange(id); }}>
                  <SelectTrigger className="h-9 min-w-[140px] max-w-[240px] flex-1 gap-2 rounded-full border-border/50 bg-gold-surface-high/35 px-3 text-sm text-foreground shadow-none hover:bg-gold-surface-high/55 focus-visible:border-primary/30 focus-visible:ring-2 focus-visible:ring-primary/10 dark:bg-gold-surface-high/35 dark:hover:bg-gold-surface-high/55">
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
                <div className="flex h-9 min-w-[140px] max-w-[240px] flex-1 items-center gap-2 rounded-full border border-border/50 bg-gold-surface-high/35 px-3 text-sm text-foreground">
                  <Folders className="size-3.5 shrink-0 text-muted-foreground/80" />
                  <span className="truncate">{workspaceName}</span>
                </div>
              )}
            </div>
            <div
              data-slot="conversation-composer-trailing-actions"
              className="ml-auto flex min-w-0 flex-1 basis-[22rem] flex-wrap items-center justify-end gap-2"
            >
              {isDirect ? (
                <>
                  {directModels.length > 0 ? (
                    <Select value={selectedDirectModel || '__default__'} onValueChange={(value) => {
                      const modelId = value === '__default__' ? '' : value;
                      setSelectedDirectModel(modelId);
                      updateDirectConfig({
                        agentType: selectedDirectAgent,
                        modelId: modelId || undefined,
                        permissionMode: selectedDirectPermissionMode || undefined,
                      });
                    }}>
                      <SelectTrigger className="h-8 min-w-[130px] max-w-[180px] flex-1 rounded-full border-border/50 bg-gold-surface-high/35 text-xs">
                        <SelectValue placeholder={t('conversation.home.selectModel')} />
                      </SelectTrigger>
                      <SelectContent position="popper" align="end">
                        <SelectItem value="__default__">{t('conversation.home.defaultModel')}</SelectItem>
                        {directModels.map((model) => <SelectItem value={model.id} key={model.id}>{model.name}</SelectItem>)}
                      </SelectContent>
                    </Select>
                  ) : null}
                  <Select value={selectedDirectPermissionMode || '__default__'} onValueChange={(value) => {
                    const permissionMode = value === '__default__' ? '' : value;
                    setSelectedDirectPermissionMode(permissionMode);
                    updateDirectConfig({
                      agentType: selectedDirectAgent,
                      modelId: selectedDirectModel || undefined,
                      permissionMode: permissionMode || undefined,
                    });
                  }}>
                    <SelectTrigger className="h-8 min-w-[130px] max-w-[180px] flex-1 rounded-full border-border/50 bg-gold-surface-high/35 text-xs">
                      <SelectValue placeholder={t('runMode.permissionMode')} />
                    </SelectTrigger>
                    <SelectContent position="popper" align="end">
                      <SelectItem value="__default__">{t('workflowEditor.permissionModeDefault')}</SelectItem>
                      {directPermissionModes.map((mode) => <SelectItem value={mode.id} key={mode.id}>{mode.name}</SelectItem>)}
                    </SelectContent>
                  </Select>
                </>
              ) : null}
              <Button size="sm" className="h-8 shrink-0 gap-1.5 rounded-full px-3" disabled={!canSubmit} onClick={() => { void handleSubmit(); }}>
                <Send className="size-3.5" />
                {t('acp.send')}
              </Button>
            </div>
          </div>
        </div>

        {/* Attachment chips */}
        <AttachmentChipsList
          attachments={attachments}
          onRemove={removeAttachment}
          onPreview={handlePreviewAttachment}
          onClear={clearAttachments}
          clearLabel={t('common.clear') ?? 'Clear all'}
        />

        {/* File error */}
        {fileError ? (
          <div className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">{fileError}</div>
        ) : null}

        {/* Run mode selector */}
        <div className="flex items-center gap-3 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
          <span className="text-xs font-medium text-muted-foreground">{t('conversation.home.runMode')}</span>
          <Tabs value={runMode.mode} onValueChange={(value) => {
            if (value === 'direct') {
              const agentType = selectedDirectAgent || agents[0]?.agentType || '';
              if (agentType) {
                const config = directConfigForAgent(runMode, agentType);
                selectDirectAgent(config.agentType);
              } else {
                onRunModeChange({ mode: 'direct', directPreferences: runMode.directPreferences });
              }
            } else if (value === 'workflow') {
              onRunModeChange({ mode: 'workflow', workflowTemplateId: workflowTemplateId || runMode.workflowTemplateId, includeInterview: runMode.includeInterview });
            } else {
              onRunModeChange({ mode: 'auto', autoConfig: autoConfigWithSession() });
            }
          }}>
            <TabsList className="h-8">
              {CONVERSATION_RUN_MODE_ORDER.map((mode) => (
                <TabsTrigger value={mode} className="px-3 text-xs" key={mode}>
                  {t(`conversation.home.${mode}`)}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>

          {showRunModeManagement ? (
            <Button variant="ghost" size="sm" className="ml-auto h-7 gap-1 text-xs" onClick={onOpenRunModeSettings}>
              <Workflow className="size-3" />
              {t('conversation.home.configureNow')}
            </Button>
          ) : null}
        </div>

        {isDirect ? (
          <div className="flex min-h-14 items-center gap-2 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
            <span className="mr-1 text-xs font-medium text-muted-foreground">{t('conversation.home.selectAgent')}</span>
            <TooltipProvider>
              <Tabs value={selectedDirectAgent} onValueChange={selectDirectAgent}>
                <TabsList className="h-10 gap-1 bg-transparent p-0">
                  {agentOptions.map(({ agent, selectable, reason }) => (
                    <Tooltip key={agent.agentType}>
                      <TooltipTrigger asChild>
                        <span>
                          <TabsTrigger
                            value={agent.agentType}
                            disabled={!selectable}
                            className="h-10 min-w-10 gap-2 rounded-full border border-transparent px-2.5 data-[state=active]:border-primary/25 data-[state=active]:bg-primary/10"
                          >
                            <img
                              src={agentIconSrc(agent.iconKey)}
                              alt=""
                              className={agentIconClass(agent.iconKey, 'size-5')}
                            />
                            {selectedDirectAgent === agent.agentType ? (
                              <span className="max-w-36 truncate text-xs">{agent.displayName}</span>
                            ) : null}
                          </TabsTrigger>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent>{reason || agent.displayName}</TooltipContent>
                    </Tooltip>
                  ))}
                </TabsList>
              </Tabs>
            </TooltipProvider>
            {agentOptions.length === 0 ? (
              <DirectAgentEmptyState onOpenAgentManagement={onOpenAgentManagement} />
            ) : null}
          </div>
        ) : isAuto ? (
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
                const nextGlobalGoal = event.target.value;
                setGlobalGoal(nextGlobalGoal);
                updateAutoSession({ globalGoal: optionalRunModeText(nextGlobalGoal) });
              }}
            />
          </div>
        ) : (
          <div className="flex items-center gap-3 rounded-xl border border-border/50 bg-card/40 px-4 py-3">
            <Workflow className="size-4 text-muted-foreground" />
            <Select value={workflowTemplateId} onValueChange={(id) => { setWorkflowTemplateId(id); onRunModeChange({ mode: 'workflow', workflowTemplateId: id, includeInterview: runMode.includeInterview }); }}>
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
            {showInterviewToggle ? (
              <label className="flex shrink-0 items-center gap-1.5">
                <span className="text-xs text-muted-foreground">{t('conversation.home.includeInterview')}</span>
                <Switch
                  checked={runMode.includeInterview ?? true}
                  onCheckedChange={(checked) => onRunModeChange({ ...runMode, includeInterview: checked })}
                />
              </label>
            ) : null}
            <Button variant="ghost" size="sm" className="h-7 gap-1 text-xs" onClick={onOpenRunModeSettings}>
              <Workflow className="size-3" />
              {t('conversation.home.configureWorkflow')}
            </Button>
          </div>
        )}
        {runModeError ? (
          <div className="flex items-start gap-3 whitespace-pre-wrap rounded-xl border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive">
            <span className="min-w-0 flex-1">{runModeError}</span>
            {showRunModeManagement ? (
              <Button variant="outline" size="sm" className="h-7 shrink-0 border-destructive/30 bg-background/40 px-2 text-xs text-destructive hover:text-destructive" onClick={onOpenRunModeSettings}>
                <Workflow className="mr-1 size-3" />
                {t('conversation.runtime.repairAction')}
              </Button>
            ) : null}
          </div>
        ) : null}
      </div>

      <AttachmentPreviewDialogs
        previewImage={previewImage}
        textPreview={textPreview}
        onCloseImage={() => setPreviewImage(null)}
        onCloseText={() => setTextPreview(null)}
      />
    </>
  );
}

export function DirectAgentEmptyState({
  onOpenAgentManagement,
}: {
  onOpenAgentManagement: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-0 items-center gap-2">
      <span className="truncate text-xs text-muted-foreground">{t('conversation.home.noAgent')}</span>
      <Button
        type="button"
        variant="outline"
        size="icon"
        className="size-7 shrink-0 rounded-full border-border/60 bg-background/30"
        aria-label={t('agentManagement.addAgent')}
        title={t('agentManagement.addAgent')}
        onClick={onOpenAgentManagement}
      >
        <Plus className="size-3.5" />
      </Button>
    </div>
  );
}
