import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { displayAppError } from '@/i18n';
import { Send, Paperclip, Workflow, Route, Bot, Folders, Plus, ChevronDown, Settings2, AlarmClock, X, Laptop, GitFork, Check, Loader2 } from 'lucide-react';
import type { AgentRegistryVm, ConversationAutoConfigVm, ConversationCreateInput, ConversationDirectConfigVm, ConversationRunModeVm, ConversationWorkLocation, ConversationWorkspaceVm, ProfileVm, WorkflowRepairTarget, WorkflowTemplateStore } from '../../types';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { canOpenRunModeManagement, CONVERSATION_RUN_MODE_ORDER, directConfigForAgent, includeOptionalEntryForSubmit, normalizeConversationAutoConfigForSubmit, normalizeConversationDirectConfigForSubmit, optionalRunModeText, setOptionalEntryPreference, shouldShowOptionalEntryToggle } from '@/lib/conversation-run-mode-config';
import { groupSelectableAgentOptions, normalizeConfigOptionOverrides, selectableAgentOptions, type SelectableAgentOption, validateAutoConfig, validateDirectConfig, validateWorkflowTemplateForConversationStartWithFreshProfiles, workflowRepairTargetForTemplate } from '@/lib/run-mode-validation';
import { useAttachmentPicker, useWindowDragGuard } from '@/lib/attachment-service';
import { ComposerContextArea } from '@/components/shared/ComposerContextArea';
import { useConversationComposerDraft } from '@/lib/conversation-composer-draft';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import { useAgentCommands } from '@/hooks/useAgentCommands';
import { useSlashCommandController } from '@/hooks/useSlashCommandController';
import { SlashCommandMenu } from '@/components/conversation/SlashCommandMenu';
import { SlashCommandInputTag } from '@/components/conversation/SlashCommandInputTag';
import {
  AcpModelThoughtSelects,
  findAcpThoughtLevel,
  updateAcpConfigOptionOverride,
} from '@/components/acp/AcpModelThoughtSelects';
import { AcpSingleConfigMenu } from '@/components/acp/AcpSingleConfigMenu';
import { parseCommittedSlashCommand, restoreSlashCommandInputFocus } from '@/lib/slash-command';
import { useLeadingAdornmentTextIndent } from '@/hooks/useLeadingAdornmentTextIndent';
import { ScheduledTaskDialog } from '@/components/conversation/ScheduledTaskDialog';
import type { ScheduledScheduleInput } from '@/types';
import { validateScheduledConversationInput } from '@/lib/scheduled-task-validation';
import { formatScheduledScheduleInput } from '@/lib/scheduled-task-formatting';
import { PromptInput, PromptInputTextarea } from '@/components/prompt-kit/prompt-input';
import { CONVERSATION_HOME_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';
import { workflowTemplateDisplayName } from '@/lib/workflow-template';
import { useOverflowTooltip } from '@/hooks/useOverflowTooltip';
import { cn } from '@/lib/utils';
import { hasUserPromptPayload } from '@/lib/composer-context';
import { GitBranchSelector } from '@/components/git/GitBranchSelector';
import {
  createDraftAttachmentWorkspaceResource,
  draftAttachmentWorkspaceResourceKey,
  scheduledTaskConfigWorkspaceResourceKey,
  useOptionalRightWorkspace,
  type RightWorkspaceResource,
} from '@/components/workspace/right-workspace-context';

interface ConversationComposerProps {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  profiles: ProfileVm[];
  busy: boolean;
  inlineContentMaxBytes: number;
  initialScheduledMode?: boolean;
  scheduledTaskCreated?: boolean;
  workLocation: ConversationWorkLocation;
  onRunModeChange: (mode: ConversationRunModeVm, projectId: string) => void;
  onLoadProfiles: () => Promise<ProfileVm[]>;
  onSubmit: (input: ConversationCreateInput) => Promise<string | null | undefined> | string | null | undefined;
  onCreateScheduledTask?: (input: ConversationCreateInput & { schedule: ScheduledScheduleInput; overlapPolicy: 'skip_when_running' | 'retry_when_busy'; sessionPolicy?: 'new' | 'continuous' }) => Promise<void>;
  onScheduledTaskCreated?: () => void;
  onOpenAgentManagement: () => void;
  onOpenScheduledTasks: () => void;
  onOpenRunModeSettings: () => void;
  onWorkflowRepairTargetChange?: (target: WorkflowRepairTarget | null) => void;
  onWorkspaceChange: (projectId: string) => void;
  onWorkLocationChange: (location: ConversationWorkLocation, projectId: string) => Promise<void> | void;
  onScheduledModeExit?: () => void;
}

interface ConversationWorkspaceControlProps {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  onWorkspaceChange: (projectId: string) => void;
  variant?: 'toolbar' | 'info';
}

const CONTEXT_CONTROL_INTERACTION_CLASS_NAME = 'bg-transparent text-foreground transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:bg-accent focus-visible:text-accent-foreground data-[state=open]:bg-accent data-[state=open]:text-accent-foreground dark:bg-transparent dark:hover:bg-accent/50 dark:focus-visible:bg-accent/50 dark:data-[state=open]:bg-accent/50';
const CONVERSATION_WORKSPACE_INFO_CURVE_VIEW_BOX = '0 0 48 28';
const CONVERSATION_WORKSPACE_INFO_CURVE_PATH = 'M0 28L20.14 4Q23.497 0 29.497 0H48V28Z';

export function ScheduledTaskCreatedNotice({ onOpenScheduledTasks }: { onOpenScheduledTasks: () => void }) {
  return (
    <div className="px-3 text-xs text-muted-foreground" role="status">
      <Trans i18nKey="scheduled.composer.created" components={{
        tasks: (
          <a
            href="/chat/scheduled-tasks"
            className="rounded-sm font-medium text-link underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
            onClick={(event) => {
              if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
              event.preventDefault();
              onOpenScheduledTasks();
            }}
          />
        ),
      }} />
    </div>
  );
}
export function ConversationWorkspaceControl({
  projectId,
  workspaceName,
  workspaces,
  onWorkspaceChange,
  variant = 'toolbar',
}: ConversationWorkspaceControlProps) {
  const { t } = useTranslation();
  const selectedWorkspaceName = workspaces.find((workspace) => workspace.projectId === projectId)?.name ?? workspaceName;
  const selectTriggerRef = useRef<HTMLButtonElement>(null);
  const selectionUsedPointerRef = useRef(false);
  const [selectOpen, setSelectOpen] = useState(false);
  const {
    valueRef,
    tooltipOpen,
    showTooltipIfOverflowing,
    hideTooltip,
    handleTooltipOpenChange,
  } = useOverflowTooltip<HTMLSpanElement>({ always: variant === 'info' });
  const controlClassName = variant === 'info'
    ? `${CONVERSATION_HOME_COMPOSER_LAYOUT.workspaceControlClassName} flex h-7 w-7 flex-none items-center justify-center gap-1.5 rounded-md border border-transparent px-0 text-sm shadow-none data-[size=default]:h-7 [&>svg:last-child]:hidden @xs/conversation-context:w-fit @xs/conversation-context:max-w-full @xs/conversation-context:flex-initial @xs/conversation-context:justify-between @xs/conversation-context:px-1.5 @xs/conversation-context:[&>svg:last-child]:block`
    : `${CONVERSATION_HOME_COMPOSER_LAYOUT.workspaceControlClassName} flex h-9 items-center gap-2 rounded-full border border-border/50 bg-gold-surface-high/35 px-3 text-sm text-foreground shadow-none`;
  const triggerSurfaceClassName = variant === 'info'
    ? CONTEXT_CONTROL_INTERACTION_CLASS_NAME
    : 'hover:bg-gold-surface-high/55 dark:bg-gold-surface-high/35 dark:hover:bg-gold-surface-high/55';
  const triggerEvents = {
    onPointerEnter: showTooltipIfOverflowing,
    onPointerLeave: hideTooltip,
    onPointerDown: () => {
      selectionUsedPointerRef.current = true;
      hideTooltip();
    },
    onKeyDown: () => {
      selectionUsedPointerRef.current = false;
    },
    onFocus: showTooltipIfOverflowing,
    onBlur: hideTooltip,
  };
  const value = (
    <TooltipTrigger asChild>
      <span className={cn('flex min-w-0 flex-1 items-center', variant === 'info' ? 'gap-1.5' : 'gap-2')}>
        <Folders className={cn('size-3.5 shrink-0', variant === 'info' ? 'text-current' : 'text-muted-foreground/80')} />
        <span
          ref={valueRef}
          data-conversation-workspace-value="true"
          className={cn('min-w-0 truncate', variant === 'info' && 'hidden @xs/conversation-context:inline')}
        >
          {selectedWorkspaceName}
        </span>
      </span>
    </TooltipTrigger>
  );

  return (
    <TooltipProvider>
      <Tooltip open={tooltipOpen && !selectOpen} onOpenChange={handleTooltipOpenChange}>
        {workspaces.length > 1 ? (
          <Select
            value={projectId}
            onValueChange={onWorkspaceChange}
            onOpenChange={(open) => {
              setSelectOpen(open);
              hideTooltip();
            }}
          >
            <SelectTrigger
              ref={selectTriggerRef}
              {...triggerEvents}
              aria-label={`${t('conversation.home.workspace')}: ${selectedWorkspaceName}`}
              data-context-control={variant === 'info' ? 'workspace' : undefined}
              className={`${controlClassName} ${triggerSurfaceClassName} focus-visible:border-primary/30 focus-visible:ring-2 focus-visible:ring-primary/10`}
            >
              {value}
            </SelectTrigger>
            <SelectContent
              position="popper"
              align="start"
              onPointerDownCapture={() => {
                selectionUsedPointerRef.current = true;
              }}
              onKeyDownCapture={() => {
                selectionUsedPointerRef.current = false;
              }}
              onCloseAutoFocus={(event) => {
                if (!selectionUsedPointerRef.current) return;
                event.preventDefault();
                selectTriggerRef.current?.blur();
                selectionUsedPointerRef.current = false;
              }}
            >
              {workspaces.map((workspace) => (
                <SelectItem key={workspace.projectId} value={workspace.projectId}>{workspace.name}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <div
            {...triggerEvents}
            tabIndex={0}
            aria-label={`${t('conversation.home.workspace')}: ${selectedWorkspaceName}`}
            data-context-control={variant === 'info' ? 'workspace' : undefined}
            className={cn(controlClassName, triggerSurfaceClassName, 'focus-visible:border-primary/30 focus-visible:ring-2 focus-visible:ring-primary/10 focus-visible:outline-none')}
          >
            {value}
          </div>
        )}
        <TooltipContent
          side="top"
          sideOffset={6}
          className="max-w-[min(24rem,calc(100vw-2rem))] whitespace-normal break-words"
        >
          {selectedWorkspaceName}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

interface ConversationWorkspaceInfoBarProps extends ConversationWorkspaceControlProps {
  workLocation: ConversationWorkLocation;
  busy: boolean;
  onWorkLocationChange: (location: ConversationWorkLocation, projectId: string) => Promise<void> | void;
  showWorkLocation?: boolean;
  showBranch?: boolean;
  onBranchChange?: (branch: string | null) => void;
  onBranchMutationPendingChange?: (pending: boolean) => void;
}

export function ConversationWorkspaceInfoBar({
  projectId,
  workspaceName,
  workspaces,
  workLocation,
  busy,
  onWorkspaceChange,
  onWorkLocationChange,
  showWorkLocation = true,
  showBranch,
  onBranchChange,
  onBranchMutationPendingChange,
}: ConversationWorkspaceInfoBarProps) {
  const { t } = useTranslation();
  const [checkingLocation, setCheckingLocation] = useState(false);
  const [locationMenuOpen, setLocationMenuOpen] = useState(false);
  const [locationTooltipOpen, setLocationTooltipOpen] = useState(false);
  const locationTriggerRef = useRef<HTMLButtonElement>(null);
  const locationMenuUsedPointerRef = useRef(false);

  const selectLocation = async (location: ConversationWorkLocation) => {
    if (checkingLocation || location === workLocation) return;
    setCheckingLocation(true);
    try {
      await onWorkLocationChange(location, projectId);
    } finally {
      setCheckingLocation(false);
    }
  };

  const LocationIcon = workLocation === 'worktree' ? GitFork : Laptop;
  const locationLabel = workLocation === 'worktree'
    ? t('conversation.home.workLocationWorktree')
    : t('conversation.home.workLocationMain');
  const branchVisible = showBranch ?? showWorkLocation;

  return (
    <TooltipProvider>
      <div
        data-conversation-workspace-info="true"
        className={CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoClassName}
      >
        <div
          aria-hidden="true"
          data-conversation-workspace-info-body="true"
          className="pointer-events-none absolute inset-y-0 left-12 right-12 bg-[var(--conversation-workspace-info-surface)]"
        />
        <svg
          aria-hidden="true"
          focusable="false"
          viewBox={CONVERSATION_WORKSPACE_INFO_CURVE_VIEW_BOX}
          preserveAspectRatio="none"
          data-conversation-workspace-info-curve="left"
          className="pointer-events-none absolute left-0 bottom-0 h-7 w-12 text-[var(--conversation-workspace-info-surface)]"
        >
          <path d={CONVERSATION_WORKSPACE_INFO_CURVE_PATH} fill="currentColor" />
        </svg>
        <svg
          aria-hidden="true"
          focusable="false"
          viewBox={CONVERSATION_WORKSPACE_INFO_CURVE_VIEW_BOX}
          preserveAspectRatio="none"
          data-conversation-workspace-info-curve="right"
          className="pointer-events-none absolute right-0 bottom-0 h-7 w-12 text-[var(--conversation-workspace-info-surface)]"
        >
          <path d={CONVERSATION_WORKSPACE_INFO_CURVE_PATH} fill="currentColor" transform="translate(48 0) scale(-1 1)" />
        </svg>
        <div data-conversation-workspace-info-controls="true" className="relative z-10 flex min-w-0 items-center gap-0">
          <ConversationWorkspaceControl
            projectId={projectId}
            workspaceName={workspaceName}
            workspaces={workspaces}
            onWorkspaceChange={onWorkspaceChange}
            variant="info"
          />
          {showWorkLocation ? (
            <Tooltip
              open={locationTooltipOpen && !locationMenuOpen}
              onOpenChange={(open) => setLocationTooltipOpen(open && !locationMenuOpen)}
            >
              <DropdownMenu onOpenChange={(open) => {
                setLocationMenuOpen(open);
                setLocationTooltipOpen(false);
              }}>
                <TooltipTrigger asChild>
                  <DropdownMenuTrigger asChild>
                    <Button
                      ref={locationTriggerRef}
                      type="button"
                      variant={null}
                      size="sm"
                      disabled={busy || checkingLocation}
                      aria-label={`${t('conversation.home.workLocation')}: ${locationLabel}`}
                      data-conversation-work-location-trigger="true"
                      data-context-control="work-location"
                      onPointerDown={() => {
                        locationMenuUsedPointerRef.current = true;
                        setLocationTooltipOpen(false);
                      }}
                      onKeyDown={() => {
                        locationMenuUsedPointerRef.current = false;
                      }}
                      className={cn('h-7 w-7 min-w-0 shrink-0 gap-0 rounded-md px-0 text-sm font-normal has-[>svg]:px-0 @md/conversation-context:w-auto @md/conversation-context:shrink @md/conversation-context:gap-1.5 @md/conversation-context:px-1.5 @md/conversation-context:has-[>svg]:px-1.5', CONTEXT_CONTROL_INTERACTION_CLASS_NAME)}
                    >
                      {checkingLocation ? <Loader2 className="size-3.5 animate-spin text-current" /> : <LocationIcon className="size-3.5 text-current" />}
                      <span data-conversation-work-location-value="true" className="hidden truncate @md/conversation-context:inline">{locationLabel}</span>
                      <ChevronDown className="hidden size-4 text-muted-foreground opacity-50 @md/conversation-context:block" />
                    </Button>
                  </DropdownMenuTrigger>
                </TooltipTrigger>
                <DropdownMenuContent
                  align="start"
                  className="min-w-56"
                  onPointerDownCapture={() => {
                    locationMenuUsedPointerRef.current = true;
                  }}
                  onKeyDownCapture={() => {
                    locationMenuUsedPointerRef.current = false;
                  }}
                  onCloseAutoFocus={(event) => {
                    if (!locationMenuUsedPointerRef.current) return;
                    event.preventDefault();
                    locationTriggerRef.current?.blur();
                    locationMenuUsedPointerRef.current = false;
                  }}
                >
                  <div className="px-2 py-1.5 text-xs font-medium text-muted-foreground">
                    {t('conversation.home.workLocation')}
                  </div>
                  <DropdownMenuItem onSelect={() => { void selectLocation('main'); }}>
                    <Laptop className="size-4" />
                    <span>{t('conversation.home.workLocationMain')}</span>
                    {workLocation === 'main' ? <Check className="ml-auto size-4" /> : null}
                  </DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => { void selectLocation('worktree'); }}>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="flex min-w-0 flex-1 items-center gap-2">
                          <GitFork className="size-4" />
                          <span>{t('conversation.home.workLocationNewWorktree')}</span>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent side="right">{t('conversation.home.worktreeTip')}</TooltipContent>
                    </Tooltip>
                    {workLocation === 'worktree' ? <Check className="ml-auto size-4" /> : null}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
              <TooltipContent side="top" sideOffset={6}>
                {t('conversation.home.workLocation')}: {locationLabel}
              </TooltipContent>
            </Tooltip>
          ) : null}
          {branchVisible ? (
            <GitBranchSelector
              projectId={projectId}
              disabled={busy || checkingLocation}
              responsiveContext
              onBranchChange={onBranchChange}
              onMutationPendingChange={onBranchMutationPendingChange}
            />
          ) : null}
        </div>
      </div>
    </TooltipProvider>
  );
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
  inlineContentMaxBytes,
  initialScheduledMode = false,
  scheduledTaskCreated = false,
  workLocation,
  onRunModeChange,
  onLoadProfiles,
  onSubmit,
  onCreateScheduledTask,
  onScheduledTaskCreated,
  onOpenAgentManagement,
  onOpenScheduledTasks,
  onOpenRunModeSettings,
  onWorkflowRepairTargetChange,
  onWorkspaceChange,
  onWorkLocationChange,
  onScheduledModeExit,
}: ConversationComposerProps) {
  const { t } = useTranslation();
  const composerDraft = useConversationComposerDraft();
  const content = composerDraft.draft.content;
  const setContent = composerDraft.setContent;
  const scheduledMode = composerDraft.draft.submission.kind === 'scheduled-task';
  const scheduledConfig = composerDraft.draft.submission.kind === 'scheduled-task'
    ? composerDraft.draft.submission.config
    : null;
  const [selectedDirectAgent, setSelectedDirectAgent] = useState(runMode.directConfig?.agentType ?? '');
  const [selectedDirectModel, setSelectedDirectModel] = useState(runMode.directConfig?.modelId ?? '');
  const [selectedDirectPermissionMode, setSelectedDirectPermissionMode] = useState(runMode.directConfig?.permissionMode ?? '');
  const [selectedDirectConfigOptions, setSelectedDirectConfigOptions] = useState<Record<string, string>>(runMode.directConfig?.configOptions ?? {});
  const [selectedAgent, setSelectedAgent] = useState(runMode.autoConfig?.agentType ?? '');
  const [selectedModel, setSelectedModel] = useState(runMode.autoConfig?.modelId ?? '');
  const [selectedPermissionMode, setSelectedPermissionMode] = useState(runMode.autoConfig?.permissionMode ?? '');
  const [selectedConfigOptions, setSelectedConfigOptions] = useState<Record<string, string>>(runMode.autoConfig?.configOptions ?? {});
  const [globalGoal, setGlobalGoal] = useState(runMode.autoConfig?.globalGoal ?? '');
  const [workflowTemplateId, setWorkflowTemplateId] = useState(runMode.workflowTemplateId ?? '');
  const [runModeError, setRunModeError] = useState<string | null>(null);
  const [submittingAttachments, setSubmittingAttachments] = useState(false);
  const [branchSelection, setBranchSelection] = useState<{ projectId: string; branch: string } | null>(null);
  const [branchMutationPending, setBranchMutationPending] = useState(false);
  const previousInitialScheduledModeRef = useRef(initialScheduledMode);
  const initialScheduledModeOpenedRef = useRef(false);
  const rightWorkspace = useOptionalRightWorkspace();
  const composerTextareaRef = useRef<HTMLTextAreaElement>(null);
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
    handlePaste,
  } = useAttachmentPicker({
    attachments: [composerDraft.draft.attachments, composerDraft.setAttachments],
    inlineContentMaxBytes,
  });

  const openComposerAttachment = useCallback((attachment: import('@/lib/attachment-service').AttachmentItem) => {
    if (!rightWorkspace?.scopeKey) return;
    void rightWorkspace.openResource(createDraftAttachmentWorkspaceResource({
      scopeKey: rightWorkspace.scopeKey,
      projectId,
      attachment,
    }));
  }, [projectId, rightWorkspace]);

  const closeComposerAttachmentPreview = useCallback((attachment: import('@/lib/attachment-service').AttachmentItem) => {
    if (!rightWorkspace?.scopeKey) return;
    void rightWorkspace.closeTab(draftAttachmentWorkspaceResourceKey(rightWorkspace.scopeKey, attachment.id));
  }, [rightWorkspace]);

  const removeComposerAttachment = useCallback((id: string) => {
    const attachment = attachments.find((item) => item.id === id);
    if (attachment) closeComposerAttachmentPreview(attachment);
    removeAttachment(id);
  }, [attachments, closeComposerAttachmentPreview, removeAttachment]);

  const clearComposerAttachments = useCallback(() => {
    attachments.forEach(closeComposerAttachmentPreview);
    clearAttachments();
  }, [attachments, clearAttachments, closeComposerAttachmentPreview]);

  const handleBranchChange = useCallback((branch: string | null) => {
    setBranchSelection((current) => {
      if (!branch) return current === null ? current : null;
      if (current?.projectId === projectId && current.branch === branch) return current;
      return { projectId, branch };
    });
  }, [projectId]);

  useWindowDragGuard();

  const isAuto = runMode.mode === 'auto';
  const isDirect = runMode.mode === 'direct';
  const showRunModeManagement = canOpenRunModeManagement(runMode.mode);
  const autoStrategy = runMode.autoConfig?.agentStrategy ?? 'fixed';
  const isDynamicAuto = autoStrategy === 'dynamic';
  const scheduledSummary = scheduledConfig
    ? formatScheduledScheduleInput(t, scheduledConfig.schedule)
    : t('scheduled.composer.unconfigured');
  const canSubmit = hasUserPromptPayload(content, attachments.length)
    && !busy
    && !submittingAttachments
    && !branchMutationPending;
  const canCreateScheduledTask = canSubmit && Boolean(onCreateScheduledTask);
  const scheduledConfigResourceKey = rightWorkspace?.scopeKey
    ? scheduledTaskConfigWorkspaceResourceKey(rightWorkspace.scopeKey)
    : null;

  const closeScheduledConfig = useCallback(() => {
    if (scheduledConfigResourceKey) void rightWorkspace?.closeTab(scheduledConfigResourceKey);
  }, [rightWorkspace?.closeTab, scheduledConfigResourceKey]);

  const openScheduledConfig = useCallback(() => {
    if (!rightWorkspace?.scopeKey || !scheduledConfigResourceKey) return;
    composerDraft.enterScheduledTask();
    void rightWorkspace.openResource({
      kind: 'scheduled-task-config',
      key: scheduledConfigResourceKey,
      scopeKey: rightWorkspace.scopeKey,
      title: t('scheduled.dialog.title'),
      description: t('scheduled.composer.configure'),
      attention: false,
    });
  }, [composerDraft.enterScheduledTask, rightWorkspace, scheduledConfigResourceKey, t]);

  const exitScheduledMode = useCallback(() => {
    composerDraft.exitScheduledTask();
    closeScheduledConfig();
    onScheduledModeExit?.();
  }, [closeScheduledConfig, composerDraft.exitScheduledTask, onScheduledModeExit]);

  useEffect(() => {
    const wasInitiallyScheduled = previousInitialScheduledModeRef.current;
    previousInitialScheduledModeRef.current = initialScheduledMode;
    if (!initialScheduledMode) {
      initialScheduledModeOpenedRef.current = false;
      if (wasInitiallyScheduled) {
        composerDraft.exitScheduledTask();
        closeScheduledConfig();
      }
      return;
    }
    composerDraft.enterScheduledTask();
    if (initialScheduledModeOpenedRef.current || !rightWorkspace?.scopeKey) return;
    initialScheduledModeOpenedRef.current = true;
    openScheduledConfig();
  }, [closeScheduledConfig, composerDraft.enterScheduledTask, composerDraft.exitScheduledTask, initialScheduledMode, openScheduledConfig, rightWorkspace?.scopeKey]);

  const renderScheduledConfig = useCallback((resource: RightWorkspaceResource) => (
    resource.kind === 'scheduled-task-config' ? (
      <ScheduledTaskDialog
        allowContinuous={isDirect}
        open
        presentation="workspace"
        onOpenChange={(open) => { if (!open) closeScheduledConfig(); }}
        draftConfig={scheduledConfig}
        onSave={async (config) => { composerDraft.setScheduledTaskConfig(config); }}
      />
    ) : null
  ), [closeScheduledConfig, composerDraft.setScheduledTaskConfig, isDirect, scheduledConfig]);

  useEffect(() => {
    if (!rightWorkspace) return;
    return rightWorkspace.registerResourceRenderer('scheduled-task-config', renderScheduledConfig);
  }, [renderScheduledConfig, rightWorkspace?.registerResourceRenderer]);

  const agentOptions = useMemo(() => selectableAgentOptions(agentRegistry, t), [agentRegistry, t]);
  const directAgentGroups = useMemo(() => groupSelectableAgentOptions(agentOptions), [agentOptions]);
  const agents = useMemo(
    () => agentOptions.filter((item) => item.selectable).map((item) => item.agent),
    [agentOptions],
  );
  const selectedAgentObj = agents.find((a) => a.agentType === selectedAgent);
  const selectedDirectAgentObj = agents.find((agent) => agent.agentType === selectedDirectAgent);
  const directModels = selectedDirectAgentObj?.supportedModels ?? [];
  const directPermissionModes = selectedDirectAgentObj?.supportedModes ?? [];
  const directThoughtLevel = findAcpThoughtLevel(selectedDirectAgentObj?.configOptions);
  const models = selectedAgentObj?.supportedModels ?? [];
  const permissionModes = selectedAgentObj?.supportedModes ?? [];
  const autoPermissionModes = permissionModes;
  const thoughtLevel = findAcpThoughtLevel(selectedAgentObj?.configOptions);
  const templates = workflowTemplates?.templates ?? [];
  const selectedWorkflowTemplateId = workflowTemplateId || runMode.workflowTemplateId || undefined;
  const selectedWorkflowTemplate = templates.find((template) => template.id === selectedWorkflowTemplateId);
  const showOptionalEntryToggle = shouldShowOptionalEntryToggle(runMode.mode, selectedWorkflowTemplate);
  const includeOptionalEntry = includeOptionalEntryForSubmit(runMode, selectedWorkflowTemplate);
  const renderDirectAgentOption = ({ agent, selectable, reason }: SelectableAgentOption) => (
    <Tooltip key={agent.agentType}>
      <TooltipTrigger asChild>
        <span>
          <TabsTrigger
            value={agent.agentType}
            disabled={!selectable}
            className={CONVERSATION_HOME_COMPOSER_LAYOUT.agentOptionClassName}
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
  );
  const workspacePath = workspaces.find((workspace) => workspace.projectId === projectId)?.workspacePath;
  const commandAgentType = isDirect
    ? selectedDirectAgent
    : isAuto && !isDynamicAuto
      ? selectedAgent
      : null;
  const agentCommands = useAgentCommands(commandAgentType, workspacePath);
  const restoreComposerFocus = useCallback(() => {
    restoreSlashCommandInputFocus(composerTextareaRef);
  }, []);
  const slashCommands = useSlashCommandController({
    input: content,
    commands: agentCommands.commands,
    contextKey: agentCommands.catalogKey,
    onInputChange: setContent,
    onInputFocusRequested: restoreComposerFocus,
  });
  const committedSlashCommand = useMemo(
    () => parseCommittedSlashCommand(content, agentCommands.commands),
    [agentCommands.commands, content],
  );
  const visibleContent = committedSlashCommand?.suffix ?? content;
  const committedInputLayout = useLeadingAdornmentTextIndent(Boolean(committedSlashCommand));

  useEffect(() => {
    const fallbackAgent = runMode.directConfig?.agentType
      || agents[0]?.agentType
      || '';
    const directConfig = fallbackAgent ? directConfigForAgent(runMode, fallbackAgent) : undefined;
    setSelectedDirectAgent(fallbackAgent);
    setSelectedDirectModel(directConfig?.modelId ?? '');
    setSelectedDirectPermissionMode(directConfig?.permissionMode ?? '');
    setSelectedDirectConfigOptions(directConfig?.configOptions ?? {});
    setSelectedAgent(runMode.autoConfig?.agentType ?? '');
    setSelectedModel(runMode.autoConfig?.modelId ?? '');
    setSelectedPermissionMode(runMode.autoConfig?.permissionMode ?? '');
    setSelectedConfigOptions(runMode.autoConfig?.configOptions ?? {});
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
    }, projectId);
  };

  const selectDirectAgent = (agentType: string) => {
    const remembered = directConfigForAgent(runMode, agentType);
    setSelectedDirectAgent(agentType);
    setSelectedDirectModel(remembered.modelId ?? '');
    setSelectedDirectPermissionMode(remembered.permissionMode ?? '');
    setSelectedDirectConfigOptions(remembered.configOptions ?? {});
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
    const nextConfigOptions = patchedValue(patch, 'configOptions', selectedConfigOptions);
    const nextGlobalGoal = patchedValue(patch, 'globalGoal', globalGoal);
    if (isDynamicAuto) {
      return {
        ...base,
        agentStrategy: 'dynamic',
        agentType: base.agentType || base.bootstrapAgentType || nextAgent || '',
        ...patch,
        configOptions: undefined,
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
      configOptions: nextConfigOptions,
      globalGoal: optionalRunModeText(nextGlobalGoal),
    };
  };

  const updateAutoSession = (patch: Partial<ConversationAutoConfigVm>) => {
    onRunModeChange({ mode: 'auto', autoConfig: autoConfigWithSession(patch) }, projectId);
  };

  useEffect(() => {
    if (!isDirect || !selectedDirectAgentObj) return;
    const normalized = normalizeConfigOptionOverrides(selectedDirectAgentObj, selectedDirectConfigOptions);
    if (normalized.removedOptionIds.length === 0) return;
    setSelectedDirectConfigOptions(normalized.configOptions);
    updateDirectConfig({
      agentType: selectedDirectAgent,
      modelId: selectedDirectModel || undefined,
      permissionMode: selectedDirectPermissionMode || undefined,
      configOptions: normalized.configOptions,
    });
  }, [isDirect, selectedDirectAgentObj, selectedDirectAgent, selectedDirectModel, selectedDirectPermissionMode, selectedDirectConfigOptions]);

  useEffect(() => {
    if (!isAuto || isDynamicAuto || !selectedAgentObj) return;
    const normalized = normalizeConfigOptionOverrides(selectedAgentObj, selectedConfigOptions);
    if (normalized.removedOptionIds.length === 0) return;
    setSelectedConfigOptions(normalized.configOptions);
    updateAutoSession({ configOptions: normalized.configOptions });
  }, [isAuto, isDynamicAuto, selectedAgentObj, selectedConfigOptions]);

  const handleSubmit = async () => {
    if (!canSubmit) return;
    const trimmed = content.trim();
    const inputBase: ConversationCreateInput = {
      projectId,
      content: trimmed,
      runMode: runMode.mode,
      workflowTemplateId: isAuto || isDirect ? undefined : selectedWorkflowTemplateId,
      includeOptionalEntry,
      directConfig: isDirect
        ? normalizeConversationDirectConfigForSubmit({
          agentType: selectedDirectAgent,
          modelId: selectedDirectModel || undefined,
          permissionMode: selectedDirectPermissionMode || undefined,
          configOptions: selectedDirectAgentObj
            ? normalizeConfigOptionOverrides(selectedDirectAgentObj, selectedDirectConfigOptions).configOptions
            : selectedDirectConfigOptions,
        })
        : undefined,
      autoConfig: isAuto
        ? normalizeConversationAutoConfigForSubmit(autoConfigWithSession(
          !isDynamicAuto && selectedAgentObj
            ? { configOptions: normalizeConfigOptionOverrides(selectedAgentObj, selectedConfigOptions).configOptions }
            : {},
        ))
        : undefined,
      workLocation,
      selectedBranch: workLocation === 'worktree' && branchSelection?.projectId === projectId
        ? branchSelection.branch
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
        if (!isDirect && !isAuto) {
          onWorkflowRepairTargetChange?.(workflowRepairTargetForTemplate(
            inputBase.workflowTemplateId,
            agentRegistry,
            profiles,
            workflowTemplates,
            t,
          ));
        }
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
      attachments.forEach(closeComposerAttachmentPreview);
      composerDraft.reset();
    } catch {
      // Attachment hook owns the user-facing file error.
    } finally {
      setSubmittingAttachments(false);
    }
  };

  const scheduledConversationInput = () => ({
    projectId,
    content: content.trim(),
    runMode: runMode.mode,
    workflowTemplateId: isAuto || isDirect ? undefined : selectedWorkflowTemplateId,
    includeOptionalEntry,
    directConfig: isDirect ? normalizeConversationDirectConfigForSubmit({ agentType: selectedDirectAgent, modelId: selectedDirectModel || undefined, permissionMode: selectedDirectPermissionMode || undefined, configOptions: selectedDirectConfigOptions }) : undefined,
    autoConfig: isAuto ? normalizeConversationAutoConfigForSubmit(autoConfigWithSession()) : undefined,
  });

  const createScheduledTask = async () => {
    if (!canCreateScheduledTask || !onCreateScheduledTask) return;
    if (!scheduledConfig) {
      openScheduledConfig();
      return;
    }
    const inputBase = scheduledConversationInput();
    const localIssues = await validateScheduledConversationInput(inputBase, {
      agentRegistry,
      workflowTemplates,
      profiles,
      loadProfiles: onLoadProfiles,
      t,
    });
    if (localIssues.length > 0) {
      setRunModeError(localIssues.join('\n'));
      return;
    }
    setSubmittingAttachments(true);
    try {
      const paths = await resolveAttachmentPaths();
      await onCreateScheduledTask({ ...inputBase, ...scheduledConfig, attachmentPaths: paths.length ? paths : undefined });
      attachments.forEach(closeComposerAttachmentPreview);
      composerDraft.reset();
      onScheduledTaskCreated?.();
      exitScheduledMode();
      setRunModeError(null);
    } catch (error) {
      setRunModeError(displayAppError(t, error));
    } finally {
      setSubmittingAttachments(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (slashCommands.onKeyDown(e as React.KeyboardEvent<HTMLTextAreaElement>)) return;
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      void (scheduledMode ? createScheduledTask() : handleSubmit());
    }
  };

  return (
    <>
      <div
        data-conversation-composer="quick"
        data-attachment-dropzone="true"
        className={CONVERSATION_HOME_COMPOSER_LAYOUT.containerClassName}
        {...dropZoneHandlers}
      >
        {scheduledMode ? (
          <div className="flex min-h-8 items-center gap-2 px-2 text-xs text-muted-foreground">
            <AlarmClock className="size-4 text-foreground" />
            <span className="truncate"><strong className="font-medium text-foreground">{scheduledSummary}</strong> · {t('scheduled.composer.creating')}</span>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" className="ml-auto size-7 rounded-md" aria-label={t('scheduled.composer.exit')} onClick={exitScheduledMode}><X className="size-3.5" /></Button>
              </TooltipTrigger>
              <TooltipContent>{t('scheduled.composer.exit')}</TooltipContent>
            </Tooltip>
          </div>
        ) : null}
        {/* Main text input */}
        <div className={CONVERSATION_HOME_COMPOSER_LAYOUT.attachedInfoRailClassName}>
          <ConversationWorkspaceInfoBar
            projectId={projectId}
            workspaceName={workspaceName}
            workspaces={workspaces}
            workLocation={workLocation}
            busy={busy || submittingAttachments}
            onWorkspaceChange={onWorkspaceChange}
            onWorkLocationChange={onWorkLocationChange}
            showWorkLocation={!scheduledMode}
            showBranch={!scheduledMode}
            onBranchChange={handleBranchChange}
            onBranchMutationPendingChange={setBranchMutationPending}
          />
          <PromptInput
          value={visibleContent}
          onValueChange={(value) => setContent(`${committedSlashCommand?.prefix ?? ''}${value}`)}
          maxHeight={CONVERSATION_HOME_COMPOSER_LAYOUT.textareaMaxHeightPx}
          onSubmit={() => { void handleSubmit(); }}
          disabled={busy || submittingAttachments || branchMutationPending}
          className={cn(
            CONVERSATION_HOME_COMPOSER_LAYOUT.promptInputClassName,
            slashCommands.isOpen && 'z-50',
          )}
        >
          <ComposerContextArea
            attachments={attachments}
            onRemoveAttachment={removeComposerAttachment}
            onPreviewAttachment={openComposerAttachment}
          />
          <SlashCommandMenu
            open={slashCommands.isOpen}
            commands={slashCommands.filteredCommands}
            activeIndex={slashCommands.activeIndex}
            onActiveIndexChange={slashCommands.setActiveIndex}
            onDismiss={slashCommands.dismiss}
            onSelect={(index) => { slashCommands.selectByIndex(index); }}
            variant="inline"
          >
            <div className="relative min-w-0">
              {committedSlashCommand ? (
                <span ref={committedInputLayout.adornmentRef} className="absolute left-0 top-2 z-10 inline-flex">
                  <SlashCommandInputTag
                    prefix={committedSlashCommand.prefix}
                    description={committedSlashCommand.command.description}
                  />
                </span>
              ) : null}
              <PromptInputTextarea
                ref={composerTextareaRef}
                style={committedInputLayout.textareaStyle}
                className={CONVERSATION_HOME_COMPOSER_LAYOUT.textareaClassName}
                placeholder={t('conversation.home.inputPlaceholder')}
                onKeyDown={handleKeyDown}
                onPaste={(e) => { void handlePaste(e); }}
                onDragEnter={dropZoneHandlers.onDragEnter}
                onDragOver={dropZoneHandlers.onDragOver}
                onDrop={dropZoneHandlers.onDrop}
                disabled={busy || submittingAttachments}
              />
            </div>
          </SlashCommandMenu>
          <div
            data-slot="conversation-composer-toolbar"
            data-layout={isDirect ? 'configured' : 'simple'}
            className={`${CONVERSATION_HOME_COMPOSER_LAYOUT.toolbarClassName} ${isDirect ? CONVERSATION_HOME_COMPOSER_LAYOUT.configuredToolbarClassName : CONVERSATION_HOME_COMPOSER_LAYOUT.simpleToolbarClassName}`}
          >
            <div
              data-slot="conversation-composer-leading-actions"
              className={CONVERSATION_HOME_COMPOSER_LAYOUT.leadingActionsClassName}
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
                className="size-7 rounded-full"
                onClick={() => { void pickFiles(); }}
                disabled={busy || submittingAttachments}
                aria-label={t('acp.attachHint')}
              >
                <Paperclip className="size-3.5" />
              </Button>
            </div>
            <div
              data-slot="conversation-composer-trailing-actions"
              className={isDirect ? CONVERSATION_HOME_COMPOSER_LAYOUT.configuredTrailingActionsClassName : CONVERSATION_HOME_COMPOSER_LAYOUT.simpleTrailingActionsClassName}
            >
              {isDirect ? (
                <>
                  <AcpModelThoughtSelects
                    models={directModels}
                    modelValue={selectedDirectModel}
                    thoughtLevel={directThoughtLevel}
                    thoughtValue={directThoughtLevel ? selectedDirectConfigOptions[directThoughtLevel.id] : null}
                    triggerClassName={CONVERSATION_HOME_COMPOSER_LAYOUT.configTriggerClassName}
                    onModelChange={(value) => {
                      const modelId = value ?? '';
                      setSelectedDirectModel(modelId);
                      updateDirectConfig({
                        agentType: selectedDirectAgent,
                        modelId: modelId || undefined,
                        permissionMode: selectedDirectPermissionMode || undefined,
                        configOptions: selectedDirectConfigOptions,
                      });
                    }}
                    onThoughtChange={(optionId, value) => {
                      const next = updateAcpConfigOptionOverride(selectedDirectConfigOptions, optionId, value);
                      setSelectedDirectConfigOptions(next);
                      updateDirectConfig({
                        agentType: selectedDirectAgent,
                        modelId: selectedDirectModel || undefined,
                        permissionMode: selectedDirectPermissionMode || undefined,
                        configOptions: next,
                      });
                    }}
                  />
                  <AcpSingleConfigMenu
                    label={t('acp.permissionMode')}
                    value={selectedDirectPermissionMode}
                    options={directPermissionModes}
                    unspecifiedLabel={t('workflowEditor.permissionModeUnspecified')}
                    align="end"
                    triggerClassName={CONVERSATION_HOME_COMPOSER_LAYOUT.configTriggerClassName}
                    onValueChange={(value) => {
                      const permissionMode = value ?? '';
                      setSelectedDirectPermissionMode(permissionMode);
                      updateDirectConfig({
                        agentType: selectedDirectAgent,
                        modelId: selectedDirectModel || undefined,
                        permissionMode: permissionMode || undefined,
                        configOptions: selectedDirectConfigOptions,
                      });
                    }}
                  />
                </>
              ) : null}
              {scheduledMode ? (
                <div className={`${CONVERSATION_HOME_COMPOSER_LAYOUT.submitActionsClassName} items-center gap-1`}>
                  <div className="flex min-w-0 overflow-hidden rounded-full bg-primary text-primary-foreground">
                    <Button size="sm" className="h-8 min-w-0 rounded-none px-3 shadow-none" disabled={!canCreateScheduledTask} onClick={() => void createScheduledTask()}><AlarmClock className="size-3.5" />{t('scheduled.composer.create')}</Button>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild><Button size="sm" className="h-8 w-6 rounded-none px-0 shadow-none" disabled={busy || submittingAttachments || !onCreateScheduledTask} aria-label={t('scheduled.composer.moreSendOptions')}><ChevronDown className="size-2.5" /></Button></DropdownMenuTrigger>
                      <DropdownMenuContent align="end">
                        <DropdownMenuItem onSelect={exitScheduledMode}>
                          <span className="text-xs leading-5 text-muted-foreground">{t('scheduled.composer.switchTo')}</span>
                          <Send className="size-3.5" />
                          <span>{t('acp.send')}</span>
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button variant="ghost" size="icon-sm" className="rounded-full" aria-label={t('scheduled.composer.configure')} onClick={openScheduledConfig}><Settings2 className="size-3.5" /></Button>
                    </TooltipTrigger>
                    <TooltipContent>{t('scheduled.composer.configure')}</TooltipContent>
                  </Tooltip>
                </div>
              ) : (
                <div className={`${CONVERSATION_HOME_COMPOSER_LAYOUT.submitActionsClassName} overflow-hidden rounded-full bg-primary text-primary-foreground`}>
                  <Button size="sm" className={`${CONVERSATION_HOME_COMPOSER_LAYOUT.sendButtonClassName} min-w-0 flex-1 rounded-none shadow-none`} disabled={!canSubmit} onClick={() => { void handleSubmit(); }}><Send className="size-3.5" />{t('acp.send')}</Button>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild><Button size="sm" className="h-8 w-6 rounded-none px-0 shadow-none" disabled={busy || submittingAttachments || !onCreateScheduledTask} aria-label={t('scheduled.composer.moreSendOptions')}><ChevronDown className="size-2.5" /></Button></DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem onSelect={openScheduledConfig}>
                        <span className="text-xs leading-5 text-muted-foreground">{t('scheduled.composer.switchTo')}</span>
                        <AlarmClock className="size-3.5" />
                        <span>{t('scheduled.composer.create')}</span>
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              )}
            </div>
          </div>
        </PromptInput>
        </div>

        {/* File error */}
        {fileError ? (
          <div className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">{fileError}</div>
        ) : null}

        {scheduledTaskCreated ? <ScheduledTaskCreatedNotice onOpenScheduledTasks={onOpenScheduledTasks} /> : null}

        {/* Run mode selector */}
        <div className={CONVERSATION_HOME_COMPOSER_LAYOUT.optionSectionClassName}>
          <span className="text-xs font-medium text-muted-foreground">{t('conversation.home.runMode')}</span>
          <Tabs value={runMode.mode} onValueChange={(value) => {
            if (value === 'direct') {
              const agentType = selectedDirectAgent || agents[0]?.agentType || '';
              if (agentType) {
                const config = directConfigForAgent(runMode, agentType);
                selectDirectAgent(config.agentType);
              } else {
                onRunModeChange({ mode: 'direct', directPreferences: runMode.directPreferences }, projectId);
              }
            } else if (value === 'workflow') {
              onRunModeChange({ mode: 'workflow', workflowTemplateId: workflowTemplateId || runMode.workflowTemplateId, optionalEntryPreferences: runMode.optionalEntryPreferences }, projectId);
            } else {
              onRunModeChange({ mode: 'auto', autoConfig: autoConfigWithSession() }, projectId);
            }
          }}>
            <TabsList className={CONVERSATION_HOME_COMPOSER_LAYOUT.optionTabsListClassName}>
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
          <div className={CONVERSATION_HOME_COMPOSER_LAYOUT.agentSectionClassName}>
            <span className="mr-1 text-xs font-medium text-muted-foreground">{t('conversation.home.selectAgent')}</span>
            <TooltipProvider>
              <Tabs value={selectedDirectAgent} onValueChange={selectDirectAgent} className={CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsClassName}>
                <TabsList variant="bare" className={CONVERSATION_HOME_COMPOSER_LAYOUT.agentTabsListClassName}>
                  {directAgentGroups.selectable.map(renderDirectAgentOption)}
                  {directAgentGroups.selectable.length > 0 && directAgentGroups.unavailable.length > 0 ? (
                    <span
                      aria-orientation="vertical"
                      className="mx-1 h-6 w-px shrink-0 bg-border/70"
                      role="separator"
                    />
                  ) : null}
                  {directAgentGroups.unavailable.map(renderDirectAgentOption)}
                </TabsList>
              </Tabs>
            </TooltipProvider>
            {agentOptions.length === 0 ? (
              <DirectAgentEmptyState onOpenAgentManagement={onOpenAgentManagement} />
            ) : null}
          </div>
        ) : isAuto ? (
          <div className={CONVERSATION_HOME_COMPOSER_LAYOUT.autoSectionClassName}>
            <div className="flex items-center gap-3">
              <Bot className="size-4 text-muted-foreground" />
              <div className="flex min-w-0 flex-1 flex-wrap items-center gap-3">
                {isDynamicAuto ? (
                  <div className={`${CONVERSATION_HOME_COMPOSER_LAYOUT.modeControlHeightClassName} flex min-w-0 items-center rounded-md border border-border/60 bg-background/40 px-3 text-xs text-muted-foreground`}>
                    <span className="truncate">{t('conversation.home.dynamicAgent')}</span>
                  </div>
                ) : (
                  <Select value={selectedAgent} onValueChange={(v) => { setSelectedAgent(v); setSelectedModel(''); setSelectedConfigOptions({}); updateAutoSession({ agentType: v, modelId: undefined, configOptions: {} }); }}>
                    <SelectTrigger className={`${CONVERSATION_HOME_COMPOSER_LAYOUT.modeControlHeightClassName} w-[180px] min-w-0 text-xs`}>
                      <SelectValue placeholder={t('conversation.home.selectAgent')} />
                    </SelectTrigger>
                    <SelectContent position="popper" align="start">
                      {agentOptions.map(({ agent: a, selectable, reason }) => (
                        <SelectItem key={a.agentType} value={a.agentType} disabled={!selectable}>
                          <span className="block min-w-0">
                            <span className="block truncate">{a.displayName}</span>
                            {!selectable && reason ? <span className="mt-0.5 block whitespace-normal text-ui-caption text-destructive">{reason}</span> : null}
                          </span>
                        </SelectItem>
                      ))}
                      {agentOptions.length === 0 ? (
                        <div className="px-2 py-3 text-xs text-muted-foreground">{t('conversation.home.noAgent')}</div>
                      ) : null}
                    </SelectContent>
                  </Select>
                )}
                {!isDynamicAuto && selectedAgentObj ? (
                  <AcpModelThoughtSelects
                    models={models}
                    modelValue={selectedModel}
                    thoughtLevel={thoughtLevel}
                    thoughtValue={thoughtLevel ? selectedConfigOptions[thoughtLevel.id] : null}
                    align="start"
                    triggerClassName={CONVERSATION_HOME_COMPOSER_LAYOUT.modeControlHeightClassName}
                    onModelChange={(value) => {
                      const modelId = value ?? '';
                      setSelectedModel(modelId);
                      updateAutoSession({ modelId: modelId || undefined });
                    }}
                    onThoughtChange={(optionId, value) => {
                      const next = updateAcpConfigOptionOverride(selectedConfigOptions, optionId, value);
                      setSelectedConfigOptions(next);
                      updateAutoSession({ configOptions: next });
                    }}
                  />
                ) : null}
                {!isDynamicAuto ? (
                  <AcpSingleConfigMenu
                    label={t('acp.permissionMode')}
                    value={selectedPermissionMode}
                    options={autoPermissionModes}
                    unspecifiedLabel={t('workflowEditor.permissionModeUnspecified')}
                    triggerClassName={CONVERSATION_HOME_COMPOSER_LAYOUT.modeControlHeightClassName}
                    onValueChange={(value) => {
                      const next = value ?? '';
                      setSelectedPermissionMode(next);
                      updateAutoSession({ permissionMode: next || undefined });
                    }}
                  />
                ) : null}
                <Button variant="ghost" size="sm" className="h-7 gap-1 text-xs" onClick={onOpenRunModeSettings}>
                  <Workflow className="size-3" />
                  {t('conversation.home.configureAuto')}
                </Button>
              </div>
            </div>
            <textarea
              className={CONVERSATION_HOME_COMPOSER_LAYOUT.autoGoalClassName}
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
          <div data-conversation-workflow-selector="true" className={CONVERSATION_HOME_COMPOSER_LAYOUT.workflowSectionClassName}>
            <Route className="size-4 text-muted-foreground" />
            <Select value={workflowTemplateId} onValueChange={(id) => { setWorkflowTemplateId(id); onRunModeChange({ mode: 'workflow', workflowTemplateId: id, optionalEntryPreferences: runMode.optionalEntryPreferences }, projectId); }}>
              <SelectTrigger className={`${CONVERSATION_HOME_COMPOSER_LAYOUT.modeControlHeightClassName} min-w-0 flex-1 text-xs`}>
                <SelectValue placeholder={t('conversation.home.selectWorkflowTemplate')} />
              </SelectTrigger>
              <SelectContent position="popper" align="start">
                {templates.map((tpl) => (
                  <SelectItem key={tpl.id} value={tpl.id}>{workflowTemplateDisplayName(tpl, t)}</SelectItem>
                ))}
                {templates.length === 0 ? (
                  <div className="px-2 py-3 text-xs text-muted-foreground">{t('conversation.home.noWorkflowTemplate')}</div>
                ) : null}
              </SelectContent>
            </Select>
            {showOptionalEntryToggle && selectedWorkflowTemplate?.optionalEntryStage ? (
              <label className="flex shrink-0 items-center gap-1.5">
                <span className="text-xs text-muted-foreground">{t(selectedWorkflowTemplate.optionalEntryStage.labelKey)}</span>
                <Switch
                  checked={includeOptionalEntry}
                  onCheckedChange={(checked) => onRunModeChange(setOptionalEntryPreference(runMode, selectedWorkflowTemplate.id, checked), projectId)}
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
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="size-7 shrink-0 rounded-full border-border/60 bg-background/30"
            aria-label={t('agentManagement.addAgent')}
            onClick={onOpenAgentManagement}
          >
            <Plus className="size-3.5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>{t('agentManagement.addAgent')}</TooltipContent>
      </Tooltip>
    </div>
  );
}
