import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getAcpRawFrames, getAcpSession, getAgentRegistry, getConversationExecutionPlan, getWorkflowTemplates, preflightConversationExecutionPlanSave, recoverConversationExecutionPlanOperation, saveConversationExecutionPlan } from '@/api';
import { RawFrameViewer, SystemPromptPanel } from '@/components/acp/ACPChatDialog';
import { resolveGoldBandHiddenSection } from '@/components/acp/hiddenPromptSections';
import { GraphView } from '@/components/GraphView';
import { StatusBadge } from '@/components/StatusBadge';
import { WorkflowEditor, type WorkflowEditorLayout, type WorkflowEditorSessionDraft } from '@/components/WorkflowEditor';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import { displayAppError } from '@/i18n';
import { workflowEditorSessionDraftIsDirty } from '@/lib/workflow-editor-session-draft';
import { ExecutionPlanSaveBar } from '@/components/conversation/ExecutionPlanSaveBar';
import { RunModeManagementPage } from '@/pages/RunModeManagementPage';
import {
  activeExecutionPlanBaseline,
  activeExecutionPlanDraft,
  applyExecutionPlanSave,
  cancelExecutionPlanUnsplit,
  confirmExecutionPlanUnsplit,
  executionPlanSessionIsDirty,
  revertExecutionPlanToPersisted,
  createExecutionPlanSession,
  executionPlanBaselinesFromView,
  executionPlanIssueNotice,
  executionPlanSaveExpectations,
  executionPlanThrownNotice,
  handoffExecutionPlanSide,
  openExecutionPlanUnsplit,
  rebaseExecutionPlanSession,
  resolveExecutionPlanTarget,
  splitExecutionPlanSession,
  withActiveExecutionPlanDraft,
  withExecutionPlanPhase,
  type ExecutionPlanBaselines,
  type ExecutionPlanSession,
} from '@/lib/execution-plan-editor';
import { readExecutionPlanDraft, writeExecutionPlanDraft } from '@/lib/execution-plan-draft-cache';
import { readExecutionPlanSaveTarget, writeExecutionPlanSaveTarget } from '@/lib/execution-plan-preferences';
import { useWorkflowProfileCatalog } from '@/lib/workflow-profile-catalog';
import type {
  AcpRawFramePageVm,
  AcpRawFrameQueryInput,
  AgentRegistryVm,
  ConversationRunVm,
  ExecutionPlanSaveResultVm,
  ExecutionPlanSaveTarget,
  ExecutionPlanViewVm,
  GraphNodeVm,
  WorkflowDsl,
  WorkflowModelBindings,
  WorkflowTemplateStore,
} from '@/types';
import {
  type AutoConfigWorkspaceResource,
  type RawFramesWorkspaceResource,
  type RightWorkspaceResource,
  type HiddenPromptSectionWorkspaceResource,
  type SystemPromptWorkspaceResource,
  type WorkflowEditWorkspaceResource,
  type WorkflowViewWorkspaceResource,
} from './right-workspace-context';

type ConversationRunWorkspaceResource =
  | WorkflowViewWorkspaceResource
  | WorkflowEditWorkspaceResource
  | AutoConfigWorkspaceResource
  | SystemPromptWorkspaceResource
  | HiddenPromptSectionWorkspaceResource
  | RawFramesWorkspaceResource;

interface ConversationRunWorkspaceResourcePanelProps {
  resource: ConversationRunWorkspaceResource;
  run: ConversationRunVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates?: WorkflowTemplateStore | null;
  onWorkflowTemplatesChange?: (store: WorkflowTemplateStore) => void;
  /** Fires after an execution-plan save fully committed so the owner can refresh the run projection. */
  onExecutionPlanSaved?: (saved: ExecutionPlanSaveResultVm) => void | Promise<void>;
  onNodeOpenSession?: (node: GraphNodeVm) => void;
}

export function conversationRunWorkspacePropsAreEqual(
  prev: ConversationRunWorkspaceResourcePanelProps,
  next: ConversationRunWorkspaceResourcePanelProps,
) {
  if (prev.resource.kind !== next.resource.kind || prev.resource.key !== next.resource.key) return false;
  if (prev.resource.kind === 'workflow-edit' && next.resource.kind === 'workflow-edit' && prev.resource.mode !== next.resource.mode) return false;
  if (prev.agentRegistry !== next.agentRegistry) return false;
  if (prev.workflowTemplates !== next.workflowTemplates) return false;
  if (prev.onExecutionPlanSaved !== next.onExecutionPlanSaved) return false;
  if (prev.onNodeOpenSession !== next.onNodeOpenSession) return false;
  const sameScope = prev.run.projectId === next.run.projectId
    && prev.run.taskId === next.run.taskId
    && prev.run.taskUuid === next.run.taskUuid
    && prev.run.runId === next.run.runId
    && prev.run.workflowValid === next.run.workflowValid;
  if (prev.resource.kind === 'workflow-view') return sameScope && prev.run.workflowGraph === next.run.workflowGraph;
  if (prev.resource.kind === 'workflow-edit' || prev.resource.kind === 'auto-config') return sameScope;
  return prev.run === next.run && prev.resource === next.resource;
}

function ConversationRunWorkspaceResourcePanelComponent({
  resource,
  run,
  agentRegistry,
  workflowTemplates = null,
  onWorkflowTemplatesChange,
  onExecutionPlanSaved,
  onNodeOpenSession,
}: ConversationRunWorkspaceResourcePanelProps) {
  if (resource.kind === 'auto-config') {
    return (
      <AutoConfigWorkspacePanel
        resource={resource}
        agentRegistry={agentRegistry}
        workflowTemplates={workflowTemplates}
        onWorkflowTemplatesChange={onWorkflowTemplatesChange}
        onExecutionPlanSaved={onExecutionPlanSaved}
      />
    );
  }
  if (resource.kind === 'workflow-view') {
    return <WorkflowViewPanel run={run} onNodeOpenSession={onNodeOpenSession} />;
  }
  if (resource.kind === 'workflow-edit') {
    return (
      <WorkflowEditPanel
        resource={resource}
        run={run}
        initialAgentRegistry={agentRegistry}
        onExecutionPlanSaved={onExecutionPlanSaved}
      />
    );
  }
  if (resource.kind === 'system-prompt') {
    return <SystemPromptWorkspacePanel resource={resource} />;
  }
  if (resource.kind === 'hidden-prompt-section') {
    return <HiddenPromptSectionWorkspacePanel resource={resource} />;
  }
  return <RawFramesWorkspacePanel resource={resource} />;
}

export const ConversationRunWorkspaceResourcePanel = memo(
  ConversationRunWorkspaceResourcePanelComponent,
  conversationRunWorkspacePropsAreEqual,
);

function WorkspaceLoadingState() {
  const { t } = useTranslation();
  return (
    <div className="flex min-h-0 flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
      <Loader2 className="size-4 animate-spin" />
      {t('common.loading')}
    </div>
  );
}

function WorkflowViewPanel({ run, onNodeOpenSession }: { run: ConversationRunVm; onNodeOpenSession?: (node: GraphNodeVm) => void }) {
  const { t } = useTranslation();
  if (run.workflowGraph.nodes.length === 0) {
    return <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">{t('common.empty')}</div>;
  }
  return (
    <div className="min-h-0 flex-1 p-2" data-right-workspace-resource="workflow-view">
      <GraphView
        graph={run.workflowGraph}
        variant="actual"
        onNodeOpenDetail={onNodeOpenSession}
        onNodeOpenSession={onNodeOpenSession}
      />
    </div>
  );
}

interface WorkflowDraftCacheEntry {
  baselineWorkflow: WorkflowDsl;
  baselineModelBindings: WorkflowModelBindings;
  draft: WorkflowDsl;
  editorDraft: WorkflowEditorSessionDraft;
}

const workflowDraftCache = new BoundedLruCache<string, WorkflowDraftCacheEntry>(24);

export function clearConversationRunWorkflowDraftCache() {
  workflowDraftCache.clear();
}

interface WorkflowPlanDraft {
  workflow: WorkflowDsl;
  modelBindings: WorkflowModelBindings;
}

type WorkflowPlanSession = ExecutionPlanSession<WorkflowPlanDraft>;

const EMPTY_MODEL_BINDINGS: WorkflowModelBindings = { definitionRevision: '', bindingRevision: 0, bindings: [] };

function workflowPlanBaselines(plan: ExecutionPlanViewVm): ExecutionPlanBaselines<WorkflowPlanDraft> {
  if (!plan.currentWorkflow || !plan.nextWorkflow) throw new Error('conversation.execution-plan.not-found');
  return executionPlanBaselinesFromView(
    plan,
    { workflow: plan.currentWorkflow, modelBindings: plan.currentModelBindings ?? EMPTY_MODEL_BINDINGS },
    { workflow: plan.nextWorkflow, modelBindings: plan.nextModelBindings ?? EMPTY_MODEL_BINDINGS },
  );
}

function workflowDraftIsDirty(entry: WorkflowDraftCacheEntry) {
  return workflowEditorSessionDraftIsDirty(
    entry.baselineWorkflow,
    entry.baselineModelBindings,
    entry.editorDraft,
  );
}

export function confirmCloseConversationRunWorkspaceResource(
  resource: RightWorkspaceResource,
  confirmDiscard: () => boolean,
) {
  if (resource.kind !== 'workflow-edit') return true;
  const cached = workflowDraftCache.peek(resource.key);
  if (!cached || !workflowDraftIsDirty(cached)) return true;
  if (!confirmDiscard()) return false;
  workflowDraftCache.delete(resource.key);
  return true;
}

const EMBEDDED_AUTO_RUN_MODE = { mode: 'auto' as const };

function AutoConfigWorkspacePanel({
  resource,
  agentRegistry,
  workflowTemplates,
  onWorkflowTemplatesChange,
  onExecutionPlanSaved,
}: {
  resource: AutoConfigWorkspaceResource;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  onWorkflowTemplatesChange?: (store: WorkflowTemplateStore) => void;
  onExecutionPlanSaved?: (saved: ExecutionPlanSaveResultVm) => void | Promise<void>;
}) {
  const [templates, setTemplates] = useState(workflowTemplates);
  const [registry, setRegistry] = useState(agentRegistry);
  const onTemplatesChangeRef = useRef(onWorkflowTemplatesChange);
  onTemplatesChangeRef.current = onWorkflowTemplatesChange;

  useEffect(() => {
    if (workflowTemplates) setTemplates(workflowTemplates);
  }, [workflowTemplates]);

  useEffect(() => {
    if (agentRegistry) setRegistry(agentRegistry);
  }, [agentRegistry]);

  useEffect(() => {
    let active = true;
    if (!workflowTemplates) {
      void getWorkflowTemplates().then((store) => {
        if (active) setTemplates(store);
      }).catch(() => {});
    }
    if (!agentRegistry) {
      void getAgentRegistry().then((next) => {
        if (active) setRegistry(next);
      }).catch(() => {});
    }
    return () => { active = false; };
  }, [agentRegistry, workflowTemplates]);

  const locator = resource.locator;
  return (
    <RunModeManagementPage
      embedded
      projectId={locator.projectId}
      workspaceName=""
      workspaces={[]}
      runMode={EMBEDDED_AUTO_RUN_MODE}
      agentRegistry={registry}
      workflowTemplates={templates}
      onProjectChange={() => undefined}
      onSave={() => undefined}
      onWorkflowTemplatesChange={(store) => {
        setTemplates(store);
        onTemplatesChangeRef.current?.(store);
      }}
      runContext={{
        projectId: locator.projectId,
        taskId: locator.taskId,
        taskUuid: locator.taskUuid || locator.taskId,
        runId: locator.runId,
      }}
      onExecutionPlanSaved={onExecutionPlanSaved}
    />
  );
}

function WorkflowEditPanel({
  resource,
  run,
  initialAgentRegistry,
  onExecutionPlanSaved,
}: {
  resource: WorkflowEditWorkspaceResource;
  run: ConversationRunVm;
  initialAgentRegistry: AgentRegistryVm | null;
  onExecutionPlanSaved?: (saved: ExecutionPlanSaveResultVm) => void | Promise<void>;
}) {
  const { t } = useTranslation();
  const translateRef = useRef(t);
  translateRef.current = t;
  const onExecutionPlanSavedRef = useRef(onExecutionPlanSaved);
  onExecutionPlanSavedRef.current = onExecutionPlanSaved;
  const cached = useMemo(() => workflowDraftCache.peek(resource.key), [resource.key]);
  const [draft, setDraft] = useState<WorkflowDsl | null>(() => cached?.draft ?? null);
  const [editorDraft, setEditorDraft] = useState<WorkflowEditorSessionDraft | null>(() => cached?.editorDraft ?? null);
  const [baselineWorkflow, setBaselineWorkflow] = useState<WorkflowDsl | null>(() => cached?.baselineWorkflow ?? null);
  const [baselineModelBindings, setBaselineModelBindings] = useState<WorkflowModelBindings | null>(() => cached?.baselineModelBindings ?? null);
  const [registry, setRegistry] = useState(initialAgentRegistry);
  const profileCatalog = useWorkflowProfileCatalog();
  const [dependenciesLoading, setDependenciesLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [planSession, setPlanSession] = useState<WorkflowPlanSession | null>(null);
  const [planOperationId, setPlanOperationId] = useState<string | null>(null);
  const [planReloadNonce, setPlanReloadNonce] = useState(0);
  const [editorLayout, setEditorLayout] = useState<WorkflowEditorLayout>('split');
  const [inspectorRequestId, setInspectorRequestId] = useState(0);
  const [restoreRequestId, setRestoreRequestId] = useState(0);
  const planSessionRef = useRef<WorkflowPlanSession | null>(null);
  const initialAgentRegistryRef = useRef(initialAgentRegistry);
  initialAgentRegistryRef.current = initialAgentRegistry;
  const editorSessionRef = useRef<WorkflowEditorSessionDraft | null>(null);
  planSessionRef.current = planSession;
  const planLocator = useMemo(() => ({
    projectId: run.projectId,
    taskId: run.taskId,
    taskUuid: run.taskUuid || run.taskId,
    runId: run.runId,
  }), [run.projectId, run.runId, run.taskId, run.taskUuid]);
  const livePlanDraft = draft && (editorDraft || baselineModelBindings)
    ? { workflow: editorDraft?.workflow ?? draft, modelBindings: editorDraft?.modelBindings ?? baselineModelBindings ?? EMPTY_MODEL_BINDINGS }
    : null;
  const dirty = Boolean(
    (draft && editorDraft && baselineWorkflow && baselineModelBindings
      && workflowEditorSessionDraftIsDirty(baselineWorkflow, baselineModelBindings, editorDraft))
    || (planSession && livePlanDraft && executionPlanSessionIsDirty(planSession, livePlanDraft)),
  );

  // Points the editor at the active side of the plan session: draft, baseline and
  // editor UI state all follow the tab, so one tab never leaks into the other.
  const showPlanSide = useCallback((session: WorkflowPlanSession) => {
    const activeDraft = activeExecutionPlanDraft(session);
    const baseline = activeExecutionPlanBaseline(session);
    setDraft(activeDraft.workflow);
    setBaselineWorkflow(baseline.workflow);
    setBaselineModelBindings(baseline.modelBindings);
    setEditorDraft({
      workflow: activeDraft.workflow,
      modelBindings: activeDraft.modelBindings,
      tab: 'canvas',
      jsonDraft: JSON.stringify(activeDraft.workflow, null, 2),
    });
    workflowDraftCache.delete(resource.key);
  }, [resource.key]);

  const commitPlanSession = useCallback((session: WorkflowPlanSession, refocusEditor: boolean) => {
    setPlanSession(session);
    writeExecutionPlanDraft(planLocator, session);
    if (refocusEditor) showPlanSide(session);
  }, [planLocator, showPlanSide]);

  useEffect(() => {
    if (initialAgentRegistry) setRegistry(initialAgentRegistry);
  }, [initialAgentRegistry]);

  useEffect(() => {
    let active = true;
    if (planSessionRef.current == null) {
      setDependenciesLoading(true);
      setLoadError(null);
    }
    const knownRegistry = initialAgentRegistryRef.current;
    Promise.all([
      knownRegistry ? Promise.resolve(knownRegistry) : getAgentRegistry().catch(() => ({ agents: [], catalog: [] })),
      getConversationExecutionPlan(planLocator.projectId, planLocator.taskId, planLocator.taskUuid, planLocator.runId),
    ])
      .then(([nextRegistry, plan]) => {
        if (!active) return;
        const baselines = workflowPlanBaselines(plan);
        const remembered = planSessionRef.current ?? readExecutionPlanDraft<WorkflowPlanSession>(planLocator) ?? null;
        let session = remembered
          ? rebaseExecutionPlanSession(remembered, baselines)
          : createExecutionPlanSession(baselines, readExecutionPlanSaveTarget());
        const cachedDraft = workflowDraftCache.peek(resource.key);
        const cachedDirty = Boolean(cachedDraft && workflowDraftIsDirty(cachedDraft));
        if (cachedDraft && !cachedDirty) workflowDraftCache.delete(resource.key);
        if (cachedDraft && cachedDirty) {
          session = withActiveExecutionPlanDraft(session, {
            workflow: cachedDraft.editorDraft.workflow,
            modelBindings: cachedDraft.editorDraft.modelBindings,
          });
        }
        setRegistry(nextRegistry);
        setPlanSession(session);
        writeExecutionPlanDraft(planLocator, session);
        const activeDraft = activeExecutionPlanDraft(session);
        const activeBaseline = activeExecutionPlanBaseline(session);
        setDraft(activeDraft.workflow);
        setBaselineWorkflow(activeBaseline.workflow);
        setBaselineModelBindings(activeBaseline.modelBindings);
        setEditorDraft(cachedDirty && cachedDraft
          ? cachedDraft.editorDraft
          : {
              workflow: activeDraft.workflow,
              modelBindings: activeDraft.modelBindings,
              tab: 'canvas',
              jsonDraft: JSON.stringify(activeDraft.workflow, null, 2),
            });
      })
      .catch((error) => {
        if (active) setLoadError(displayAppError(translateRef.current, error));
      })
      .finally(() => {
        if (active) setDependenciesLoading(false);
      });
    return () => { active = false; };
  }, [planLocator, planReloadNonce, resource.key]);

  useEffect(() => {
    if (!dirty) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  const handleEditorDraftChange = useCallback((next: WorkflowEditorSessionDraft) => {
    setDraft(next.workflow);
    setEditorDraft(next);
    const session = planSessionRef.current;
    if (session) {
      const updated = withActiveExecutionPlanDraft(session, { workflow: next.workflow, modelBindings: next.modelBindings });
      planSessionRef.current = updated;
      setPlanSession(updated);
      writeExecutionPlanDraft(planLocator, updated);
    }
    if (!baselineWorkflow || !baselineModelBindings) return;
    workflowDraftCache.set(resource.key, {
      baselineWorkflow,
      baselineModelBindings,
      draft: next.workflow,
      editorDraft: next,
    });
  }, [baselineModelBindings, baselineWorkflow, planLocator, resource.key]);

  const applyPlanOutcome = useCallback((
    session: WorkflowPlanSession,
    saved: ExecutionPlanSaveResultVm,
    savedDraft: WorkflowPlanDraft,
  ) => {
    const nextSession = applyExecutionPlanSave(session, {
      complete: saved.complete,
      diverged: saved.diverged,
      planRevision: saved.planRevision,
      authoringRevision: saved.authoringRevision,
      executionRevision: saved.executionRevision,
      savedDraft,
      targets: saved.targets.map((item) => ({ target: item.target, committed: item.committed, errorCode: item.error?.code ?? null })),
    });
    commitPlanSession(nextSession, true);
    if (saved.complete) {
      setPlanOperationId(null);
      void onExecutionPlanSavedRef.current?.(saved);
    }
  }, [commitPlanSession]);

  const handlePlanSave = useCallback(async (target: ExecutionPlanSaveTarget) => {
    if (!planSession || !draft) return;
    const resolved = resolveExecutionPlanTarget(planSession, target);
    writeExecutionPlanSaveTarget(resolved);
    const live = editorSessionRef.current;
    const submitted: WorkflowPlanDraft = live
      ? { workflow: live.workflow, modelBindings: live.modelBindings }
      : { workflow: draft, modelBindings: editorDraft?.modelBindings ?? baselineModelBindings ?? EMPTY_MODEL_BINDINGS };
    const baseSession = withActiveExecutionPlanDraft({ ...planSession, preferredTarget: resolved }, submitted);
    const savingSession = withExecutionPlanPhase(baseSession, 'saving');
    setPlanSession(savingSession);
    setSaving(true);
    const operationId = planOperationId ?? `plan-${planLocator.runId}-${Date.now()}`;
    setPlanOperationId(operationId);
    let preflightPassed = false;
    try {
      const command = {
        ...planLocator,
        operationId,
        target: resolved,
        ...executionPlanSaveExpectations(planSession),
        workflow: submitted,
      };
      const preflight = await preflightConversationExecutionPlanSave(command);
      if (preflight.blocking.length > 0) {
        // Preflight does not create a recoverable operation journal.
        setPlanOperationId(null);
        const notice = executionPlanIssueNotice(preflight.blocking);
        commitPlanSession(withExecutionPlanPhase(baseSession, 'error', notice.code, notice.nodeIds), false);
        return;
      }
      preflightPassed = true;
      const saved = await saveConversationExecutionPlan(command);
      applyPlanOutcome(savingSession, saved, submitted);
    } catch (error) {
      if (!preflightPassed) setPlanOperationId(null);
      const notice = executionPlanThrownNotice(error);
      commitPlanSession(withExecutionPlanPhase(baseSession, notice.code?.includes('conflict') ? 'conflict' : 'error', notice.code, notice.nodeIds), false);
    } finally {
      setSaving(false);
    }
  }, [applyPlanOutcome, baselineModelBindings, commitPlanSession, draft, editorDraft?.modelBindings, planLocator, planOperationId, planSession]);

  const handlePlanRecover = useCallback(async () => {
    if (!planSession || !planOperationId || !draft) return;
    const live = editorSessionRef.current;
    const submitted: WorkflowPlanDraft = live
      ? { workflow: live.workflow, modelBindings: live.modelBindings }
      : { workflow: draft, modelBindings: editorDraft?.modelBindings ?? baselineModelBindings ?? EMPTY_MODEL_BINDINGS };
    setPlanSession(withExecutionPlanPhase(planSession, 'saving'));
    setSaving(true);
    try {
      const recovered = await recoverConversationExecutionPlanOperation(planLocator.projectId, planLocator.taskId, planLocator.taskUuid, planLocator.runId, planOperationId);
      applyPlanOutcome(planSession, recovered, submitted);
    } catch (error) {
      const code = typeof error === 'object' && error && 'code' in error ? String((error as { code: string }).code) : null;
      commitPlanSession(withExecutionPlanPhase(planSession, 'partial', code), false);
    } finally {
      setSaving(false);
    }
  }, [applyPlanOutcome, baselineModelBindings, commitPlanSession, draft, editorDraft?.modelBindings, planLocator, planOperationId, planSession]);

  const handleEditorLayout = useCallback((layout: WorkflowEditorLayout) => {
    setEditorLayout((current) => current === layout ? current : layout);
  }, []);

  const handleRevert = useCallback(() => {
    const session = planSessionRef.current;
    if (!session) return;
    commitPlanSession(revertExecutionPlanToPersisted(session), true);
    setRestoreRequestId((value) => value + 1);
  }, [commitPlanSession]);

  if (dependenciesLoading) return <WorkspaceLoadingState />;
  if (loadError) {
    return <div className="flex min-h-0 flex-1 items-center justify-center px-4 text-sm text-destructive">{loadError}</div>;
  }
  if (!draft || !planSession) {
    return <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">{t('common.empty')}</div>;
  }
  const repairMode = resource.mode === 'repair';
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden" data-right-workspace-resource="workflow-edit">
      {repairMode ? (
        <div className="flex shrink-0 flex-col gap-2 border-b border-border/60 px-4 py-3">
          <div className="flex items-center gap-2">
            <StatusBadge value={run.workflowValid ? 'valid' : 'invalid'} label={run.workflowValid ? t('status.valid') : t('status.invalid')} />
          </div>
          {!run.workflowValid ? <p className="text-xs text-muted-foreground">{t('conversation.runtime.workflowInvalid')}</p> : null}
        </div>
      ) : null}
      <ExecutionPlanSaveBar
        session={{ ...planSession, phase: saving ? 'saving' : planSession.phase }}
        onTargetChange={(target) => {
          writeExecutionPlanSaveTarget(target);
          setPlanSession({ ...planSession, preferredTarget: target });
        }}
        onSave={(target) => { void handlePlanSave(target); }}
        onSelectSide={(side) => {
          const session = planSessionRef.current ?? planSession;
          const live = editorSessionRef.current;
          const leaving = live
            ? { workflow: live.workflow, modelBindings: live.modelBindings }
            : { workflow: draft, modelBindings: editorDraft?.modelBindings ?? baselineModelBindings ?? EMPTY_MODEL_BINDINGS };
          const next = handoffExecutionPlanSide(session, leaving, side);
          commitPlanSession(next, !session.split || session.activeSide !== side);
        }}
        onSplit={() => {
          const session = planSessionRef.current ?? planSession;
          const live = editorSessionRef.current;
          const unified = live
            ? { workflow: live.workflow, modelBindings: live.modelBindings }
            : { workflow: draft, modelBindings: editorDraft?.modelBindings ?? baselineModelBindings ?? EMPTY_MODEL_BINDINGS };
          commitPlanSession(splitExecutionPlanSession(session, unified), true);
        }}
        onOpenUnsplit={() => setPlanSession(openExecutionPlanUnsplit(planSession))}
        onCancelUnsplit={() => setPlanSession(cancelExecutionPlanUnsplit(planSession))}
        onConfirmUnsplit={(side) => {
          const session = planSessionRef.current ?? planSession;
          const live = editorSessionRef.current;
          const leaving = live
            ? { workflow: live.workflow, modelBindings: live.modelBindings }
            : { workflow: draft, modelBindings: editorDraft?.modelBindings ?? baselineModelBindings ?? EMPTY_MODEL_BINDINGS };
          commitPlanSession(confirmExecutionPlanUnsplit(withActiveExecutionPlanDraft(session, leaving), side), true);
        }}
        onReload={() => setPlanReloadNonce((value) => value + 1)}
        onRecover={planOperationId ? () => { void handlePlanRecover(); } : undefined}
        dirty={dirty}
        onRevert={handleRevert}
        onViewIssues={editorLayout === 'compact' && planSession.phase === 'error' && planSession.errorCode === 'conversation.execution-plan.validation-failed'
          ? () => setInspectorRequestId((value) => value + 1)
          : undefined}
      />
      <div className="min-h-0 min-w-0 flex-1 overflow-hidden" data-workflow-editor-host="true">
        <WorkflowEditor
          className="h-full min-h-0"
          value={draft}
          modelBindings={editorDraft?.modelBindings ?? activeExecutionPlanDraft(planSession).modelBindings}
          agentRegistry={registry}
          profileCatalog={profileCatalog}
          saving={saving}
          showSaveAction={false}
          validationRequestId={repairMode ? 1 : 0}
          initialSessionDraft={editorDraft}
          onSessionDraftChange={handleEditorDraftChange}
          sessionSnapshotRef={editorSessionRef}
          draftScope={planSession.split ? planSession.activeSide : 'unified'}
          onLayoutChange={handleEditorLayout}
          focusInspectorRequest={inspectorRequestId}
          restoreRequestId={restoreRequestId}
        />
      </div>
    </div>
  );
}

function SystemPromptWorkspacePanel({ resource }: { resource: SystemPromptWorkspaceResource }) {
  const { t } = useTranslation();
  const [prompt, setPrompt] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    const locator = resource.locator;
    void getAcpSession(
      locator.projectId,
      locator.taskId,
      locator.runId,
      locator.roundId,
      locator.nodeId,
      locator.attemptId,
      { branchId: locator.branchId, pageSize: 1, eventLimit: 1 },
      null,
      locator.outerNodeId,
      locator.outerAttemptId,
    ).then((session) => {
      if (active) setPrompt(session?.systemPromptAppend ?? null);
    }).catch((reason) => {
      if (active) setError(displayAppError(t, reason));
    }).finally(() => {
      if (active) setLoading(false);
    });
    return () => { active = false; };
  }, [resource.key, t]);
  if (loading) return <WorkspaceLoadingState />;
  if (error) return <WorkspaceErrorState message={error} />;
  return <SystemPromptPanel prompt={prompt} />;
}

function HiddenPromptSectionWorkspacePanel({
  resource,
}: {
  resource: HiddenPromptSectionWorkspaceResource;
}) {
  const { t } = useTranslation();
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    const locator = resource.locator;
    void getAcpSession(
      locator.projectId,
      locator.taskId,
      locator.runId,
      locator.roundId,
      locator.nodeId,
      locator.attemptId,
      {
        branchId: locator.branchId,
        afterSeq: Math.max(0, locator.eventSeq - 1),
        eventLimit: 1,
        pageSize: 1,
      },
      null,
      locator.outerNodeId,
      locator.outerAttemptId,
    ).then((session) => {
      if (!active) return;
      const section = resolveGoldBandHiddenSection(session?.events ?? [], locator);
      if (section) {
        setContent(section.text);
      } else {
        setError(t('acp.hiddenPromptUnavailable'));
      }
    }).catch((reason) => {
      if (active) setError(displayAppError(t, reason));
    }).finally(() => {
      if (active) setLoading(false);
    });
    return () => { active = false; };
  }, [resource.key, t]);
  if (loading) return <WorkspaceLoadingState />;
  if (error) return <WorkspaceErrorState message={error} />;
  return (
    <SystemPromptPanel
      prompt={content}
      documentKey={resource.key}
      resourceKind={resource.kind}
      emptyMessage={t('acp.hiddenPromptUnavailable')}
    />
  );
}

function RawFramesWorkspacePanel({ resource }: { resource: RawFramesWorkspaceResource }) {
  const { t } = useTranslation();
  const [page, setPage] = useState<AcpRawFramePageVm | null>(null);
  const [query, setQuery] = useState<AcpRawFrameQueryInput>({ page: 0, pageSize: 100, order: 'desc' });
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async (nextQuery: AcpRawFrameQueryInput) => {
    const locator = resource.locator;
    setLoading(true);
    setError(null);
    try {
      const nextPage = await getAcpRawFrames(
        locator.projectId,
        locator.taskId,
        locator.runId,
        locator.roundId,
        locator.nodeId,
        locator.attemptId,
        nextQuery,
        locator.outerNodeId,
        locator.outerAttemptId,
      );
      setPage(nextPage);
      setQuery({
        page: nextPage.page,
        pageSize: nextPage.pageSize,
        search: nextPage.search ?? undefined,
        kind: nextPage.kind ?? undefined,
        direction: nextPage.direction ?? undefined,
        order: nextPage.order,
      });
    } catch (reason) {
      setError(displayAppError(t, reason));
    } finally {
      setLoading(false);
    }
  }, [resource.key, t]);
  useEffect(() => { void load({ page: 0, pageSize: 100, order: 'desc' }); }, [load]);
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden p-3" data-right-workspace-resource="raw-frames">
      {error ? <WorkspaceErrorState message={error} compact /> : null}
      <RawFrameViewer loading={loading} page={page} query={query} onQueryChange={(next) => void load(next)} />
    </div>
  );
}

function WorkspaceErrorState({ message, compact = false }: { message: string; compact?: boolean }) {
  return (
    <div className={compact
      ? 'mb-3 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive'
      : 'm-4 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive'}>
      {message}
    </div>
  );
}
