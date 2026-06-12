import { useEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, Bot, ChevronDown, Plus, Save, Trash2, X } from 'lucide-react';
import type { AgentRegistryVm, AutoTemplate, ConversationAutoConfigVm, ConversationRunModeVm, DynamicAgentRefDsl, DynamicControlDsl, ProfileVm, WorkflowDsl, WorkflowTemplate, WorkflowTemplateStore } from '../types';
import { deleteAutoTemplate as deleteAutoTemplateApi, deleteWorkflowTemplate, getAutoTemplates, getProfiles, replaceAutoTemplates, saveAutoTemplate, saveWorkflowTemplate, updateAutoTemplate, updateWorkflowTemplate } from '@/api';
import { Page, PageHeader } from '@/components/PageScaffold';
import { WorkflowEditor, validateWorkflowForSave } from '@/components/WorkflowEditor';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Separator } from '@/components/ui/separator';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { displayAppError } from '@/i18n';
import { selectableAgentOptions, selectableWorkflowOptions, validateAutoConfig } from '@/lib/run-mode-validation';
import { cn } from '@/lib/utils';

interface RunModeManagementPageProps {
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  onSave: (mode: ConversationRunModeVm) => void;
  onWorkflowTemplatesChange?: (store: WorkflowTemplateStore) => void;
  onBack: () => void;
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

export function RunModeManagementPage({
  runMode,
  agentRegistry,
  workflowTemplates,
  onSave,
  onWorkflowTemplatesChange,
  onBack,
}: RunModeManagementPageProps) {
  const { t } = useTranslation();
  const [mode, setMode] = useState<'auto' | 'workflow'>(runMode.mode);
  const [agentStrategy, setAgentStrategy] = useState<'fixed' | 'dynamic'>(runMode.autoConfig?.agentStrategy ?? 'fixed');
  const [agent, setAgent] = useState(runMode.autoConfig?.agentType ?? '');
  const [bootstrapAgent, setBootstrapAgent] = useState(runMode.autoConfig?.bootstrapAgentType ?? runMode.autoConfig?.agentType ?? '');
  const [bootstrapModel, setBootstrapModel] = useState(runMode.autoConfig?.bootstrapModelId ?? '');
  const [model, setModel] = useState(runMode.autoConfig?.modelId ?? '');
  const [availableAgents, setAvailableAgents] = useState<DynamicAgentRefDsl[]>(runMode.autoConfig?.availableAgents ?? []);
  const [routingPrompt, setRoutingPrompt] = useState(runMode.autoConfig?.routingPrompt ?? '');
  const [allowedWorkflowIds, setAllowedWorkflowIds] = useState((runMode.autoConfig?.allowedWorkflows ?? []).map((item) => item.workflowId));
  const [allowedProfiles, setAllowedProfiles] = useState(runMode.autoConfig?.allowedProfiles ?? []);
  const [control, setControl] = useState<DynamicControlDsl>({ ...DEFAULT_DYNAMIC_CONTROL, ...(runMode.autoConfig?.control ?? {}) });
  const [workflowTemplateId, setWorkflowTemplateId] = useState(runMode.workflowTemplateId ?? workflowTemplates?.lastUsedTemplateId ?? workflowTemplates?.templates[0]?.id ?? '');
  const [profiles, setProfiles] = useState<ProfileVm[]>([]);
  const [templates, setTemplates] = useState<AutoTemplate[]>([]);
  const [templateName, setTemplateName] = useState(runMode.autoConfig?.activeTemplateName ?? '');
  const [activeTemplateId, setActiveTemplateId] = useState(runMode.autoConfig?.activeTemplateId ?? '');
  const [saved, setSaved] = useState(false);
  const [autoNotice, setAutoNotice] = useState<{ tone: 'success' | 'error'; message: string } | null>(null);
  const [autoTemplatePickerOpen, setAutoTemplatePickerOpen] = useState(false);

  // ── Workflow template editor state ──
  const [wfEditTemplateId, setWfEditTemplateId] = useState<string | null>(null);
  const [wfEditWorkflow, setWfEditWorkflow] = useState<WorkflowDsl | null>(null);
  const [wfTemplatePickerOpen, setWfTemplatePickerOpen] = useState(false);
  const [wfSaveName, setWfSaveName] = useState('');
  const [wfDeleteTarget, setWfDeleteTarget] = useState<WorkflowTemplate | null>(null);
  const [wfLastUsedHintDismissed, setWfLastUsedHintDismissed] = useState(false);
  const [wfSaving, setWfSaving] = useState(false);
  const [wfNotice, setWfNotice] = useState<string | null>(null);
  const [wfError, setWfError] = useState<string | null>(null);
  const [wfTemplateStore, setWfTemplateStore] = useState<WorkflowTemplateStore | null>(workflowTemplates);

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
  const selectedBootstrapAgent = agents.find((a) => a.agentType === bootstrapAgent) ?? null;
  const bootstrapModels = selectedBootstrapAgent?.supportedModels ?? [];
  const availableAgentMap = useMemo(() => new Map(availableAgents.map((item) => [item.provider, item])), [availableAgents]);

  useEffect(() => {
    getProfiles().then((result) => setProfiles(result.profiles)).catch(() => setProfiles([]));
  }, []);

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

  const sessionFields = (): Pick<ConversationAutoConfigVm, 'permissionMode' | 'globalGoal'> => ({
    permissionMode: runMode.autoConfig?.permissionMode || undefined,
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
      allowedWorkflows: allowedWorkflowIds.map((workflowId) => ({ workflowId })),
      allowedProfiles,
      control,
      activeTemplateId: activeTemplateId || undefined,
      activeTemplateName: templateName.trim() || undefined,
      ...preservedSessionFields,
      ...templatePatch,
    };
  };

  const applyAutoConfig = (config: ConversationAutoConfigVm) => {
    setAgentStrategy(config.agentStrategy ?? 'fixed');
    setAgent(config.agentType ?? '');
    setBootstrapAgent(config.bootstrapAgentType ?? config.agentType ?? '');
    setBootstrapModel(config.bootstrapModelId ?? '');
    setModel(config.modelId ?? '');
    setAvailableAgents(config.availableAgents ?? []);
    setRoutingPrompt(config.routingPrompt ?? '');
    setAllowedWorkflowIds((config.allowedWorkflows ?? []).map((item) => item.workflowId));
    setAllowedProfiles(config.allowedProfiles ?? []);
    setControl({ ...DEFAULT_DYNAMIC_CONTROL, ...(config.control ?? {}) });
    setActiveTemplateId(config.activeTemplateId ?? '');
    setTemplateName(config.activeTemplateName ?? '');
  };

  const selectAutoTemplate = (templateId: string) => {
    if (templateId === '__none__') {
      setActiveTemplateId('');
      setTemplateName('');
      setAutoTemplatePickerOpen(false);
      return;
    }
    const template = templates.find((item) => item.id === templateId);
    if (!template) return;
    applyAutoConfig({ ...template.config, activeTemplateId: template.id, activeTemplateName: template.name });
    setAutoTemplatePickerOpen(false);
  };

  const deleteAutoTemplate = async (templateId: string) => {
    const nextStore = await deleteAutoTemplateApi(templateId);
    setTemplates(nextStore.templates);
    if (activeTemplateId === templateId) {
      setActiveTemplateId('');
      setTemplateName('');
    }
    setAutoNotice({ tone: 'success', message: t('runMode.autoTemplateDeleted') });
  };

  // ── Workflow template editor helpers ──
  const initWfEditor = () => {
    const initialTemplate = effectiveWorkflowTemplates?.templates[0] ?? null;
    const initialWorkflow = initialTemplate?.workflow ?? null;
    setWfEditTemplateId(initialTemplate?.id ?? null);
    setWfEditWorkflow(initialWorkflow);
    setWfSaveName('');
    setWfLastUsedHintDismissed(initialTemplate?.id === effectiveWorkflowTemplates?.lastUsedTemplateId);
    setWfNotice(null);
    setWfError(null);
  };

  // Initialize editor on first render when templates are available
  const wfEditorInitialized = useRef(false);
  useEffect(() => {
    if (!wfEditorInitialized.current && workflowTemplateList.length > 0 && mode === 'workflow') {
      wfEditorInitialized.current = true;
      initWfEditor();
    }
  }, [workflowTemplateList, mode]);

  const selectWfTemplate = (templateId: string) => {
    const found = effectiveWorkflowTemplates?.templates.find((t) => t.id === templateId);
    if (!found) return;
    setWfEditTemplateId(found.id);
    setWfEditWorkflow(found.workflow);
    setWfSaveName('');
    setWfLastUsedHintDismissed(found.id === effectiveWorkflowTemplates?.lastUsedTemplateId);
    setWfNotice(null);
    setWfError(null);
  };

  const startWfBlank = () => {
    setWfEditTemplateId(null);
    setWfEditWorkflow(null);
    setWfSaveName('');
    setWfTemplatePickerOpen(false);
    setWfNotice(null);
    setWfError(null);
  };

  const selectedWfTemplate = effectiveWorkflowTemplates?.templates.find((t) => t.id === wfEditTemplateId) ?? null;
  const wfTemplateLabel = selectedWfTemplate?.name ?? (wfEditWorkflow ? t('taskList.create.unsavedWorkflowTemplate') : t('taskList.create.workflowTemplatePlaceholder'));
  const canUpdateWfTemplate = Boolean(wfEditTemplateId && wfEditTemplateId !== 'default');
  const lastUsedWfTemplate = effectiveWorkflowTemplates?.templates.find((t) => t.id === effectiveWorkflowTemplates?.lastUsedTemplateId) ?? null;
  const showWfLastUsedHint = Boolean(lastUsedWfTemplate && wfEditTemplateId !== lastUsedWfTemplate.id && !wfLastUsedHintDismissed);

  // Validate workflow before saving template
  const applyWorkflowTemplateStore = (store: WorkflowTemplateStore) => {
    setWfTemplateStore(store);
    onWorkflowTemplatesChange?.(store);
  };

  const validateWfForTemplate = (workflow: WorkflowDsl, validateTemplateDuplicateId = true): WorkflowDsl | null => {
    const supportedAgents = agents.filter((a) => a.supported && a.diagnostic?.available === true);
    const validation = validateWorkflowForSave(workflow, profiles, supportedAgents, t, effectiveWorkflowTemplates ?? null, wfEditTemplateId, selectedWfTemplate?.name ?? null, validateTemplateDuplicateId);
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
    const validated = validateWfForTemplate(wfEditWorkflow, false);
    if (!validated) return;
    setWfSaving(true);
    try {
      const nextStore = await saveWorkflowTemplate(wfSaveName.trim(), validated);
      const selected = nextStore.templates.at(-1) ?? null;
      applyWorkflowTemplateStore(nextStore);
      setWfEditTemplateId(selected?.id ?? null);
      setWfEditWorkflow(selected?.workflow ?? null);
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
    const validated = validateWfForTemplate(wfEditWorkflow);
    if (!validated) return;
    setWfSaving(true);
    try {
      const nextStore = await updateWorkflowTemplate(wfEditTemplateId!, validated);
      const selected = nextStore.templates.find((t) => t.id === wfEditTemplateId) ?? null;
      applyWorkflowTemplateStore(nextStore);
      setWfEditWorkflow(selected?.workflow ?? wfEditWorkflow);
      setWfNotice(t('taskList.create.workflowTemplateUpdated'));
      setTimeout(() => setWfNotice(null), 3000);
    } catch (error) {
      setWfError(displayAppError(t, error));
    } finally {
      setWfSaving(false);
    }
  };

  const deleteWfTemplate = async () => {
    if (!wfDeleteTarget || wfDeleteTarget.id === 'default') return;
    setWfSaving(true);
    try {
      const nextStore = await deleteWorkflowTemplate(wfDeleteTarget.id);
      applyWorkflowTemplateStore(nextStore);
      const nextSelected = wfEditTemplateId === wfDeleteTarget.id
        ? nextStore.templates[0] ?? null
        : nextStore.templates.find((t) => t.id === wfEditTemplateId) ?? nextStore.templates[0] ?? null;
      setWfEditTemplateId(nextSelected?.id ?? null);
      setWfEditWorkflow(nextSelected?.workflow ?? null);
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

  const handleSave = () => {
    if (mode === 'auto') {
      const issues = validateAutoConfig(buildAutoConfig(), agentRegistry, effectiveWorkflowTemplates, t);
      if (issues.length > 0) {
        setAutoNotice({ tone: 'error', message: issues.join('\n') });
        return;
      }
    }
    const updated: ConversationRunModeVm = mode === 'auto'
      ? { mode: 'auto', autoConfig: buildAutoConfig() }
      : { mode: 'workflow', workflowTemplateId: (wfEditTemplateId ?? workflowTemplateId) || undefined };
    onSave(updated);
    setSaved(true);
    setAutoNotice({ tone: 'success', message: t('runMode.saved') });
    setTimeout(() => setSaved(false), 2000);
    setTimeout(() => setAutoNotice(null), 3000);
  };

  const saveAsTemplate = async () => {
    const name = templateName.trim() || t('runMode.autoTemplateFallbackName');
    if (templates.some((item) => item.name.trim() === name)) {
      setAutoNotice({ tone: 'error', message: t('runMode.validationTemplateNameDuplicated', { name }) });
      return;
    }
    const templateConfig = buildAutoConfig({ activeTemplateId: undefined, activeTemplateName: name }, false);
    const issues = validateAutoConfig(templateConfig, agentRegistry, effectiveWorkflowTemplates, t);
    if (issues.length > 0) {
      setAutoNotice({ tone: 'error', message: issues.join('\n') });
      return;
    }
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
      onSave({ mode: 'auto', autoConfig: config });
      setAutoNotice({ tone: 'success', message: t('runMode.autoTemplateSaved') });
    } catch (error) {
      setAutoNotice({ tone: 'error', message: displayAppError(t, error) });
    }
  };

  const saveCurrentTemplate = async () => {
    if (!activeTemplateId) {
      await saveAsTemplate();
      return;
    }
    const name = templateName.trim() || templates.find((item) => item.id === activeTemplateId)?.name || t('runMode.autoTemplateFallbackName');
    if (templates.some((item) => item.id !== activeTemplateId && item.name.trim() === name)) {
      setAutoNotice({ tone: 'error', message: t('runMode.validationTemplateNameDuplicated', { name }) });
      return;
    }
    const templateConfig = buildAutoConfig({ activeTemplateId, activeTemplateName: name }, false);
    const issues = validateAutoConfig(templateConfig, agentRegistry, effectiveWorkflowTemplates, t);
    if (issues.length > 0) {
      setAutoNotice({ tone: 'error', message: issues.join('\n') });
      return;
    }
    try {
      const nextStore = await updateAutoTemplate(activeTemplateId, name, templateConfig);
      const config = { ...templateConfig, ...sessionFields() };
      setTemplates(nextStore.templates);
      setTemplateName(name);
      onSave({ mode: 'auto', autoConfig: config });
      setAutoNotice({ tone: 'success', message: t('runMode.autoTemplateSaved') });
    } catch (error) {
      setAutoNotice({ tone: 'error', message: displayAppError(t, error) });
    }
  };

  const toggleAvailableAgent = (agentType: string) => {
    setAvailableAgents((current) => {
      if (current.some((item) => item.provider === agentType)) return current.filter((item) => item.provider !== agentType);
      return [...current, { provider: agentType }];
    });
  };

  const updateAvailableAgentModel = (agentType: string, modelId: string) => {
    setAvailableAgents((current) => current.map((item) => item.provider === agentType ? { ...item, model: modelId || undefined } : item));
  };

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        title={<span className="text-title">{t('runMode.title')}</span>}
        subtitle={mode === 'auto' ? t('runMode.autoDescription') : t('runMode.workflowSection')}
        actions={<Button variant="outline" size="sm" onClick={onBack}>{t('common.back')}</Button>}
      />

      <div className="min-h-0 flex-1 space-y-6 overflow-y-auto p-5 xl:p-6">
        <Tabs value={mode} onValueChange={(value) => setMode(value as 'auto' | 'workflow')}>
          <TabsList className="grid w-fit grid-cols-2">
            <TabsTrigger value="auto">{t('runMode.autoSection')}</TabsTrigger>
            <TabsTrigger value="workflow">{t('runMode.workflowSection')}</TabsTrigger>
          </TabsList>
        </Tabs>

        {mode === 'auto' ? (
          <div className="space-y-6">
            <section className="space-y-3">
              <div className="flex items-center gap-3">
                <Popover open={autoTemplatePickerOpen} onOpenChange={setAutoTemplatePickerOpen}>
                  <PopoverTrigger asChild>
                    <Button variant="outline" className="h-9 w-[220px] justify-between px-3 font-normal" aria-label={t('runMode.autoTemplate')}>
                      <span className="truncate">{templates.find((item) => item.id === activeTemplateId)?.name ?? t('runMode.noAutoTemplate')}</span>
                      <ChevronDown className="ml-2 size-4 shrink-0 opacity-50" />
                    </Button>
                  </PopoverTrigger>
                  <PopoverContent className="w-[280px] p-0" align="start">
                    <div className="p-1">
                      <button
                        type="button"
                        className={cn('flex w-full items-center rounded-sm px-2 py-1.5 text-left text-xs hover:bg-accent', !activeTemplateId && 'bg-accent text-accent-foreground')}
                        onClick={() => selectAutoTemplate('__none__')}
                      >
                        {t('runMode.noAutoTemplate')}
                      </button>
                    </div>
                    {templates.length > 0 ? <Separator /> : null}
                    <div className="max-h-64 overflow-auto p-1">
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
                                  setAutoNotice({ tone: 'error', message: displayAppError(t, error) });
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
                <Input className="h-9 max-w-[220px]" value={templateName} onChange={(event) => setTemplateName(event.target.value)} placeholder={t('runMode.autoTemplateName')} />
                <Button variant="secondary" size="sm" onClick={() => void saveCurrentTemplate()}>{t('runMode.saveTemplate')}</Button>
                <Button variant="outline" size="sm" onClick={() => void saveAsTemplate()}>{t('runMode.saveAsTemplate')}</Button>
              </div>
            </section>

            <section className="flex flex-wrap gap-2">
              <Field label={<><Bot className="size-3.5" />{t('workflowEditor.dynamicAgentStrategy')}</>} required>
                <Select value={agentStrategy} onValueChange={(value) => setAgentStrategy(value as 'fixed' | 'dynamic')}>
                  <SelectTrigger className="h-9 w-[180px]"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="fixed">{t('workflowEditor.dynamicAgentStrategyFixed')}</SelectItem>
                    <SelectItem value="dynamic">{t('workflowEditor.dynamicAgentStrategyDynamic')}</SelectItem>
                  </SelectContent>
                </Select>
              </Field>

              {agentStrategy === 'fixed' ? (
                <Field label={t('runMode.agent')} required>
                  <Select value={agent} onValueChange={(value) => { setAgent(value); setModel(''); }}>
                    <SelectTrigger className="h-9 w-[180px]"><SelectValue placeholder={t('conversation.home.selectAgent')} /></SelectTrigger>
                    <SelectContent>
                      {agentOptions.map(({ agent: item, selectable, reason }) => (
                        <SelectItem key={item.agentType} value={item.agentType} disabled={!selectable}>
                          <span className="block min-w-0">
                            <span className="block truncate">{item.displayName}</span>
                            {!selectable && reason ? <span className="mt-0.5 block whitespace-normal text-[11px] text-destructive">{reason}</span> : null}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              ) : (
                <Field label={t('workflowEditor.dynamicBootstrapAgent')} required>
                  <Select value={bootstrapAgent} onValueChange={(value) => { setBootstrapAgent(value); setBootstrapModel(''); }}>
                    <SelectTrigger className="h-9 w-[180px]"><SelectValue placeholder={t('conversation.home.selectAgent')} /></SelectTrigger>
                    <SelectContent>
                      {agentOptions.map(({ agent: item, selectable, reason }) => (
                        <SelectItem key={item.agentType} value={item.agentType} disabled={!selectable}>
                          <span className="block min-w-0">
                            <span className="block truncate">{item.displayName}</span>
                            {!selectable && reason ? <span className="mt-0.5 block whitespace-normal text-[11px] text-destructive">{reason}</span> : null}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              )}

              {agentStrategy === 'fixed' && fixedModels.length > 0 ? (
                <Field label={t('runMode.model')}>
                  <ClearableModelSelect
                    value={model}
                    models={fixedModels}
                    placeholder={t('conversation.home.selectModel')}
                    clearLabel={t('workflowEditor.clearModel')}
                    triggerClassName="w-[180px]"
                    onChange={setModel}
                  />
                </Field>
              ) : null}

              {agentStrategy === 'dynamic' && bootstrapModels.length > 0 ? (
                <Field label={t('workflowEditor.dynamicBootstrapModel')}>
                  <ClearableModelSelect
                    value={bootstrapModel}
                    models={bootstrapModels}
                    placeholder={t('conversation.home.selectModel')}
                    clearLabel={t('workflowEditor.clearModel')}
                    triggerClassName="w-[180px]"
                    onChange={setBootstrapModel}
                  />
                </Field>
              ) : null}

            </section>

            {agentStrategy === 'dynamic' ? (
              <section className="space-y-3">
                <Field label={t('workflowEditor.dynamicAvailableAgents')} required>
                  <div className="grid gap-2">
                    {agentOptions.map(({ agent: item, selectable, reason }) => {
                      const selected = availableAgentMap.has(item.agentType);
                      const selectedModel = availableAgentMap.get(item.agentType)?.model ?? '';
                      return (
                        <div key={item.agentType} className={cn('flex items-center gap-2 rounded-md border border-border/60 bg-background/35 px-3 py-2', !selectable && 'opacity-60')}>
                          <button type="button" disabled={!selectable} className={cn('size-4 rounded border disabled:cursor-not-allowed', selected ? 'border-primary bg-primary' : 'border-border')} onClick={() => toggleAvailableAgent(item.agentType)} aria-label={item.displayName} />
                          <span className="min-w-0 flex-1 text-sm">
                            <span className="block truncate">{item.displayName}</span>
                            {!selectable && reason ? <span className="mt-0.5 block text-xs text-destructive">{reason}</span> : null}
                          </span>
                          {selected && (item.supportedModels?.length ?? 0) > 0 ? (
                            <ClearableModelSelect
                              value={selectedModel}
                              models={item.supportedModels ?? []}
                              placeholder={t('conversation.home.selectModel')}
                              clearLabel={t('workflowEditor.clearModel')}
                              triggerClassName="h-8 w-[220px] text-xs"
                              buttonClassName="size-8"
                              onChange={(value) => updateAvailableAgentModel(item.agentType, value)}
                            />
                          ) : null}
                        </div>
                      );
                    })}
                  </div>
                </Field>
                <Field label={t('workflowEditor.dynamicAgentRoutingPrompt')}>
                  <Textarea className="min-h-20" value={routingPrompt} onChange={(event) => setRoutingPrompt(event.target.value)} placeholder={t('workflowEditor.dynamicAgentRoutingPromptPlaceholder')} />
                </Field>
              </section>
            ) : null}

            <section className="grid gap-4 md:grid-cols-2">
              <Field label={t('workflowEditor.allowedWorkflows')}>
                <MultiToggle
                  items={workflowOptions.map(({ template, workflowId, selectable, reason }) => ({ id: workflowId || template.id, label: template.name, selectable, reason }))}
                  selected={allowedWorkflowIds}
                  onChange={setAllowedWorkflowIds}
                  emptyLabel={t('workflowEditor.noWorkflowTemplates')}
                />
              </Field>
              <Field label={t('workflowEditor.allowedProfiles')}>
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
                <Field key={item.key} label={item.label} required>
                  <Input className="h-9" type="number" min={1} step={1} value={String(control[item.key])} onChange={(event) => setControl((current) => ({ ...current, [item.key]: parsePositiveInt(event.target.value) }))} />
                </Field>
              ))}
            </section>
          </div>
        ) : (
          <div className="space-y-4">
            {/* Template picker + actions */}
            <div className="flex flex-wrap items-center gap-3">
              <span className="text-xs font-medium text-muted-foreground">{t('taskList.create.workflowTemplate')}</span>
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
                  <div className="max-h-64 overflow-auto p-1">
                    {workflowTemplateList.map((tpl) => {
                      const selected = tpl.id === wfEditTemplateId;
                      const isDefault = tpl.id === 'default';
                      return (
                        <div key={tpl.id} className={cn('flex items-center gap-1 rounded-sm p-1', selected && 'bg-accent text-accent-foreground')}>
                          <button
                            type="button"
                            className="min-w-0 flex-1 truncate px-1 py-1 text-left text-xs"
                            onClick={() => { selectWfTemplate(tpl.id); setWfTemplatePickerOpen(false); }}
                          >
                            {tpl.name}
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

              {showWfLastUsedHint && lastUsedWfTemplate ? (
                <button
                  type="button"
                  className="rounded-full border border-primary/30 bg-primary/5 px-3 py-1 text-xs text-primary hover:bg-primary/10"
                  onClick={() => selectWfTemplate(lastUsedWfTemplate.id)}
                >
                  {t('taskList.create.selectLastUsedWorkflow', { name: lastUsedWfTemplate.name })}
                </button>
              ) : null}

              {canUpdateWfTemplate ? (
                <Button variant="outline" size="sm" disabled={wfSaving} onClick={saveWfCurrent}>
                  {wfSaving ? t('taskList.create.savingWorkflowTemplate') : t('taskList.create.saveCurrentWorkflow')}
                </Button>
              ) : null}
              <Input className="h-8 w-40" value={wfSaveName} placeholder={t('taskList.create.workflowTemplateName')} onChange={(e) => setWfSaveName(e.target.value)} />
              <Button variant="outline" size="sm" disabled={!wfSaveName.trim() || wfSaving} onClick={saveWfAsNew}>
                {t('taskList.create.saveAsWorkflow')}
              </Button>
            </div>

            {wfNotice ? (
              <div className="rounded-md border border-emerald-500/20 bg-emerald-500/5 px-3 py-2 text-xs text-emerald-600">{wfNotice}</div>
            ) : null}
            {wfError ? (
              <div className="whitespace-pre-wrap rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">{wfError}</div>
            ) : null}

            {/* Embedded workflow editor */}
            <div className="min-h-[480px] min-w-0">
              {wfEditWorkflow ? (
                <WorkflowEditor
                  value={wfEditWorkflow}
                  agentRegistry={agentRegistry}
                  profiles={profiles}
                  workflowTemplates={effectiveWorkflowTemplates}
                  currentTemplateId={wfEditTemplateId}
                  currentTemplateName={selectedWfTemplate?.name ?? null}
                  showSaveAction={false}
                  allowAiDynamic={true}
                  onChange={setWfEditWorkflow}
                  onSave={async () => {
                    if (canUpdateWfTemplate) await saveWfCurrent();
                    else await saveWfAsNew();
                  }}
                />
              ) : (
                <div className="flex h-[480px] items-center justify-center rounded-xl border border-dashed border-border bg-muted/20 text-sm text-muted-foreground">
                  {workflowTemplateList.length > 0
                    ? t('taskList.create.newWorkflowTemplate')
                    : t('taskList.create.noWorkflowTemplate')}
                </div>
              )}
            </div>
          </div>
        )}

        <div className="flex items-center gap-3">
          <Button className="gap-2" onClick={handleSave}>
            <Save className="size-4" />
            {t('common.save')}
          </Button>
          {saved ? <span className="text-sm text-emerald-500">{t('runMode.saved')}</span> : null}
        </div>
        {autoNotice ? (
          <div className={cn('whitespace-pre-wrap rounded-md border px-3 py-2 text-sm', autoNotice.tone === 'success' ? 'border-emerald-500/20 bg-emerald-500/5 text-emerald-600' : 'border-destructive/30 bg-destructive/5 text-destructive')}>
            {autoNotice.message}
          </div>
        ) : null}
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

function Field({ label, children, required = false }: { label: ReactNode; children: ReactNode; required?: boolean }) {
  return (
    <div className="grid gap-1.5 text-sm">
      <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
        <span className="inline-flex min-w-0 items-center gap-1.5">{label}</span>
        {required ? <span className="text-destructive">*</span> : null}
      </div>
      {children}
    </div>
  );
}

function ModelItem({ id, name, description }: { id: string; name: string; description?: string | null }) {
  return (
    <SelectItem value={id} className="items-start py-2">
      <span className="block min-w-0">
        <span className="block truncate font-medium">{name}</span>
        {description ? <span className="mt-0.5 block whitespace-normal break-words text-[11px] leading-4 text-muted-foreground">{description}</span> : null}
      </span>
    </SelectItem>
  );
}

function ClearableModelSelect({
  value,
  models,
  placeholder,
  clearLabel,
  triggerClassName,
  buttonClassName,
  onChange,
}: {
  value: string;
  models: Array<{ id: string; name: string; description?: string | null }>;
  placeholder: string;
  clearLabel: string;
  triggerClassName?: string;
  buttonClassName?: string;
  onChange: (value: string) => void;
}) {
  const selected = models.find((item) => item.id === value) ?? null;
  return (
    <div className="flex min-w-0 items-center gap-1">
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger className={cn('h-9 min-w-0', triggerClassName)}>
          <span className="truncate">{selected?.name ?? placeholder}</span>
        </SelectTrigger>
        <SelectContent className="w-[min(28rem,calc(100vw-2rem))]">
          {models.map((item) => <ModelItem key={item.id} id={item.id} name={item.name} description={item.description} />)}
        </SelectContent>
      </Select>
      {value ? (
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          className={cn('size-9 shrink-0', buttonClassName)}
          aria-label={clearLabel}
          onClick={() => onChange('')}
        >
          <X className="size-3.5" />
        </Button>
      ) : null}
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
            <button
              key={`${item.id}-${index}`}
              type="button"
              className={cn('max-w-full rounded-full border px-2.5 py-1 text-xs transition-colors', active ? 'border-primary/40 bg-primary/10 text-primary' : 'border-border/60 bg-background/35 text-muted-foreground hover:text-foreground')}
              onClick={() => onChange(active ? selected.filter((id) => id !== item.id) : [...selected, item.id])}
              title={item.id}
            >
              <span className="block max-w-52 truncate">{item.label}</span>
            </button>
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

function dynamicControlFields(t: (key: string) => string): Array<{ key: Exclude<keyof DynamicControlDsl, 'allowNestedDynamic'>; label: string }> {
  return [
    { key: 'maxDynamicNodes', label: t('workflowEditor.maxDynamicNodes') },
    { key: 'maxFanout', label: t('workflowEditor.maxFanout') },
    { key: 'maxDepth', label: t('workflowEditor.maxDepth') },
    { key: 'maxParallel', label: t('workflowEditor.maxParallel') },
    { key: 'maxGroupDepth', label: t('workflowEditor.maxGroupDepth') },
    { key: 'maxWorkflowInvocations', label: t('workflowEditor.maxWorkflowInvocations') },
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
