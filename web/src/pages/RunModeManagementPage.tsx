import { useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Bot, ChevronDown, CircleHelp, Folders, Plus, Route, Trash2 } from 'lucide-react';
import type { AgentRegistryVm, AutoTemplate, ConversationAutoConfigVm, ConversationRunModeVm, ConversationWorkspaceVm, DynamicAgentRefDsl, DynamicControlDsl, ProfileVm, WorkflowDsl, WorkflowModelBindings, WorkflowRepairTarget, WorkflowTemplate, WorkflowTemplateStore } from '../types';
import { deleteAutoTemplate as deleteAutoTemplateApi, deleteWorkflowTemplate, getAutoTemplates, getProfiles, replaceAutoTemplates, saveAutoTemplate, saveWorkflowTemplate, updateAutoTemplate, updateWorkflowTemplate } from '@/api';
import { Page, PageHeader } from '@/components/PageScaffold';
import {
  AcpModelThoughtSelects,
  findAcpThoughtLevel,
  updateAcpConfigOptionOverride,
} from '@/components/acp/AcpModelThoughtSelects';
import { AcpSingleConfigMenu } from '@/components/acp/AcpSingleConfigMenu';
import { WorkflowEditor, validateWorkflowForSave } from '@/components/WorkflowEditor';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Separator } from '@/components/ui/separator';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { displayAppError } from '@/i18n';
import { pruneMissingAutoConfigReferences, pruneMissingAutoAllowedProfileIds, pruneMissingAutoAllowedWorkflowIds, selectableAgentOptions, selectableWorkflowOptions, validateAutoConfig } from '@/lib/run-mode-validation';
import { createBlankWorkflowDraft, hasWorkflowBindingDraftChanges, hasWorkflowDraftChanges, restoreBuiltInWorkflowDefinition, shouldShowDefaultWorkflowSaveAsNotice, workflowTemplateDisplayName } from '@/lib/workflow-template';
import { cn } from '@/lib/utils';

interface RunModeManagementPageProps {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  repairTarget?: WorkflowRepairTarget | null;
  onProjectChange: (projectId: string) => void;
  onSave: (mode: ConversationRunModeVm) => void | Promise<void>;
  onWorkflowTemplatesChange?: (store: WorkflowTemplateStore) => void;
}

type RunModeManagementTab = 'auto' | 'workflow';

export function createBlankWorkflowTemplateEditorState() {
  const workflow = createBlankWorkflowDraft();
  return {
    templateId: null as string | null,
    workflow,
    saveName: '',
  };
}

export function RunModeTabsToolbar({
  mode,
  onModeChange,
  workflowLabel,
  autoLabel,
}: {
  mode: RunModeManagementTab;
  onModeChange: (mode: RunModeManagementTab) => void;
  workflowLabel: string;
  autoLabel: string;
}) {
  return (
    <div data-testid="run-mode-tabs-toolbar" className="flex flex-wrap items-center gap-3">
      <Tabs value={mode} onValueChange={(value) => onModeChange(value as RunModeManagementTab)}>
        <TabsList className="grid w-fit grid-cols-2">
          <TabsTrigger value="workflow">{workflowLabel}</TabsTrigger>
          <TabsTrigger value="auto">{autoLabel}</TabsTrigger>
        </TabsList>
      </Tabs>
    </div>
  );
}

export function TemplateActionRow({
  label,
  picker,
  auxiliaryAction,
  notice,
  showSaveCurrent = true,
  saveCurrentDisabled = false,
  saveAsDisabled = false,
  saving,
  saveCurrentLabel,
  savingLabel,
  onSaveCurrent,
  restoreAction,
  name,
  namePlaceholder,
  onNameChange,
  saveAsLabel,
  onSaveAs,
}: {
  label: ReactNode;
  picker: ReactNode;
  auxiliaryAction?: ReactNode;
  notice?: ReactNode;
  showSaveCurrent?: boolean;
  saveCurrentDisabled?: boolean;
  saveAsDisabled?: boolean;
  saving: boolean;
  saveCurrentLabel: string;
  savingLabel: string;
  onSaveCurrent: () => void;
  restoreAction?: ReactNode;
  name: string;
  namePlaceholder: string;
  onNameChange: (value: string) => void;
  saveAsLabel: string;
  onSaveAs: () => void;
}) {
  return (
    <div data-testid="template-action-row" className="space-y-1.5">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-xs font-medium text-muted-foreground">{label}</span>
        {picker}
        {auxiliaryAction}
        {showSaveCurrent ? (
          <Button size="sm" disabled={saving || saveCurrentDisabled} onClick={onSaveCurrent}>
            {saving ? savingLabel : saveCurrentLabel}
          </Button>
        ) : null}
        {restoreAction}
        <Input className="h-8 w-40" disabled={saving} value={name} placeholder={namePlaceholder} onChange={(event) => onNameChange(event.target.value)} />
        <Button size="sm" disabled={!name.trim() || saving || saveAsDisabled} onClick={onSaveAs}>
          {saveAsLabel}
        </Button>
      </div>
      {notice ? (
        <p data-testid="template-action-notice" role="status" className="text-xs text-destructive">
          {notice}
        </p>
      ) : null}
    </div>
  );
}

export function RunModeProjectSelector({
  projectId,
  workspaceName,
  workspaces,
  label,
  onProjectChange,
}: {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  label: string;
  onProjectChange: (projectId: string) => void;
}) {
  const selectedWorkspace = workspaces.find((workspace) => workspace.projectId === projectId) ?? null;
  const selectedLabel = selectedWorkspace?.name ?? workspaceName;

  return (
    <div data-testid="run-mode-project-selector" className="flex min-w-0 flex-wrap items-center gap-2">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      {workspaces.length > 1 ? (
        <Select value={projectId} onValueChange={onProjectChange}>
          <SelectTrigger className="h-9 min-w-[220px] max-w-[320px] gap-2">
            <Folders className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="truncate">{selectedLabel}</span>
          </SelectTrigger>
          <SelectContent align="end">
            {workspaces.map((workspace) => (
              <SelectItem key={workspace.projectId} value={workspace.projectId}>
                <span className="block min-w-0">
                  <span className="block truncate">{workspace.name}</span>
                  <span className="mt-0.5 block truncate text-ui-caption text-muted-foreground">{workspace.workspacePath}</span>
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <div className="flex h-9 min-w-[220px] max-w-[320px] items-center gap-2 rounded-md border border-input bg-background px-3 text-sm">
          <Folders className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="truncate">{selectedLabel}</span>
        </div>
      )}
    </div>
  );
}

const AUTO_TEMPLATE_STORAGE_KEY = 'gold-band-auto-mode-templates';

const DEFAULT_DYNAMIC_CONTROL: DynamicControlDsl = {
  maxDynamicNodes: 20,
  maxFanout: 5,
  maxDepth: 6,
  maxParallel: 3,
  maxGroupDepth: 1,
  maxWorkflowInvocations: 10,
  allowNestedDynamic: false,
};

export function createBlankAutoTemplateEditorState(): ConversationAutoConfigVm {
  return {
    agentStrategy: 'fixed',
    agentType: '',
    bootstrapAgentType: '',
    bootstrapModelId: '',
    permissionMode: '',
    bootstrapConfigOptions: {},
    acceptanceModelId: '',
    acceptanceConfigOptions: {},
    modelId: '',
    configOptions: {},
    availableAgents: [],
    routingPrompt: '',
    allowedWorkflows: [],
    allowedProfiles: [],
    control: { ...DEFAULT_DYNAMIC_CONTROL },
    activeTemplateId: '',
    activeTemplateName: '',
  };
}

export function autoSaveTarget(activeTemplateId: string | null | undefined): 'template' | 'run-mode' {
  return activeTemplateId ? 'template' : 'run-mode';
}

export function autoNoticeAutoDismiss(tone: 'success' | 'error' | 'warning'): boolean {
  return tone !== 'error';
}

export function autoNoticeDismissDelay(tone: 'success' | 'error' | 'warning'): number {
  return tone === 'warning' ? 5000 : 3000;
}

export function findSavedWorkflowTemplate(store: WorkflowTemplateStore, name: string): WorkflowTemplate | null {
  return store.templates.find((template) => template.name === name) ?? null;
}

export const templatePickerSavedListClass = 'max-h-64 overflow-auto p-1';

export function isNoAutoTemplateSelected(activeTemplateId: string | null | undefined, isDraft: boolean): boolean {
  return !activeTemplateId && !isDraft;
}

export function RunModeManagementPage({
  projectId,
  workspaceName,
  workspaces,
  runMode,
  agentRegistry,
  workflowTemplates,
  repairTarget = null,
  onProjectChange,
  onSave,
  onWorkflowTemplatesChange,
}: RunModeManagementPageProps) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<RunModeManagementTab>(runMode.mode === 'auto' ? 'auto' : 'workflow');
  const [agentStrategy, setAgentStrategy] = useState<'fixed' | 'dynamic'>(runMode.autoConfig?.agentStrategy ?? 'fixed');
  const [agent, setAgent] = useState(runMode.autoConfig?.agentType ?? '');
  const [bootstrapAgent, setBootstrapAgent] = useState(runMode.autoConfig?.bootstrapAgentType ?? runMode.autoConfig?.agentType ?? '');
  const [bootstrapModel, setBootstrapModel] = useState(runMode.autoConfig?.bootstrapModelId ?? '');
  const [permissionMode, setPermissionMode] = useState(runMode.autoConfig?.permissionMode ?? '');
  const [bootstrapConfigOptions, setBootstrapConfigOptions] = useState<Record<string, string>>(runMode.autoConfig?.bootstrapConfigOptions ?? {});
  const [acceptanceModel, setAcceptanceModel] = useState(runMode.autoConfig?.acceptanceModelId ?? '');
  const [acceptanceConfigOptions, setAcceptanceConfigOptions] = useState<Record<string, string>>(runMode.autoConfig?.acceptanceConfigOptions ?? {});
  const [model, setModel] = useState(runMode.autoConfig?.modelId ?? '');
  const [configOptions, setConfigOptions] = useState<Record<string, string>>(runMode.autoConfig?.configOptions ?? {});
  const [availableAgents, setAvailableAgents] = useState<DynamicAgentRefDsl[]>(runMode.autoConfig?.availableAgents ?? []);
  const [routingPrompt, setRoutingPrompt] = useState(runMode.autoConfig?.routingPrompt ?? '');
  const [allowedWorkflowIds, setAllowedWorkflowIds] = useState((runMode.autoConfig?.allowedWorkflows ?? []).map((item) => item.workflowId));
  const [allowedProfiles, setAllowedProfiles] = useState(runMode.autoConfig?.allowedProfiles ?? []);
  const [control, setControl] = useState<DynamicControlDsl>({ ...DEFAULT_DYNAMIC_CONTROL, ...(runMode.autoConfig?.control ?? {}) });
  const [workflowTemplateId, setWorkflowTemplateId] = useState(runMode.workflowTemplateId ?? workflowTemplates?.lastUsedTemplateId ?? workflowTemplates?.templates[0]?.id ?? '');
  const [profiles, setProfiles] = useState<ProfileVm[]>([]);
  const [profilesLoaded, setProfilesLoaded] = useState(false);
  const [templates, setTemplates] = useState<AutoTemplate[]>([]);
  const [templateName, setTemplateName] = useState(runMode.autoConfig?.activeTemplateName ?? '');
  const [activeTemplateId, setActiveTemplateId] = useState(runMode.autoConfig?.activeTemplateId ?? '');
  const [isAutoTemplateDraft, setIsAutoTemplateDraft] = useState(false);
  const [autoNotice, setAutoNotice] = useState<{ tone: 'success' | 'error' | 'warning'; message: string } | null>(null);
  const [autoSaving, setAutoSaving] = useState(false);
  const [autoTemplatePickerOpen, setAutoTemplatePickerOpen] = useState(false);
  const autoNoticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Workflow template editor state
  const [wfEditTemplateId, setWfEditTemplateId] = useState<string | null>(null);
  const [wfEditWorkflow, setWfEditWorkflow] = useState<WorkflowDsl | null>(null);
  const [wfEditModelBindings, setWfEditModelBindings] = useState<WorkflowModelBindings>({ definitionRevision: '', bindingRevision: 0, bindings: [] });
  const [wfTemplatePickerOpen, setWfTemplatePickerOpen] = useState(false);
  const [wfSaveName, setWfSaveName] = useState('');
  const [wfDeleteTarget, setWfDeleteTarget] = useState<WorkflowTemplate | null>(null);
  const [wfLastUsedHintDismissed, setWfLastUsedHintDismissed] = useState(false);
  const [wfSaving, setWfSaving] = useState(false);
  const [wfNotice, setWfNotice] = useState<string | null>(null);
  const [wfError, setWfError] = useState<string | null>(null);
  const [wfTemplateStore, setWfTemplateStore] = useState<WorkflowTemplateStore | null>(workflowTemplates);
  const wfEditorInitialized = useRef(false);
  const previousProjectIdRef = useRef(projectId);
  const prunedAutoConfigRef = useRef<string | null>(null);
  const prunedAutoTemplateRef = useRef<string | null>(null);
  const handledRepairTargetRef = useRef<string | null>(null);

  useEffect(() => {
    setWfTemplateStore(workflowTemplates);
  }, [workflowTemplates]);

  const agentOptions = useMemo(() => selectableAgentOptions(agentRegistry, t), [agentRegistry, t]);
  const agents = useMemo(() => agentOptions.filter((item) => item.selectable).map((item) => item.agent), [agentOptions]);
  const effectiveWorkflowTemplates = wfTemplateStore ?? workflowTemplates;
  const workflowTemplateList = effectiveWorkflowTemplates?.templates ?? [];
  const workflowOptions = useMemo(() => selectableWorkflowOptions(effectiveWorkflowTemplates, t), [effectiveWorkflowTemplates, t]);
  const selectedAgent = agents.find((a) => a.agentType === agent) ?? null;
  const fixedModels = selectedAgent?.supportedModels ?? [];
  const fixedThoughtLevel = findAcpThoughtLevel(selectedAgent?.configOptions);
  const selectedBootstrapAgent = agents.find((a) => a.agentType === bootstrapAgent) ?? null;
  const bootstrapModels = selectedBootstrapAgent?.supportedModels ?? [];
  const bootstrapThoughtLevel = findAcpThoughtLevel(selectedBootstrapAgent?.configOptions);
  const availableAgentMap = useMemo(() => new Map(availableAgents.map((item) => [item.provider, item])), [availableAgents]);
  const acceptanceModels = bootstrapModels;
  const acceptanceThoughtLevel = findAcpThoughtLevel(selectedBootstrapAgent?.configOptions);

  useEffect(() => {
    if (!repairTarget) return;
    const repairKey = `${repairTarget.workflowTemplateId}:${repairTarget.nodeId}`;
    if (handledRepairTargetRef.current === repairKey) return;
    const template = workflowTemplateList.find((item) => item.id === repairTarget.workflowTemplateId);
    if (!template || !template.workflow.nodes.some((node) => node.id === repairTarget.nodeId)) return;
    handledRepairTargetRef.current = repairKey;
    setMode('workflow');
    setWorkflowTemplateId(template.id);
    setWfEditTemplateId(template.id);
    setWfEditWorkflow(template.workflow);
    setWfEditModelBindings(template.modelBindings);
    setWfSaveName('');
    setWfNotice(null);
    setWfError(null);
  }, [repairTarget, workflowTemplateList]);

  useEffect(() => {
    const projectChanged = previousProjectIdRef.current !== projectId;
    previousProjectIdRef.current = projectId;

    setMode(runMode.mode === 'auto' ? 'auto' : 'workflow');
    const config = runMode.autoConfig ?? null;
    setAgentStrategy(config?.agentStrategy ?? 'fixed');
    setAgent(config?.agentType ?? '');
    setBootstrapAgent(config?.bootstrapAgentType ?? config?.agentType ?? '');
    setBootstrapModel(config?.bootstrapModelId ?? '');
    setPermissionMode(config?.permissionMode ?? '');
    setBootstrapConfigOptions(config?.bootstrapConfigOptions ?? {});
    setAcceptanceModel(config?.acceptanceModelId ?? '');
    setAcceptanceConfigOptions(config?.acceptanceConfigOptions ?? {});
    setModel(config?.modelId ?? '');
    setConfigOptions(config?.configOptions ?? {});
    setAvailableAgents(config?.availableAgents ?? []);
    setRoutingPrompt(config?.routingPrompt ?? '');
    setAllowedWorkflowIds((config?.allowedWorkflows ?? []).map((item) => item.workflowId));
    setAllowedProfiles(config?.allowedProfiles ?? []);
    setControl({ ...DEFAULT_DYNAMIC_CONTROL, ...(config?.control ?? {}) });
    setActiveTemplateId(config?.activeTemplateId ?? '');
    setTemplateName(config?.activeTemplateName ?? '');
    setIsAutoTemplateDraft(false);

    const nextWorkflowTemplateId = runMode.workflowTemplateId
      ?? effectiveWorkflowTemplates?.lastUsedTemplateId
      ?? workflowTemplateList[0]?.id
      ?? '';
    setWorkflowTemplateId(nextWorkflowTemplateId);
    if (runMode.mode === 'workflow') {
      const selectedTemplate = nextWorkflowTemplateId
        ? effectiveWorkflowTemplates?.templates.find((template) => template.id === nextWorkflowTemplateId) ?? null
        : null;
      setWfEditTemplateId(selectedTemplate?.id ?? null);
      setWfEditWorkflow(selectedTemplate?.workflow ?? null);
      setWfEditModelBindings(selectedTemplate?.modelBindings ?? { definitionRevision: '', bindingRevision: 0, bindings: [] });
      setWfSaveName('');
      setWfLastUsedHintDismissed(selectedTemplate?.id === effectiveWorkflowTemplates?.lastUsedTemplateId);
      wfEditorInitialized.current = Boolean(selectedTemplate);
    } else {
      wfEditorInitialized.current = false;
    }

    if (projectChanged) {
      setAutoNotice(null);
      setWfNotice(null);
      setWfError(null);
    }

  // A template-store refresh must not reset the editor to the stale run-mode
  // selection. The run-mode scope is the owner of the initial selection; the
  // store only supplies the template data for that selection.
  }, [projectId, runMode]);

  function showAutoNotice(notice: { tone: 'success' | 'error' | 'warning'; message: string }) {
    if (autoNoticeTimerRef.current) {
      clearTimeout(autoNoticeTimerRef.current);
      autoNoticeTimerRef.current = null;
    }
    setAutoNotice(notice);
    if (autoNoticeAutoDismiss(notice.tone)) {
      autoNoticeTimerRef.current = setTimeout(() => {
        setAutoNotice(null);
        autoNoticeTimerRef.current = null;
      }, autoNoticeDismissDelay(notice.tone));
    }
  }

  useEffect(() => {
    return () => {
      if (autoNoticeTimerRef.current) {
        clearTimeout(autoNoticeTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    getProfiles()
      .then((result) => {
        if (cancelled) return;
        setProfiles(result.profiles);
        setProfilesLoaded(true);
      })
      .catch(() => {
        // Keep the last known profile set. A failed load is not proof that all
        // persisted profile references are invalid.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    const config = runMode.autoConfig;
    if (!config || !profilesLoaded || !effectiveWorkflowTemplates) return;

    const currentWorkflowIds = (config.allowedWorkflows ?? []).map((item) => item.workflowId);
    const currentProfileIds = config.allowedProfiles ?? [];
    const prunedWorkflows = pruneMissingAutoAllowedWorkflowIds(currentWorkflowIds, effectiveWorkflowTemplates);
    const prunedProfiles = pruneMissingAutoAllowedProfileIds(currentProfileIds, profiles);
    const removedCount = prunedWorkflows.removedWorkflowIds.length + prunedProfiles.removedProfileIds.length;
    if (removedCount === 0) {
      prunedAutoConfigRef.current = null;
      return;
    }

    const pruneKey = `${projectId}:${currentWorkflowIds.join('\u0000')}:${currentProfileIds.join('\u0000')}`;
    if (prunedAutoConfigRef.current === pruneKey) return;
    prunedAutoConfigRef.current = pruneKey;

    const nextConfig = {
      ...config,
      allowedWorkflows: prunedWorkflows.workflowIds.map((workflowId) => ({ workflowId })),
      allowedProfiles: prunedProfiles.profileIds,
    };
    setAllowedWorkflowIds(prunedWorkflows.workflowIds);
    setAllowedProfiles(prunedProfiles.profileIds);
    void Promise.resolve(onSave({ ...runMode, autoConfig: nextConfig }));
    showAutoNotice({
      tone: 'warning',
      message: t('runMode.invalidAutoReferencesRemoved', {
        workflows: prunedWorkflows.removedWorkflowIds.length,
        profiles: prunedProfiles.removedProfileIds.length,
      }),
    });
  }, [effectiveWorkflowTemplates, profiles, profilesLoaded, projectId, runMode, t]);

  useEffect(() => {
    const activeTemplateId = runMode.autoConfig?.activeTemplateId?.trim();
    if (!profilesLoaded || !effectiveWorkflowTemplates || !activeTemplateId) return;
    const activeTemplate = templates.find((template) => template.id === activeTemplateId);
    if (!activeTemplate) return;

    const normalized = pruneMissingAutoConfigReferences(activeTemplate.config, effectiveWorkflowTemplates, profiles);
    if (normalized.removedWorkflowIds.length === 0 && normalized.removedProfileIds.length === 0) {
      prunedAutoTemplateRef.current = null;
      return;
    }

    const pruneKey = `${activeTemplate.id}:${JSON.stringify(activeTemplate.config.allowedWorkflows ?? [])}:${JSON.stringify(activeTemplate.config.allowedProfiles ?? [])}`;
    if (prunedAutoTemplateRef.current === pruneKey) return;
    prunedAutoTemplateRef.current = pruneKey;

    let cancelled = false;
    void updateAutoTemplate(activeTemplate.id, activeTemplate.name, normalized.config)
      .then((store) => {
        if (cancelled) return;
        setTemplates(store.templates);
        showAutoNotice({
          tone: 'warning',
          message: t('runMode.invalidAutoTemplateReferencesRemoved', {
            workflows: normalized.removedWorkflowIds.length,
            profiles: normalized.removedProfileIds.length,
          }),
        });
      })
      .catch((error) => {
        if (!cancelled) showAutoNotice({ tone: 'error', message: displayAppError(t, error) });
      });
    return () => {
      cancelled = true;
    };
  }, [effectiveWorkflowTemplates, profiles, profilesLoaded, runMode.autoConfig?.activeTemplateId, t, templates]);

  useEffect(() => {
    let cancelled = false;
    getAutoTemplates()
      .then(async (store) => {
        const legacyTemplates = loadLegacyAutoTemplates();
        if (store.templates.length === 0 && legacyTemplates.length > 0) {
          const migrated = await replaceAutoTemplates(legacyTemplates);
          clearLegacyAutoTemplates();
          return migrated;
        }
        return store;
      })
      .then((store) => {
        if (!cancelled) setTemplates(store.templates);
      })
      .catch((error) => {
        if (!cancelled) setAutoNotice({ tone: 'error', message: displayAppError(t, error) });
      });
    return () => {
      cancelled = true;
    };
  }, [t]);

  useEffect(() => {
    if (workflowTemplateId || workflowTemplateList.length === 0) return;
    setWorkflowTemplateId(effectiveWorkflowTemplates?.lastUsedTemplateId ?? workflowTemplateList[0]?.id ?? '');
  }, [workflowTemplateId, workflowTemplateList, effectiveWorkflowTemplates?.lastUsedTemplateId]);

  const sessionFields = (): Pick<ConversationAutoConfigVm, 'globalGoal'> => ({
    globalGoal: runMode.autoConfig?.globalGoal || undefined,
  });

  const buildAutoConfig = (templatePatch: Partial<ConversationAutoConfigVm> = {}, includeSessionFields = true): ConversationAutoConfigVm => {
    const preservedSessionFields = includeSessionFields ? sessionFields() : {};
    if (agentStrategy === 'dynamic') {
      return {
        agentStrategy: 'dynamic',
        agentType: bootstrapAgent || agent,
        bootstrapAgentType: bootstrapAgent || agent,
        bootstrapModelId: bootstrapModel || undefined,
        permissionMode: permissionMode || undefined,
        bootstrapConfigOptions,
        acceptanceModelId: acceptanceModel || undefined,
        acceptanceConfigOptions,
        availableAgents,
        routingPrompt: routingPrompt.trim() || undefined,
        allowedWorkflows: allowedWorkflowIds.map((workflowId) => ({ workflowId })),
        allowedProfiles,
        control,
        activeTemplateId: activeTemplateId || undefined,
        activeTemplateName: templateName.trim() || undefined,
        ...preservedSessionFields,
        ...templatePatch,
      };
    }
    return {
      agentStrategy: 'fixed',
      agentType: agent,
      modelId: model || undefined,
      permissionMode: permissionMode || undefined,
      configOptions,
      allowedWorkflows: allowedWorkflowIds.map((workflowId) => ({ workflowId })),
      allowedProfiles,
      control,
      activeTemplateId: activeTemplateId || undefined,
      activeTemplateName: templateName.trim() || undefined,
      ...preservedSessionFields,
      ...templatePatch,
    };
  };

  const persistRunModeSelection = (nextMode: RunModeManagementTab, autoConfig?: ConversationAutoConfigVm, templateId = wfEditTemplateId ?? workflowTemplateId) => {
    const updated: ConversationRunModeVm = {
      ...runMode,
      mode: nextMode,
      workflowTemplateId: templateId || undefined,
      autoConfig: autoConfig ?? buildAutoConfig(),
    };
    void Promise.resolve(onSave(updated));
  };

  const changeMode = (nextMode: RunModeManagementTab) => {
    setMode(nextMode);
    if (nextMode === 'auto') {
      const config = buildAutoConfig();
      const issues = validateAutoConfig(config, agentRegistry, effectiveWorkflowTemplates, t);
      if (issues.length > 0) {
        showAutoNotice({ tone: 'error', message: issues.join('\n') });
        return;
      }
      persistRunModeSelection('auto', config);
      return;
    }
    persistRunModeSelection('workflow');
  };

  const applyAutoConfig = (config: ConversationAutoConfigVm) => {
    setAgentStrategy(config.agentStrategy ?? 'fixed');
    setAgent(config.agentType ?? '');
    setBootstrapAgent(config.bootstrapAgentType ?? config.agentType ?? '');
    setBootstrapModel(config.bootstrapModelId ?? '');
    setPermissionMode(config.permissionMode ?? '');
    setBootstrapConfigOptions(config.bootstrapConfigOptions ?? {});
    setAcceptanceModel(config.acceptanceModelId ?? '');
    setAcceptanceConfigOptions(config.acceptanceConfigOptions ?? {});
    setModel(config.modelId ?? '');
    setConfigOptions(config.configOptions ?? {});
    setAvailableAgents(config.availableAgents ?? []);
    setRoutingPrompt(config.routingPrompt ?? '');
    setAllowedWorkflowIds((config.allowedWorkflows ?? []).map((item) => item.workflowId));
    setAllowedProfiles(config.allowedProfiles ?? []);
    setControl({ ...DEFAULT_DYNAMIC_CONTROL, ...(config.control ?? {}) });
    setActiveTemplateId(config.activeTemplateId ?? '');
    setTemplateName(config.activeTemplateName ?? '');
  };

  const selectAutoTemplate = (templateId: string) => {
    if (autoNoticeTimerRef.current) {
      clearTimeout(autoNoticeTimerRef.current);
      autoNoticeTimerRef.current = null;
    }
    setAutoNotice(null);
    if (templateId === '__none__') {
      const config = buildAutoConfig({ activeTemplateId: undefined, activeTemplateName: undefined });
      setActiveTemplateId('');
      setTemplateName('');
      setIsAutoTemplateDraft(false);
      setAutoTemplatePickerOpen(false);
      persistRunModeSelection('auto', config);
      return;
    }
    const template = templates.find((item) => item.id === templateId);
    if (!template) return;
    const config = {
      ...template.config,
      activeTemplateId: template.id,
      activeTemplateName: template.name,
      ...sessionFields(),
    };
    applyAutoConfig(config);
    setIsAutoTemplateDraft(false);
    setAutoTemplatePickerOpen(false);
    persistRunModeSelection('auto', config);
  };

  const deleteAutoTemplate = async (templateId: string) => {
    const nextStore = await deleteAutoTemplateApi(templateId);
    setTemplates(nextStore.templates);
    if (activeTemplateId === templateId) {
      const config = buildAutoConfig({ activeTemplateId: undefined, activeTemplateName: undefined });
      setActiveTemplateId('');
      setTemplateName('');
      setIsAutoTemplateDraft(false);
      persistRunModeSelection('auto', config);
    }
    showAutoNotice({ tone: 'success', message: t('runMode.autoTemplateDeleted') });
  };

  const startAutoTemplate = () => {
    applyAutoConfig(createBlankAutoTemplateEditorState());
    setIsAutoTemplateDraft(true);
    setAutoTemplatePickerOpen(false);
    setAutoNotice(null);
  };

  // Workflow template editor helpers
  const initWfEditor = () => {
    const preselectedId = workflowTemplateId || effectiveWorkflowTemplates?.lastUsedTemplateId;
    const initialTemplate = preselectedId
      ? effectiveWorkflowTemplates?.templates.find((t) => t.id === preselectedId) ?? effectiveWorkflowTemplates?.templates[0] ?? null
      : effectiveWorkflowTemplates?.templates[0] ?? null;
    const initialWorkflow = initialTemplate?.workflow ?? null;
    setWfEditTemplateId(initialTemplate?.id ?? null);
    setWfEditWorkflow(initialWorkflow);
    setWfEditModelBindings(initialTemplate?.modelBindings ?? { definitionRevision: '', bindingRevision: 0, bindings: [] });
    setWfSaveName('');
    setWfLastUsedHintDismissed(initialTemplate?.id === effectiveWorkflowTemplates?.lastUsedTemplateId);
    setWfNotice(null);
    setWfError(null);
  };

  // Initialize editor on first render when templates are available
  useEffect(() => {
    if (!wfEditorInitialized.current && workflowTemplateList.length > 0 && mode === 'workflow') {
      wfEditorInitialized.current = true;
      initWfEditor();
    }
  }, [workflowTemplateList, mode]);

  const selectWfTemplate = (templateId: string) => {
    const found = effectiveWorkflowTemplates?.templates.find((t) => t.id === templateId);
    if (!found) return;
    setWorkflowTemplateId(found.id);
    setWfEditTemplateId(found.id);
    setWfEditWorkflow(found.workflow);
    setWfEditModelBindings(found.modelBindings);
    setWfSaveName('');
    setWfLastUsedHintDismissed(found.id === effectiveWorkflowTemplates?.lastUsedTemplateId);
    setWfNotice(null);
    setWfError(null);
    persistRunModeSelection('workflow', undefined, found.id);
  };

  const startWfBlank = () => {
    const draft = createBlankWorkflowTemplateEditorState();
    setWfEditTemplateId(draft.templateId);
    setWfEditWorkflow(draft.workflow);
    setWfEditModelBindings({ definitionRevision: '', bindingRevision: 0, bindings: [] });
    setWfSaveName(draft.saveName);
    setWfTemplatePickerOpen(false);
    setWfNotice(null);
    setWfError(null);
  };

  const selectedWfTemplate = effectiveWorkflowTemplates?.templates.find((t) => t.id === wfEditTemplateId) ?? null;
  const wfTemplateLabel = selectedWfTemplate ? workflowTemplateDisplayName(selectedWfTemplate, t) : (wfEditWorkflow ? t('taskList.create.unsavedWorkflowTemplate') : t('taskList.create.workflowTemplatePlaceholder'));
  const wfDefinitionChanged = hasWorkflowDraftChanges(wfEditWorkflow, selectedWfTemplate?.workflow);
  const wfBindingsChanged = hasWorkflowBindingDraftChanges(wfEditModelBindings, selectedWfTemplate?.modelBindings);
  const canUpdateWfTemplate = Boolean(wfEditTemplateId) && (!selectedWfTemplate?.isBuiltIn || !wfDefinitionChanged);
  const wfSaveCurrentDisabled = !canUpdateWfTemplate || (!wfDefinitionChanged && !wfBindingsChanged);
  const showDefaultWorkflowSaveAsNotice = shouldShowDefaultWorkflowSaveAsNotice(
    selectedWfTemplate,
    wfEditWorkflow,
    selectedWfTemplate?.workflow,
  );
  const lastUsedWfTemplate = effectiveWorkflowTemplates?.templates.find((t) => t.id === effectiveWorkflowTemplates?.lastUsedTemplateId) ?? null;
  const showWfLastUsedHint = Boolean(lastUsedWfTemplate && wfEditTemplateId !== lastUsedWfTemplate.id && !wfLastUsedHintDismissed);

  // Validate workflow before saving template
  const applyWorkflowTemplateStore = (store: WorkflowTemplateStore) => {
    setWfTemplateStore(store);
    onWorkflowTemplatesChange?.(store);
  };

  const validateWfForTemplate = (workflow: WorkflowDsl, validateTemplateDuplicateId = true, validateModelBindings = false): WorkflowDsl | null => {
    const supportedAgents = agents.filter((agent) => agent.diagnostic?.available === true);
    const validation = validateWorkflowForSave(workflow, profiles, supportedAgents, t, effectiveWorkflowTemplates ?? null, wfEditTemplateId, selectedWfTemplate ? workflowTemplateDisplayName(selectedWfTemplate, t) : null, validateTemplateDuplicateId, wfEditModelBindings, validateModelBindings);
    if (!validation.valid) {
      setWfError(validation.issues.map((issue) => issue.message).join('\n'));
      return null;
    }
    setWfError(null);
    return validation.sanitizedWorkflow;
  };

  const saveWfAsNew = async () => {
    if (!wfEditWorkflow) {
      setWfError(t('taskList.create.noWorkflowTemplate'));
      return;
    }
    if (!wfSaveName.trim()) {
      setWfError(t('runMode.validationTemplateNameRequired'));
      return;
    }
    if (workflowTemplateList.some((template) => template.name.trim() === wfSaveName.trim())) {
      setWfError(t('runMode.validationTemplateNameDuplicated', { name: wfSaveName.trim() }));
      return;
    }
    const validated = validateWfForTemplate(wfEditWorkflow, false, false);
    if (!validated) return;
    setWfSaving(true);
    try {
      const nextStore = await saveWorkflowTemplate(wfSaveName.trim(), validated, wfEditModelBindings);
      const selected = findSavedWorkflowTemplate(nextStore, wfSaveName.trim());
      if (!selected) {
        throw new Error('Saved workflow template was not returned by the template store.');
      }
      setWorkflowTemplateId(selected.id);
      setWfEditTemplateId(selected.id);
      setWfEditWorkflow(selected.workflow);
      setWfEditModelBindings(selected.modelBindings);
      persistRunModeSelection('workflow', undefined, selected.id);
      applyWorkflowTemplateStore(nextStore);
      setWfSaveName('');
      setWfNotice(t('taskList.create.workflowTemplateSaved'));
      setTimeout(() => setWfNotice(null), 3000);
    } catch (error) {
      setWfError(displayAppError(t, error));
    } finally {
      setWfSaving(false);
    }
  };

  const saveWfCurrent = async () => {
    if (!wfEditWorkflow || !canUpdateWfTemplate) return;
    const validated = validateWfForTemplate(wfEditWorkflow, true, selectedWfTemplate?.isBuiltIn === true);
    if (!validated) return;
    setWfSaving(true);
    try {
      const nextStore = await updateWorkflowTemplate(wfEditTemplateId!, validated, wfEditModelBindings);
      const selected = nextStore.templates.find((t) => t.id === wfEditTemplateId) ?? null;
      applyWorkflowTemplateStore(nextStore);
      setWfEditWorkflow(selected?.workflow ?? wfEditWorkflow);
      if (selected) setWfEditModelBindings(selected.modelBindings);
      setWfNotice(t('taskList.create.workflowTemplateUpdated'));
      setTimeout(() => setWfNotice(null), 3000);
    } catch (error) {
      setWfError(displayAppError(t, error));
    } finally {
      setWfSaving(false);
    }
  };

  const restoreWfDefinition = () => {
    if (!selectedWfTemplate?.isBuiltIn) return;
    const restored = restoreBuiltInWorkflowDefinition(selectedWfTemplate.workflow, wfEditModelBindings);
    setWfEditWorkflow(restored.workflow);
    setWfEditModelBindings(restored.modelBindings);
    setWfError(null);
    setWfNotice(null);
  };

  const deleteWfTemplate = async () => {
    if (!wfDeleteTarget || wfDeleteTarget.isBuiltIn) return;
    setWfSaving(true);
    try {
      const nextStore = await deleteWorkflowTemplate(wfDeleteTarget.id);
      applyWorkflowTemplateStore(nextStore);
      const nextSelected = wfEditTemplateId === wfDeleteTarget.id
        ? nextStore.templates[0] ?? null
        : nextStore.templates.find((t) => t.id === wfEditTemplateId) ?? nextStore.templates[0] ?? null;
      setWfEditTemplateId(nextSelected?.id ?? null);
      setWfEditWorkflow(nextSelected?.workflow ?? null);
      setWfEditModelBindings(nextSelected?.modelBindings ?? { definitionRevision: '', bindingRevision: 0, bindings: [] });
      setWfDeleteTarget(null);
      setWfSaveName('');
      setWfNotice(t('taskList.create.workflowTemplateDeleted'));
      setTimeout(() => setWfNotice(null), 3000);
    } catch {
      // Error surfaced by caller
    } finally {
      setWfSaving(false);
    }
  };

  const saveAsTemplate = async () => {
    const name = templateName.trim();
    if (!name) {
      showAutoNotice({ tone: 'error', message: t('runMode.validationTemplateNameRequired') });
      return;
    }
    if (templates.some((item) => item.name.trim() === name)) {
      showAutoNotice({ tone: 'error', message: t('runMode.validationTemplateNameDuplicated', { name }) });
      return;
    }
    const templateConfig = buildAutoConfig({ activeTemplateId: undefined, activeTemplateName: name }, false);
    const issues = validateAutoConfig(templateConfig, agentRegistry, effectiveWorkflowTemplates, t);
    if (issues.length > 0) {
      showAutoNotice({ tone: 'error', message: issues.join('\n') });
      return;
    }
    setAutoSaving(true);
    try {
      const nextStore = await saveAutoTemplate(name, templateConfig);
      const savedTemplate = nextStore.templates.find((item) => item.name === name) ?? nextStore.templates.at(-1);
      const config = {
        ...templateConfig,
        activeTemplateId: savedTemplate?.id,
        activeTemplateName: savedTemplate?.name ?? name,
        ...sessionFields(),
      };
      setTemplates(nextStore.templates);
      setActiveTemplateId(savedTemplate?.id ?? '');
      setTemplateName(savedTemplate?.name ?? name);
      setIsAutoTemplateDraft(false);
      await Promise.resolve(onSave({ mode: 'auto', autoConfig: config }));
      showAutoNotice({ tone: 'success', message: t('runMode.autoTemplateSaved') });
    } catch (error) {
      showAutoNotice({ tone: 'error', message: displayAppError(t, error) });
    } finally {
      setAutoSaving(false);
    }
  };

  const saveCurrentTemplate = async () => {
    const target = autoSaveTarget(activeTemplateId);
    if (target === 'run-mode') {
      const config = buildAutoConfig({ activeTemplateId: undefined, activeTemplateName: undefined });
      const issues = validateAutoConfig(config, agentRegistry, effectiveWorkflowTemplates, t);
      if (issues.length > 0) {
        showAutoNotice({ tone: 'error', message: issues.join('\n') });
        return;
      }
      setAutoSaving(true);
      try {
        await Promise.resolve(onSave({ mode: 'auto', autoConfig: config }));
        setActiveTemplateId('');
        showAutoNotice({ tone: 'success', message: t('runMode.saved') });
      } finally {
        setAutoSaving(false);
      }
      return;
    }

    const name = templateName.trim() || templates.find((item) => item.id === activeTemplateId)?.name || t('runMode.autoTemplateFallbackName');
    if (templates.some((item) => item.id !== activeTemplateId && item.name.trim() === name)) {
      showAutoNotice({ tone: 'error', message: t('runMode.validationTemplateNameDuplicated', { name }) });
      return;
    }
    const templateConfig = buildAutoConfig({ activeTemplateId, activeTemplateName: name }, false);
    const issues = validateAutoConfig(templateConfig, agentRegistry, effectiveWorkflowTemplates, t);
    if (issues.length > 0) {
      showAutoNotice({ tone: 'error', message: issues.join('\n') });
      return;
    }
    setAutoSaving(true);
    try {
      const nextStore = await updateAutoTemplate(activeTemplateId, name, templateConfig);
      const config = { ...templateConfig, ...sessionFields() };
      setTemplates(nextStore.templates);
      setTemplateName(name);
      await Promise.resolve(onSave({ mode: 'auto', autoConfig: config }));
      showAutoNotice({ tone: 'success', message: t('runMode.autoTemplateSaved') });
    } catch (error) {
      showAutoNotice({ tone: 'error', message: displayAppError(t, error) });
    } finally {
      setAutoSaving(false);
    }
  };

  const toggleAvailableAgent = (agentType: string) => {
    setAvailableAgents((current) => {
      if (current.some((item) => item.provider === agentType)) return current.filter((item) => item.provider !== agentType);
      return [...current, { provider: agentType }];
    });
  };

  const updateAvailableAgentConfig = (agentType: string, patch: Partial<DynamicAgentRefDsl>) => {
    setAvailableAgents((current) => current.map((item) => item.provider === agentType ? { ...item, ...patch } : item));
  };

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        variant="integrated"
        icon={<Route />}
        title={<span className="text-title">{t('runMode.title')}</span>}
      />

      <div className={cn('min-h-0 flex-1 px-6 pt-4', mode === 'workflow' ? 'flex flex-col gap-6 overflow-hidden' : 'space-y-6 overflow-y-auto pb-6')}>
        <div className="flex shrink-0 flex-wrap items-center justify-between gap-3">
          <RunModeTabsToolbar
            mode={mode}
            onModeChange={changeMode}
            workflowLabel={t('runMode.workflowSection')}
            autoLabel={t('runMode.autoSection')}
          />
          <RunModeProjectSelector
            projectId={projectId}
            workspaceName={workspaceName}
            workspaces={workspaces}
            label={t('runMode.project')}
            onProjectChange={onProjectChange}
          />
        </div>

        {mode === 'auto' ? (
          <div className="space-y-6">
            <section className="space-y-3">
              <TemplateActionRow
                label={t('runMode.autoTemplate')}
                picker={(
                  <Popover open={autoTemplatePickerOpen} onOpenChange={setAutoTemplatePickerOpen}>
                    <PopoverTrigger asChild>
                      <Button variant="outline" className="justify-between min-w-[200px]" aria-label={t('runMode.autoTemplate')}>
                        <span className="truncate">{templates.find((item) => item.id === activeTemplateId)?.name ?? (isAutoTemplateDraft ? t('runMode.unsavedAutoTemplate') : t('runMode.noAutoTemplate'))}</span>
                        <ChevronDown className="ml-2 size-4 shrink-0 opacity-50" />
                      </Button>
                    </PopoverTrigger>
                    <PopoverContent className="w-[280px] p-0" align="start">
                      <div className="p-1">
                        <button
                          type="button"
                          className={cn('flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent', isAutoTemplateDraft && 'bg-accent text-accent-foreground')}
                          onClick={startAutoTemplate}
                        >
                          <Plus className="size-3.5" />
                          {t('runMode.newAutoTemplate')}
                        </button>
                      </div>
                      <Separator />
                      <div className="p-1">
                        <button
                          type="button"
                          className={cn('flex w-full items-center rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent', isNoAutoTemplateSelected(activeTemplateId, isAutoTemplateDraft) && 'bg-accent text-accent-foreground')}
                          onClick={() => selectAutoTemplate('__none__')}
                        >
                          {t('runMode.noAutoTemplate')}
                        </button>
                      </div>
                      {templates.length > 0 ? <Separator /> : null}
                      <div className={templatePickerSavedListClass}>
                        {templates.map((template) => {
                          const selected = template.id === activeTemplateId;
                          return (
                            <div key={template.id} className={cn('flex items-center gap-1 rounded-sm p-1', selected && 'bg-accent text-accent-foreground')}>
                              <button
                                type="button"
                                className="min-w-0 flex-1 truncate px-1 py-1 text-left text-xs"
                                onClick={() => selectAutoTemplate(template.id)}
                              >
                                {template.name}
                              </button>
                              <Button
                                variant="ghost"
                                size="icon-xs"
                                className="size-6 shrink-0"
                                aria-label={t('runMode.deleteAutoTemplate', { name: template.name })}
                                onClick={(event) => {
                                  event.stopPropagation();
                                  void deleteAutoTemplate(template.id).catch((error) => {
                                    showAutoNotice({ tone: 'error', message: displayAppError(t, error) });
                                  });
                                }}
                              >
                                <Trash2 className="size-3" />
                              </Button>
                            </div>
                          );
                        })}
                      </div>
                    </PopoverContent>
                  </Popover>
                )}
                saving={autoSaving}
                showSaveCurrent={!isAutoTemplateDraft}
                saveCurrentLabel={t('runMode.saveChanges')}
                savingLabel={t('runMode.saving')}
                onSaveCurrent={() => void saveCurrentTemplate()}
                name={templateName}
                namePlaceholder={t('runMode.autoTemplateName')}
                onNameChange={setTemplateName}
                saveAsLabel={t('runMode.saveAsTemplate')}
                onSaveAs={() => void saveAsTemplate()}
              />
            </section>
            {autoNotice ? (
              autoNotice.tone === 'warning' ? (
                <Alert data-testid="auto-config-pruned-warning" className="border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300">
                  <AlertTriangle />
                  <AlertDescription className="text-current">{autoNotice.message}</AlertDescription>
                </Alert>
              ) : (
                <div className={cn('whitespace-pre-wrap rounded-md border px-3 py-2 text-sm', autoNotice.tone === 'success' ? 'border-emerald-500/20 bg-emerald-500/5 text-emerald-600' : 'border-destructive/30 bg-destructive/5 text-destructive')}>
                  {autoNotice.message}
                </div>
              )
            ) : null}

            <section className="flex flex-wrap gap-2">
              <Field label={<><Bot className="size-3.5" />{t('workflowEditor.dynamicAgentStrategy')}</>} required help={t('workflowEditor.dynamicAgentStrategyHelp')}>
                <Select value={agentStrategy} onValueChange={(value) => {
                  setAgentStrategy(value as 'fixed' | 'dynamic');
                  setModel('');
                  setPermissionMode('');
                  setConfigOptions({});
                  setBootstrapConfigOptions({});
                  setAcceptanceConfigOptions({});
                }}>
                  <SelectTrigger className="h-9 w-[180px]"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="fixed">{t('workflowEditor.dynamicAgentStrategyFixed')}</SelectItem>
                    <SelectItem value="dynamic">{t('workflowEditor.dynamicAgentStrategyDynamic')}</SelectItem>
                  </SelectContent>
                </Select>
              </Field>

              {agentStrategy === 'fixed' ? (
                <Field label={t('runMode.agent')} required help={t('workflowEditor.dynamicFixedAgentHelp')}>
                  <Select value={agent} onValueChange={(value) => { setAgent(value); setModel(''); setPermissionMode(''); setConfigOptions({}); }}>
                    <SelectTrigger className="h-9 w-[180px]"><SelectValue placeholder={t('conversation.home.selectAgent')} /></SelectTrigger>
                    <SelectContent>
                      {agentOptions.map(({ agent: item, selectable, reason }) => (
                        <SelectItem key={item.agentType} value={item.agentType} disabled={!selectable}>
                          <span className="block min-w-0">
                            <span className="block truncate">{item.displayName}</span>
                            {!selectable && reason ? <span className="mt-0.5 block whitespace-normal text-ui-caption text-destructive">{reason}</span> : null}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              ) : (
                <Field label={t('workflowEditor.dynamicBootstrapAgent')} required help={t('workflowEditor.dynamicBootstrapAgentHelp')}>
                  <Select value={bootstrapAgent} onValueChange={(value) => { setBootstrapAgent(value); setBootstrapModel(''); setPermissionMode(''); setBootstrapConfigOptions({}); setAcceptanceModel(''); setAcceptanceConfigOptions({}); }}>
                    <SelectTrigger className="h-9 w-[180px]"><SelectValue placeholder={t('conversation.home.selectAgent')} /></SelectTrigger>
                    <SelectContent>
                      {agentOptions.map(({ agent: item, selectable, reason }) => (
                        <SelectItem key={item.agentType} value={item.agentType} disabled={!selectable}>
                          <span className="block min-w-0">
                            <span className="block truncate">{item.displayName}</span>
                            {!selectable && reason ? <span className="mt-0.5 block whitespace-normal text-ui-caption text-destructive">{reason}</span> : null}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              )}

              {agentStrategy === 'fixed' && selectedAgent && (fixedModels.length > 0 || fixedThoughtLevel || (selectedAgent.supportedModes?.length ?? 0) > 0) ? (
                <Field label={t('runMode.model')} help={t('workflowEditor.dynamicFixedModelHelp')}>
                  <div className="flex flex-wrap gap-2">
                    <AcpModelThoughtSelects
                      models={fixedModels}
                      modelValue={model}
                      thoughtLevel={fixedThoughtLevel}
                      thoughtValue={fixedThoughtLevel ? configOptions[fixedThoughtLevel.id] : null}
                      compact
                      triggerClassName="w-[220px] max-w-none rounded-md"
                      onModelChange={(value) => setModel(value ?? '')}
                      onThoughtChange={(optionId, value) => setConfigOptions((current) => (
                        updateAcpConfigOptionOverride(current, optionId, value)
                      ))}
                    />
                    {(selectedAgent.supportedModes?.length ?? 0) > 0 ? (
                      <AcpSingleConfigMenu
                        label={t('acp.permissionMode')}
                        value={permissionMode}
                        options={selectedAgent.supportedModes ?? []}
                        unspecifiedLabel={t('workflowEditor.permissionModeUnspecified')}
                        compact
                        triggerClassName="w-[220px] rounded-md"
                        onValueChange={(value) => setPermissionMode(value ?? '')}
                      />
                    ) : null}
                  </div>
                </Field>
              ) : null}

              {agentStrategy === 'dynamic' && (bootstrapModels.length > 0 || bootstrapThoughtLevel) ? (
                <Field label={t('workflowEditor.dynamicBootstrapModel')} help={t('workflowEditor.dynamicBootstrapModelHelp')}>
                  <AcpModelThoughtSelects
                    models={bootstrapModels}
                    modelValue={bootstrapModel}
                    thoughtLevel={bootstrapThoughtLevel}
                    thoughtValue={bootstrapThoughtLevel ? bootstrapConfigOptions[bootstrapThoughtLevel.id] : null}
                    compact
                    triggerClassName="w-[220px] max-w-none rounded-md"
                    onModelChange={(value) => setBootstrapModel(value ?? '')}
                    onThoughtChange={(optionId, value) => setBootstrapConfigOptions((current) => (
                      updateAcpConfigOptionOverride(current, optionId, value)
                    ))}
                  />
                </Field>
              ) : null}

              {agentStrategy === 'dynamic' && (acceptanceModels.length > 0 || acceptanceThoughtLevel) ? (
                <Field label={t('workflowEditor.dynamicAcceptanceModel')} help={t('workflowEditor.dynamicAcceptanceModelHelp')}>
                  <AcpModelThoughtSelects
                    models={acceptanceModels}
                    modelValue={acceptanceModel}
                    thoughtLevel={acceptanceThoughtLevel}
                    thoughtValue={acceptanceThoughtLevel ? acceptanceConfigOptions[acceptanceThoughtLevel.id] : null}
                    compact
                    triggerClassName="w-[260px] max-w-none rounded-md"
                    onModelChange={(value) => { setAcceptanceModel(value ?? ''); setAcceptanceConfigOptions({}); }}
                    onThoughtChange={(optionId, value) => setAcceptanceConfigOptions((current) => (
                      updateAcpConfigOptionOverride(current, optionId, value)
                    ))}
                  />
                </Field>
              ) : null}

              {agentStrategy === 'dynamic' && (selectedBootstrapAgent?.supportedModes?.length ?? 0) > 0 ? (
                <Field label={t('workflowEditor.dynamicControlPermission')} help={t('workflowEditor.dynamicControlPermissionHelp')}>
                  <AcpSingleConfigMenu
                    label={t('acp.permissionMode')}
                    value={permissionMode}
                    options={selectedBootstrapAgent?.supportedModes ?? []}
                    unspecifiedLabel={t('workflowEditor.permissionModeUnspecified')}
                    compact
                    triggerClassName="w-[220px] rounded-md"
                    onValueChange={(value) => setPermissionMode(value ?? '')}
                  />
                </Field>
              ) : null}

            </section>

            {agentStrategy === 'dynamic' ? (
              <section className="space-y-3">
                <Field label={t('workflowEditor.dynamicAvailableAgents')} required help={t('workflowEditor.dynamicAvailableAgentsHelp')}>
                  <div className="grid gap-2">
                    {agentOptions.map(({ agent: item, selectable, reason }) => {
                      const selected = availableAgentMap.has(item.agentType);
                      const selectedModel = availableAgentMap.get(item.agentType)?.model ?? '';
                      const selectedPermissionMode = availableAgentMap.get(item.agentType)?.permissionMode ?? '';
                      const thoughtLevel = findAcpThoughtLevel(item.configOptions);
                      return (
                        <div key={item.agentType} className={cn('flex items-center gap-2 rounded-md border border-border/60 bg-background/35 px-3 py-2', !selectable && 'opacity-60')}>
                          <button type="button" disabled={!selectable} className={cn('size-4 rounded border disabled:cursor-not-allowed', selected ? 'border-primary bg-primary' : 'border-border')} onClick={() => toggleAvailableAgent(item.agentType)} aria-label={item.displayName} />
                          <span className="min-w-0 flex-1 text-sm">
                            <span className="block truncate">{item.displayName}</span>
                            {!selectable && reason ? <span className="mt-0.5 block text-xs text-destructive">{reason}</span> : null}
                          </span>
                          {selected && ((item.supportedModels?.length ?? 0) > 0 || thoughtLevel || (item.supportedModes?.length ?? 0) > 0) ? (
                            <div className="flex flex-wrap items-center justify-end gap-2">
                              <AcpModelThoughtSelects
                                models={item.supportedModels ?? []}
                                modelValue={selectedModel}
                                thoughtLevel={thoughtLevel}
                                thoughtValue={thoughtLevel ? availableAgentMap.get(item.agentType)?.configOptions?.[thoughtLevel.id] : null}
                                compact
                                triggerClassName="h-8 w-[260px] max-w-none rounded-md text-xs"
                                onModelChange={(value) => updateAvailableAgentConfig(item.agentType, { model: value || undefined })}
                                onThoughtChange={(optionId, value) => updateAvailableAgentConfig(item.agentType, {
                                  configOptions: updateAcpConfigOptionOverride(availableAgentMap.get(item.agentType)?.configOptions, optionId, value),
                                })}
                              />
                              {(item.supportedModes?.length ?? 0) > 0 ? (
                                <AcpSingleConfigMenu
                                  label={t('acp.permissionMode')}
                                  value={selectedPermissionMode}
                                  options={item.supportedModes ?? []}
                                  unspecifiedLabel={t('workflowEditor.permissionModeUnspecified')}
                                  compact
                                  triggerClassName="h-8 w-[220px] rounded-md text-xs"
                                  onValueChange={(value) => updateAvailableAgentConfig(item.agentType, { permissionMode: value || undefined })}
                                />
                              ) : null}
                            </div>
                          ) : null}
                        </div>
                      );
                    })}
                  </div>
                </Field>
                <Field label={t('workflowEditor.dynamicAgentRoutingPrompt')} help={t('workflowEditor.dynamicAgentRoutingPromptHelp')}>
                  <Textarea className="min-h-20" value={routingPrompt} onChange={(event) => setRoutingPrompt(event.target.value)} placeholder={t('workflowEditor.dynamicAgentRoutingPromptPlaceholder')} />
                </Field>
              </section>
            ) : null}

            <section className="grid gap-4 md:grid-cols-2">
              <Field label={t('workflowEditor.allowedWorkflows')} help={t('workflowEditor.allowedWorkflowsHelp')}>
                <MultiToggle
                  items={workflowOptions.map(({ template, workflowId, selectable, reason }) => ({ id: workflowId || template.id, label: workflowTemplateDisplayName(template, t), selectable, reason }))}
                  selected={allowedWorkflowIds}
                  onChange={setAllowedWorkflowIds}
                  emptyLabel={t('workflowEditor.noWorkflowTemplates')}
                />
              </Field>
              <Field label={t('workflowEditor.allowedProfiles')} help={t('workflowEditor.allowedProfilesHelp')}>
                <MultiToggle
                  items={profiles.map((profile) => ({ id: profile.id, label: profile.name }))}
                  selected={allowedProfiles}
                  onChange={setAllowedProfiles}
                  emptyLabel={t('workflowEditor.noProfiles')}
                />
              </Field>
            </section>

            <section className="grid gap-3 md:grid-cols-3">
              {dynamicControlFields(t).map((item) => (
                <Field key={item.key} label={item.label} required help={item.help}>
                  <Input className="h-9" type="number" min={1} step={1} value={String(control[item.key])} onChange={(event) => setControl((current) => ({ ...current, [item.key]: parsePositiveInt(event.target.value) }))} />
                </Field>
              ))}
            </section>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col gap-4">
            <div className="shrink-0">
              <TemplateActionRow
              label={t('taskList.create.workflowTemplate')}
              picker={(
                <Popover open={wfTemplatePickerOpen} onOpenChange={setWfTemplatePickerOpen}>
                  <PopoverTrigger asChild>
                    <Button variant="outline" className="justify-between min-w-[200px]" aria-label={t('taskList.create.workflowTemplate')}>
                      <span className="truncate">{wfTemplateLabel}</span>
                      <ChevronDown className="ml-2 size-4 shrink-0 opacity-50" />
                    </Button>
                  </PopoverTrigger>
                  <PopoverContent className="w-[280px] p-0" align="start">
                    <div className="p-1">
                      <button
                        type="button"
                        className="flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-xs hover:bg-accent"
                        onClick={startWfBlank}
                      >
                        <Plus className="size-3.5" />
                        {t('taskList.create.newWorkflowTemplate')}
                      </button>
                    </div>
                    {workflowTemplateList.length > 0 ? <Separator /> : null}
                    <div className={templatePickerSavedListClass}>
                      {workflowTemplateList.map((tpl) => {
                        const selected = tpl.id === wfEditTemplateId;
                        const isDefault = tpl.isBuiltIn;
                        return (
                          <div key={tpl.id} className={cn('flex items-center gap-1 rounded-sm p-1', selected && 'bg-accent text-accent-foreground')}>
                            <button
                              type="button"
                              className="min-w-0 flex-1 truncate px-1 py-1 text-left text-xs"
                              onClick={() => { selectWfTemplate(tpl.id); setWfTemplatePickerOpen(false); }}
                            >
                              {workflowTemplateDisplayName(tpl, t)}
                            </button>
                            <Button
                              variant="ghost"
                              size="icon-xs"
                              className="size-6 shrink-0"
                              disabled={isDefault}
                              aria-label={isDefault ? t('taskList.create.defaultWorkflowReadonly') : t('taskList.create.deleteWorkflowTemplate', { name: tpl.name })}
                              onClick={() => { setWfTemplatePickerOpen(false); setWfDeleteTarget(tpl); }}
                            >
                              <Trash2 className="size-3" />
                            </Button>
                          </div>
                        );
                      })}
                    </div>
                  </PopoverContent>
                </Popover>
              )}
              auxiliaryAction={showWfLastUsedHint && lastUsedWfTemplate ? (
                <button
                  type="button"
                  className="rounded-full border border-primary/30 bg-primary/5 px-3 py-1 text-xs text-primary hover:bg-primary/10"
                  onClick={() => selectWfTemplate(lastUsedWfTemplate.id)}
                >
                  {t('taskList.create.selectLastUsedWorkflow', { name: workflowTemplateDisplayName(lastUsedWfTemplate, t) })}
                </button>
              ) : null}
              notice={showDefaultWorkflowSaveAsNotice ? t('taskList.create.defaultWorkflowSaveAsNotice') : null}
              showSaveCurrent={Boolean(wfEditTemplateId)}
              saveCurrentDisabled={!profilesLoaded || wfSaveCurrentDisabled}
              saveAsDisabled={!profilesLoaded}
              saving={wfSaving}
              saveCurrentLabel={t('taskList.create.saveCurrentWorkflow')}
              savingLabel={t('taskList.create.savingWorkflowTemplate')}
              onSaveCurrent={() => void saveWfCurrent()}
              restoreAction={selectedWfTemplate?.isBuiltIn && wfDefinitionChanged ? <Button size="sm" variant="outline" disabled={wfSaving} onClick={restoreWfDefinition}>{t('taskList.create.restoreOtherWorkflowChanges')}</Button> : null}
              name={wfSaveName}
              namePlaceholder={t('taskList.create.workflowTemplateName')}
              onNameChange={setWfSaveName}
              saveAsLabel={t('taskList.create.saveAsWorkflow')}
              onSaveAs={() => void saveWfAsNew()}
              />
            </div>

            {wfNotice ? (
              <div className="rounded-md border border-emerald-500/20 bg-emerald-500/5 px-3 py-2 text-xs text-emerald-600">{wfNotice}</div>
            ) : null}
            {wfError ? (
              <div className="whitespace-pre-wrap rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">{wfError}</div>
            ) : null}

            {/* Embedded workflow editor */}
            <div className="min-h-0 min-w-0 flex-1 overflow-hidden">
              {wfEditWorkflow ? (
                  <WorkflowEditor
                    className="h-full min-h-0"
                    value={wfEditWorkflow}
                    modelBindings={wfEditModelBindings}
                    agentRegistry={agentRegistry}
                    profiles={profiles}
                    profileCatalogReady={profilesLoaded}
                    workflowTemplates={effectiveWorkflowTemplates}
                    currentTemplateId={wfEditTemplateId}
                    currentTemplateName={selectedWfTemplate ? workflowTemplateDisplayName(selectedWfTemplate, t) : null}
                    focusNodeId={repairTarget?.workflowTemplateId === wfEditTemplateId ? repairTarget.nodeId : null}
                    validateModelBindings={selectedWfTemplate?.isBuiltIn === true}
                    showSaveAction={false}
                    allowAiDynamic={true}
                    onChange={setWfEditWorkflow}
                    onModelBindingsChange={setWfEditModelBindings}
                    onSave={async () => {
                      if (canUpdateWfTemplate) await saveWfCurrent();
                      else await saveWfAsNew();
                    }}
                  />
              ) : null}
              {!wfEditWorkflow ? (
                <div className="flex size-full min-h-0 items-center justify-center rounded-xl border border-dashed border-border bg-muted/20 text-sm text-muted-foreground">
                  {workflowTemplateList.length > 0
                    ? t('taskList.create.newWorkflowTemplate')
                    : t('taskList.create.noWorkflowTemplate')}
                </div>
              ) : null}
            </div>
          </div>
        )}
      </div>

      {/* Delete template confirmation dialog */}
      <AlertDialog open={!!wfDeleteTarget} onOpenChange={(open) => { if (!open) setWfDeleteTarget(null); }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('taskList.create.deleteWorkflowTemplateTitle')}</AlertDialogTitle>
            <AlertDialogDescription>
              {t('taskList.create.deleteWorkflowTemplateDescription', { name: wfDeleteTarget?.name ?? '' })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction disabled={wfSaving} onClick={() => { void deleteWfTemplate(); }}>
              {t('taskList.create.deleteWorkflowTemplateAction')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}

function Field({ label, children, required = false, help }: { label: ReactNode; children: ReactNode; required?: boolean; help?: string }) {
  return (
    <div className="grid gap-1.5 text-sm">
      <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
        <span className="inline-flex min-w-0 items-center gap-1.5">{label}</span>
        {required ? <span className="text-destructive">*</span> : null}
        {help ? (
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button type="button" variant="ghost" size="icon-xs" className="size-5 rounded-full" aria-label={help} onClick={(e) => e.preventDefault()}>
                  <CircleHelp className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent side="top" className="max-w-72 whitespace-normal text-xs">{help}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : null}
      </div>
      {children}
    </div>
  );
}

function MultiToggle({ items, selected, onChange, emptyLabel }: { items: Array<{ id: string; label: string; selectable?: boolean; reason?: string }>; selected: string[]; onChange: (selected: string[]) => void; emptyLabel: string }) {
  if (items.length === 0) return <div className="rounded-md border border-border/60 px-3 py-2 text-xs text-muted-foreground">{emptyLabel}</div>;
  const selectedSet = new Set(selected);
  const selectableItems = items.filter((item) => item.selectable ?? true);
  const invalidItems = items.filter((item) => item.selectable === false);
  return (
    <div className="space-y-2">
      <div className="flex flex-wrap gap-2">
        {selectableItems.map((item, index) => {
          const active = selectedSet.has(item.id);
          return (
            <Tooltip key={`${item.id}-${index}`}>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  className={cn('max-w-full rounded-full border px-2.5 py-1 text-xs transition-colors', active ? 'border-primary/40 bg-primary/10 text-primary' : 'border-border/60 bg-background/35 text-muted-foreground hover:text-foreground')}
                  onClick={() => onChange(active ? selected.filter((id) => id !== item.id) : [...selected, item.id])}
                >
                  <span className="block max-w-52 truncate">{item.label}</span>
                </button>
              </TooltipTrigger>
              <TooltipContent className="max-w-[360px] break-all">{item.id}</TooltipContent>
            </Tooltip>
          );
        })}
      </div>
      {invalidItems.length > 0 ? (
        <TooltipProvider>
          <div className="pt-0.5">
            <div className="flex flex-wrap gap-2">
              {invalidItems.map((item, index) => (
                <span key={`${item.id}-${index}`} className="inline-flex max-w-full items-center gap-1.5 rounded-full border border-border/60 bg-background/25 px-2.5 py-1 text-xs text-muted-foreground">
                  <span className="block max-w-44 truncate">{item.label}</span>
                  {item.reason ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="inline-flex size-4 items-center justify-center rounded-full text-destructive" aria-label={item.reason}>
                          <AlertTriangle className="size-3.5" />
                        </span>
                      </TooltipTrigger>
                      <TooltipContent className="max-w-72 whitespace-normal text-xs">
                        {item.reason}
                      </TooltipContent>
                    </Tooltip>
                  ) : null}
                </span>
              ))}
            </div>
          </div>
        </TooltipProvider>
      ) : null}
    </div>
  );
}

function dynamicControlFields(t: (key: string) => string): Array<{ key: Exclude<keyof DynamicControlDsl, 'allowNestedDynamic'>; label: string; help: string }> {
  return [
    { key: 'maxDynamicNodes', label: t('workflowEditor.maxDynamicNodes'), help: t('workflowEditor.maxDynamicNodesHelp') },
    { key: 'maxFanout', label: t('workflowEditor.maxFanout'), help: t('workflowEditor.maxFanoutHelp') },
    { key: 'maxDepth', label: t('workflowEditor.maxDepth'), help: t('workflowEditor.maxDepthHelp') },
    { key: 'maxParallel', label: t('workflowEditor.maxParallel'), help: t('workflowEditor.maxParallelHelp') },
    { key: 'maxGroupDepth', label: t('workflowEditor.maxGroupDepth'), help: t('workflowEditor.maxGroupDepthHelp') },
    { key: 'maxWorkflowInvocations', label: t('workflowEditor.maxWorkflowInvocations'), help: t('workflowEditor.maxWorkflowInvocationsHelp') },
  ];
}

function parsePositiveInt(value: string) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.max(1, Math.trunc(parsed)) : 1;
}

function loadLegacyAutoTemplates(): AutoTemplate[] {
  if (typeof localStorage === 'undefined') return [];
  try {
    const parsed = JSON.parse(localStorage.getItem(AUTO_TEMPLATE_STORAGE_KEY) ?? '[]');
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function clearLegacyAutoTemplates() {
  if (typeof localStorage === 'undefined') return;
  localStorage.removeItem(AUTO_TEMPLATE_STORAGE_KEY);
}
