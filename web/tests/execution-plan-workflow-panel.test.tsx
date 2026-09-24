/** @vitest-environment jsdom */

import { useEffect, useState } from 'react';
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ConversationRunVm, ExecutionPlanSaveCommandVm, ExecutionPlanViewVm, WorkflowDsl, WorkflowEditorSessionDraft, WorkflowModelBindings, WorkflowVm } from '@/types';
import { clearExecutionPlanDraftCache, writeExecutionPlanDraft } from '@/lib/execution-plan-draft-cache';
import { createExecutionPlanSession, selectExecutionPlanSide, withActiveExecutionPlanDraft, type ExecutionPlanSession } from '@/lib/execution-plan-editor';

const editorProbe = vi.hoisted(() => ({ renders: 0 }));

const api = vi.hoisted(() => ({
  getAgentRegistry: vi.fn(),
  getProfiles: vi.fn(),
  getWorkflow: vi.fn(),
  getConversationExecutionPlan: vi.fn(),
  preflightConversationExecutionPlanSave: vi.fn(),
  saveConversationExecutionPlan: vi.fn(),
  recoverConversationExecutionPlanOperation: vi.fn(),
  getAcpRawFrames: vi.fn(),
  getAcpSession: vi.fn(),
}));

vi.mock('@/api', () => api);

vi.mock('react-i18next', async (importOriginal) => ({
  ...await importOriginal<typeof import('react-i18next')>(),
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/WorkflowEditor', () => ({
  parseWorkflowJson: (json: string) => JSON.parse(json) as WorkflowDsl,
  WorkflowEditor: ({
    value,
    modelBindings,
    sessionSnapshotRef,
    onSessionDraftChange,
    onLayoutChange,
    focusInspectorRequest = 0,
    restoreRequestId = 0,
  }: {
    value: WorkflowDsl;
    modelBindings?: WorkflowModelBindings;
    sessionSnapshotRef?: { current: WorkflowEditorSessionDraft | null };
    onSessionDraftChange?: (draft: WorkflowEditorSessionDraft) => void;
    onLayoutChange?: (layout: 'compact' | 'split') => void;
    focusInspectorRequest?: number;
    restoreRequestId?: number;
  }) => {
    editorProbe.renders += 1;
    const goalOf = (workflow: WorkflowDsl) => {
      const node = workflow.nodes[0] as { goal?: string };
      return node.goal ?? '';
    };
    const [goal, setGoal] = useState(goalOf(value));
    const publish = (workflow: WorkflowDsl) => {
      const draft: WorkflowEditorSessionDraft = {
        workflow,
        modelBindings: modelBindings ?? { definitionRevision: '', bindingRevision: 0, bindings: [] },
        tab: 'canvas',
        jsonDraft: JSON.stringify(workflow, null, 2),
      };
      if (sessionSnapshotRef) sessionSnapshotRef.current = draft;
      onSessionDraftChange?.(draft);
    };
    useEffect(() => {
      setGoal(goalOf(value));
      if (!sessionSnapshotRef) return;
      sessionSnapshotRef.current = {
        workflow: value,
        modelBindings: modelBindings ?? { definitionRevision: '', bindingRevision: 0, bindings: [] },
        tab: 'canvas',
        jsonDraft: JSON.stringify(value, null, 2),
      };
    }, [modelBindings, sessionSnapshotRef, value]);
    useEffect(() => {
      onLayoutChange?.('compact');
    }, [onLayoutChange]);
    const nodeIds = value.nodes.map((node) => node.id).join(',');
    const agentIds = (modelBindings?.bindings ?? []).map((binding) => `${binding.executionSlotId}:${binding.agentId}`).join(',');
    return (
      <div data-inspector-request={focusInspectorRequest} data-restore-request={restoreRequestId} data-node-ids={nodeIds} data-agent-ids={agentIds}>
        <span data-editor-goal={goal} />
        <button
          type="button"
          data-edit-goal
          onClick={() => {
            const nextGoal = `${goalOf(value)}-edited`;
            const next = {
              ...value,
              nodes: value.nodes.map((node, index) => index === 0 ? { ...node, goal: nextGoal } : node),
            };
            setGoal(nextGoal);
            publish(next);
          }}
        >
          edit
        </button>
      </div>
    );
  },
}));

vi.mock('@/components/acp/ACPChatDialog', () => ({
  RawFrameViewer: () => null,
  SystemPromptPanel: () => null,
}));

import { clearConversationRunWorkflowDraftCache, ConversationRunWorkspaceResourcePanel } from '@/components/workspace/ConversationRunWorkspaceResourcePanel';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const bindings: WorkflowModelBindings = { definitionRevision: '', bindingRevision: 0, bindings: [] };

function workflow(goal: string): WorkflowDsl {
  return {
    id: 'workflow-1',
    entry: 'worker-1',
    nodes: [{ type: 'worker', id: 'worker-1', executionSlotId: 'slot-1', goal }],
    edges: [],
    control: { maxAttempts: 1, maxRounds: 1 },
  };
}

const currentWorkflow = workflow('current-goal');
const nextWorkflow = workflow('next-goal');

const plan = {
  projectId: 'project-1',
  taskId: 'task-1',
  taskUuid: 'uuid-1',
  runId: 'run-1',
  runMode: 'workflow',
  runStatus: 'running',
  planRevision: 2,
  authoringRevision: 5,
  executionRevision: 3,
  currentEditable: true,
  diverged: true,
  currentWorkflow,
  currentModelBindings: bindings,
  nextWorkflow,
  nextModelBindings: bindings,
} as ExecutionPlanViewVm;
Object.assign(plan, {
  currentRound: 'round-001',
  currentNode: 'worker-1',
  currentAttempt: 'attempt-001',
});

const authoring = {
  workflowJson: JSON.stringify(nextWorkflow),
  modelBindings: bindings,
} as WorkflowVm;

function splitSession(): ExecutionPlanSession<{ workflow: WorkflowDsl; modelBindings: WorkflowModelBindings }> {
  return selectExecutionPlanSide(createExecutionPlanSession({
    current: { workflow: currentWorkflow, modelBindings: bindings },
    next: { workflow: nextWorkflow, modelBindings: bindings },
    planRevision: 2,
    authoringRevision: 5,
    executionRevision: 3,
    currentEditable: true,
    diverged: true,
    runStatus: 'running',
    currentRound: 'round-001',
    currentNode: 'worker-1',
    currentAttempt: 'attempt-001',
  }), 'current');
}

describe('workflow execution plan editor sides', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    clearExecutionPlanDraftCache();
    clearConversationRunWorkflowDraftCache();
    window.localStorage.clear();
    api.getProfiles.mockResolvedValue({ profiles: [] });
    api.getAgentRegistry.mockResolvedValue({ agents: [], catalog: [] });
    api.getWorkflow.mockResolvedValue(authoring);
    api.getConversationExecutionPlan.mockResolvedValue(plan);
    api.preflightConversationExecutionPlanSave.mockResolvedValue({
      planRevision: 2,
      authoringRevision: 5,
      executionRevision: 3,
      runStatus: 'running',
      currentEditable: true,
      diverged: true,
      blocking: [],
      affectedNodeIds: [],
      resumeIdentityRisks: [],
    });
    api.saveConversationExecutionPlan.mockResolvedValue({
      complete: true,
      planRevision: 3,
      authoringRevision: 5,
      executionRevision: 3,
      targets: [{ target: 'current', committed: true }],
    });
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  async function renderPanel(runId = 'run-1') {
    await act(async () => {
      root.render(
        <ConversationRunWorkspaceResourcePanel
          resource={{
            kind: 'workflow-edit',
            key: `workflow-edit:project-1:task-1:${runId}`,
            scopeKey: `conversation:project-1:task-1:${runId}`,
            title: 'Workflow',
            attention: false,
            mode: 'edit',
            locator: { projectId: 'project-1', taskId: 'task-1', runId },
          }}
          run={{
            projectId: 'project-1',
            taskId: 'task-1',
            taskUuid: 'uuid-1',
            runId,
            workflowValid: true,
          } as ConversationRunVm}
          agentRegistry={{ agents: [], catalog: [] }}
        />,
      );
    });
  }

  it('keeps an unpublished edit on the side being left', async () => {
    writeExecutionPlanDraft(
      { projectId: 'project-1', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-1' },
      splitSession(),
    );
    await renderPanel();
    expect(container.querySelector('[data-editor-goal]')?.getAttribute('data-editor-goal')).toBe('current-goal');
    const edit = container.querySelector<HTMLButtonElement>('[data-edit-goal]');
    await act(async () => edit?.click());
    expect(container.querySelector('[data-editor-goal]')?.getAttribute('data-editor-goal')).toBe('current-goal-edited');

    const activate = (label: string) => {
      const tab = [...container.querySelectorAll('[role="tab"]')].find((item) => item.textContent?.includes(label));
      expect(tab).toBeTruthy();
      return act(async () => {
        tab?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
      });
    };
    await activate('executionPlan.nextRun');
    expect(container.querySelector('[data-editor-goal]')?.getAttribute('data-editor-goal')).toBe('next-goal');
    await activate('executionPlan.currentRun');
    expect(container.querySelector('[data-editor-goal]')?.getAttribute('data-editor-goal')).toBe('current-goal-edited');
  });

  it('echoes the current locator from the execution plan view on save', async () => {
    await renderPanel();
    const save = [...container.querySelectorAll('button')].find((button) => button.textContent?.includes('executionPlan.save'));
    expect(save).toBeTruthy();
    await act(async () => save?.click());
    const command = api.preflightConversationExecutionPlanSave.mock.calls[0]?.[0] as ExecutionPlanSaveCommandVm;
    expect(command.expectedCurrentRound).toBe('round-001');
    expect(command.expectedCurrentNode).toBe('worker-1');
    expect(command.expectedCurrentAttempt).toBe('attempt-001');
  });

  it('keeps the open editor mounted when only the run lifecycle changes', async () => {
    const resource = {
      kind: 'workflow-edit' as const,
      key: 'workflow-edit:project-1:task-1:run-1',
      scopeKey: 'conversation:project-1:task-1:run-1',
      title: 'Workflow',
      attention: false,
      mode: 'edit' as const,
      locator: { projectId: 'project-1', taskId: 'task-1', runId: 'run-1' },
    };
    const registry = { agents: [], catalog: [] };
    const run = {
      projectId: 'project-1',
      taskId: 'task-1',
      taskUuid: 'uuid-1',
      runId: 'run-1',
      runStatus: 'running',
      workflowValid: true,
    } as ConversationRunVm;
    await act(async () => {
      root.render(<ConversationRunWorkspaceResourcePanel resource={resource} run={run} agentRegistry={registry} />);
    });
    const marker = container.querySelector('[data-editor-goal]');
    const rendersAfterLoad = editorProbe.renders;
    expect(marker).not.toBeNull();
    await act(async () => {
      root.render(
        <ConversationRunWorkspaceResourcePanel
          resource={resource}
          run={{ ...run, runStatus: 'paused' }}
          agentRegistry={registry}
        />,
      );
    });
    expect(editorProbe.renders).toBe(rendersAfterLoad);
    expect(container.querySelector('[data-editor-goal]')).toBe(marker);
    expect(container.textContent).not.toContain('common.loading');
  });

  it('does not replace the editor with a loading state when the agent registry object changes', async () => {
    await renderPanel();
    const marker = container.querySelector('[data-editor-goal]');
    expect(marker).not.toBeNull();
    await act(async () => {
      root.render(
        <ConversationRunWorkspaceResourcePanel
          resource={{
            kind: 'workflow-edit',
            key: 'workflow-edit:project-1:task-1:run-1',
            scopeKey: 'conversation:project-1:task-1:run-1',
            title: 'Workflow',
            attention: false,
            mode: 'edit',
            locator: { projectId: 'project-1', taskId: 'task-1', runId: 'run-1' },
          }}
          run={{
            projectId: 'project-1',
            taskId: 'task-1',
            taskUuid: 'uuid-1',
            runId: 'run-1',
            workflowValid: true,
          } as ConversationRunVm}
          agentRegistry={{ agents: [], catalog: [] }}
        />,
      );
    });
    expect(container.querySelector('[data-editor-goal]')).toBe(marker);
    expect(container.textContent).not.toContain('common.loading');
  });

  it('opens the inspector for a validation failure and restores the persisted workflow', async () => {
    api.preflightConversationExecutionPlanSave.mockResolvedValue({
      planRevision: 2,
      authoringRevision: 5,
      executionRevision: 3,
      runStatus: 'running',
      currentEditable: true,
      diverged: true,
      blocking: [
        { code: 'conversation.execution-plan.validation-failed', context: {} },
      ],
      affectedNodeIds: [],
      resumeIdentityRisks: [],
    });
    await renderPanel();
    const edit = container.querySelector<HTMLButtonElement>('[data-edit-goal]');
    await act(async () => edit?.click());
    const revert = container.querySelector<HTMLButtonElement>('[data-execution-plan-revert]');
    expect(revert?.textContent).toBe('executionPlan.revert');
    const save = [...container.querySelectorAll('button')].find((button) => button.textContent?.includes('executionPlan.save'));
    await act(async () => save?.click());
    const viewIssues = container.querySelector<HTMLButtonElement>('[data-execution-plan-view-issues]');
    expect(viewIssues).not.toBeNull();
    expect(container.textContent).not.toContain('executionPlan.reload');
    await act(async () => viewIssues?.click());
    expect(container.querySelector('[data-inspector-request]')?.getAttribute('data-inspector-request')).toBe('1');
    await act(async () => container.querySelector<HTMLButtonElement>('[data-execution-plan-revert]')?.click());
    expect(container.querySelector('[data-editor-goal]')?.getAttribute('data-editor-goal')).toBe('next-goal');
    expect(container.querySelector('[data-restore-request]')?.getAttribute('data-restore-request')).not.toBe('0');
    expect(container.querySelector('[data-execution-plan-revert]')).toBeNull();
  });

  it('shows the shared next workflow and its bindings after another run saves it', async () => {
    const shared = {
      ...workflow('dev'),
      entry: 'dev',
      nodes: [{ type: 'worker' as const, id: 'dev', executionSlotId: 'slot-dev', goal: 'dev' }],
    };
    const sharedBindings: WorkflowModelBindings = {
      definitionRevision: 'rev-2',
      bindingRevision: 2,
      bindings: [{ executionSlotId: 'slot-dev', agentId: 'codex' }],
    };
    const stale = {
      ...workflow('review'),
      entry: 'dev',
      nodes: [
        { type: 'worker' as const, id: 'dev', executionSlotId: 'slot-dev', goal: 'dev' },
        { type: 'worker' as const, id: 'review', executionSlotId: 'slot-review', goal: 'review' },
      ],
    };
    const staleBindings: WorkflowModelBindings = {
      definitionRevision: 'rev-1',
      bindingRevision: 1,
      bindings: [
        { executionSlotId: 'slot-dev', agentId: 'codex' },
        { executionSlotId: 'slot-review', agentId: 'claude' },
      ],
    };
    writeExecutionPlanDraft(
      { projectId: 'project-1', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-sync' },
      createExecutionPlanSession({
        current: { workflow: currentWorkflow, modelBindings: bindings },
        next: { workflow: stale, modelBindings: staleBindings },
        planRevision: 2,
        authoringRevision: 5,
        executionRevision: 3,
        currentEditable: true,
        diverged: false,
        runStatus: 'running',
        currentRound: 'round-001',
        currentNode: 'dev',
        currentAttempt: 'attempt-001',
      }),
    );
    api.getConversationExecutionPlan.mockResolvedValue({
      ...plan,
      authoringRevision: 8,
      diverged: true,
      nextWorkflow: shared,
      nextModelBindings: sharedBindings,
    });
    await renderPanel('run-sync');
    expect(container.querySelector('[data-node-ids]')?.getAttribute('data-node-ids')).toBe('dev');
    expect(container.querySelector('[data-agent-ids]')?.getAttribute('data-agent-ids')).toBe('slot-dev:codex');
  });

  it('keeps a dirty next draft graph and agent bindings together when shared authoring moves', async () => {
    const stale = {
      ...workflow('review'),
      nodes: [
        { type: 'worker' as const, id: 'dev', executionSlotId: 'slot-dev', goal: 'local' },
        { type: 'worker' as const, id: 'review', executionSlotId: 'slot-review', goal: 'review' },
      ],
    };
    const staleBindings: WorkflowModelBindings = {
      definitionRevision: 'rev-1',
      bindingRevision: 1,
      bindings: [
        { executionSlotId: 'slot-dev', agentId: 'codex' },
        { executionSlotId: 'slot-review', agentId: 'claude' },
      ],
    };
    const baselineNext = { workflow: workflow('server-before'), modelBindings: bindings };
    const session = withActiveExecutionPlanDraft(createExecutionPlanSession({
      current: { workflow: currentWorkflow, modelBindings: bindings },
      next: baselineNext,
      planRevision: 2,
      authoringRevision: 5,
      executionRevision: 3,
      currentEditable: true,
      diverged: true,
      runStatus: 'running',
      currentRound: 'round-001',
      currentNode: 'dev',
      currentAttempt: 'attempt-001',
    }), { workflow: stale, modelBindings: staleBindings });
    writeExecutionPlanDraft(
      { projectId: 'project-1', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-dirty' },
      session,
    );
    api.getConversationExecutionPlan.mockResolvedValue({
      ...plan,
      authoringRevision: 8,
      nextWorkflow: workflow('server-after'),
      nextModelBindings: {
        definitionRevision: 'rev-2',
        bindingRevision: 2,
        bindings: [{ executionSlotId: 'slot-1', agentId: 'codex' }],
      },
    });
    await renderPanel('run-dirty');
    expect(container.querySelector('[data-node-ids]')?.getAttribute('data-node-ids')).toBe('dev,review');
    expect(container.querySelector('[data-agent-ids]')?.getAttribute('data-agent-ids')).toBe('slot-dev:codex,slot-review:claude');
  });
});
