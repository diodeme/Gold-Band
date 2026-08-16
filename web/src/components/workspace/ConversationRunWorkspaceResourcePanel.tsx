import { useCallback, useEffect, useMemo, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getAcpRawFrames, getAcpSession, getAgentRegistry, getProfiles } from '@/api';
import { RawFrameViewer, SystemPromptPanel } from '@/components/acp/ACPChatDialog';
import { GraphView } from '@/components/GraphView';
import { StatusBadge } from '@/components/StatusBadge';
import { WorkflowEditor, parseWorkflowJson, type WorkflowEditorSessionDraft } from '@/components/WorkflowEditor';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import { displayAppError } from '@/i18n';
import { goldThemedScrollbarClassName } from '@/lib/themed-scrollbar';
import { workflowEditorSessionDraftIsDirty } from '@/lib/workflow-editor-session-draft';
import type {
  AcpRawFramePageVm,
  AcpRawFrameQueryInput,
  AgentRegistryVm,
  ConversationRunVm,
  GraphNodeVm,
  ProfileVm,
  WorkflowDsl,
  WorkflowModelBindings,
} from '@/types';
import {
  type RawFramesWorkspaceResource,
  type RightWorkspaceResource,
  type SystemPromptWorkspaceResource,
  type WorkflowEditWorkspaceResource,
  type WorkflowViewWorkspaceResource,
} from './right-workspace-context';

type ConversationRunWorkspaceResource =
  | WorkflowViewWorkspaceResource
  | WorkflowEditWorkspaceResource
  | SystemPromptWorkspaceResource
  | RawFramesWorkspaceResource;

interface ConversationRunWorkspaceResourcePanelProps {
  resource: ConversationRunWorkspaceResource;
  run: ConversationRunVm;
  agentRegistry: AgentRegistryVm | null;
  onSaveWorkflow?: (json: string, modelBindings: WorkflowModelBindings) => Promise<void>;
  onNodeOpenSession?: (node: GraphNodeVm) => void;
}

export function ConversationRunWorkspaceResourcePanel({
  resource,
  run,
  agentRegistry,
  onSaveWorkflow,
  onNodeOpenSession,
}: ConversationRunWorkspaceResourcePanelProps) {
  if (resource.kind === 'workflow-view') {
    return <WorkflowViewPanel run={run} onNodeOpenSession={onNodeOpenSession} />;
  }
  if (resource.kind === 'workflow-edit') {
    return (
      <WorkflowEditPanel
        resource={resource}
        run={run}
        initialAgentRegistry={agentRegistry}
        onSaveWorkflow={onSaveWorkflow}
      />
    );
  }
  if (resource.kind === 'system-prompt') {
    return <SystemPromptWorkspacePanel resource={resource} />;
  }
  return <RawFramesWorkspacePanel resource={resource} />;
}

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
  baselineSignature: string;
  draft: WorkflowDsl;
  editorDraft: WorkflowEditorSessionDraft;
}

const workflowDraftCache = new BoundedLruCache<string, WorkflowDraftCacheEntry>(24);

function workflowSignature(workflow: WorkflowDsl | null) {
  return workflow ? JSON.stringify(workflow) : '';
}

function workflowDraftIsDirty(entry: WorkflowDraftCacheEntry) {
  const baseline = entry.baselineSignature ? JSON.parse(entry.baselineSignature) as WorkflowDsl : null;
  return workflowEditorSessionDraftIsDirty(baseline, entry.editorDraft);
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

function WorkflowEditPanel({
  resource,
  run,
  initialAgentRegistry,
  onSaveWorkflow,
}: {
  resource: WorkflowEditWorkspaceResource;
  run: ConversationRunVm;
  initialAgentRegistry: AgentRegistryVm | null;
  onSaveWorkflow?: (json: string, modelBindings: WorkflowModelBindings) => Promise<void>;
}) {
  const { t } = useTranslation();
  const parsedWorkflow = useMemo(() => parseWorkflowJson(run.workflowJson), [run.workflowJson]);
  const initialBaselineSignature = workflowSignature(parsedWorkflow);
  const cached = workflowDraftCache.peek(resource.key);
  const [draft, setDraft] = useState<WorkflowDsl | null>(() => cached?.draft ?? parsedWorkflow);
  const [editorDraft, setEditorDraft] = useState<WorkflowEditorSessionDraft | null>(() => cached?.editorDraft ?? null);
  const [baselineSignature, setBaselineSignature] = useState(() => cached?.baselineSignature ?? initialBaselineSignature);
  const [registry, setRegistry] = useState(initialAgentRegistry);
  const [profiles, setProfiles] = useState<ProfileVm[]>([]);
  const [dependenciesLoading, setDependenciesLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const dirty = Boolean(draft) && (editorDraft
    ? workflowDraftIsDirty({ baselineSignature, draft: draft!, editorDraft })
    : workflowSignature(draft) !== baselineSignature);

  useEffect(() => {
    let active = true;
    Promise.all([
      registry ? Promise.resolve(registry) : getAgentRegistry().catch(() => ({ agents: [], catalog: [] })),
      getProfiles().then((result) => result.profiles).catch(() => []),
    ]).then(([nextRegistry, nextProfiles]) => {
      if (!active) return;
      setRegistry(nextRegistry);
      setProfiles(nextProfiles);
      setDependenciesLoading(false);
    });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (dirty || initialBaselineSignature === baselineSignature) return;
    setDraft(parsedWorkflow);
    setEditorDraft(null);
    setBaselineSignature(initialBaselineSignature);
    workflowDraftCache.delete(resource.key);
  }, [baselineSignature, dirty, initialBaselineSignature, parsedWorkflow, resource.key]);

  useEffect(() => {
    if (!dirty) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  const handleEditorDraftChange = useCallback((next: WorkflowEditorSessionDraft) => {
    setDraft(next.workflow);
    setEditorDraft(next);
    workflowDraftCache.set(resource.key, { baselineSignature, draft: next.workflow, editorDraft: next });
  }, [baselineSignature, resource.key]);

  const handleSave = useCallback(async (next: WorkflowDsl, modelBindings: WorkflowModelBindings) => {
    setSaving(true);
    try {
      await onSaveWorkflow?.(JSON.stringify(next), modelBindings);
      const signature = workflowSignature(next);
      setDraft(next);
      setBaselineSignature(signature);
      workflowDraftCache.delete(resource.key);
    } finally {
      setSaving(false);
    }
  }, [onSaveWorkflow, resource.key]);

  if (dependenciesLoading) return <WorkspaceLoadingState />;
  if (!draft) {
    return <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-muted-foreground">{t('common.empty')}</div>;
  }
  const repairMode = resource.mode === 'repair';
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-right-workspace-resource="workflow-edit">
      {repairMode ? (
        <div className="flex shrink-0 flex-col gap-2 border-b border-border/60 px-4 py-3">
          <div className="flex items-center gap-2">
            <StatusBadge value={run.workflowValid ? 'valid' : 'invalid'} label={run.workflowValid ? t('status.valid') : t('status.invalid')} />
          </div>
          {!run.workflowValid ? <p className="text-xs text-muted-foreground">{t('conversation.runtime.workflowInvalid')}</p> : null}
        </div>
      ) : null}
      <div className={goldThemedScrollbarClassName('min-h-0 flex-1 overflow-auto')}>
        <WorkflowEditor
          value={draft}
          agentRegistry={registry}
          profiles={profiles}
          saving={saving}
          validationRequestId={repairMode ? 1 : 0}
          initialSessionDraft={cached?.editorDraft ?? null}
          onSessionDraftChange={handleEditorDraftChange}
          onSave={handleSave}
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
    <div className={goldThemedScrollbarClassName('min-h-0 flex-1 overflow-y-auto p-3')} data-right-workspace-resource="raw-frames">
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
