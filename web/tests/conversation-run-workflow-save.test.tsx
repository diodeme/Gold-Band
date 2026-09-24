/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ConversationRunVm, ExecutionPlanSaveResultVm, ExecutionPlanViewVm, WorkflowDsl, WorkflowModelBindings } from '@/types';

const api = vi.hoisted(() => ({
  getAgentRegistry: vi.fn(),
  getProfiles: vi.fn(),
  getConversationExecutionPlan: vi.fn(),
  preflightConversationExecutionPlanSave: vi.fn(),
  saveConversationExecutionPlan: vi.fn(),
  recoverConversationExecutionPlanOperation: vi.fn(),
}));

const workflow: WorkflowDsl = {
  id: 'workflow-1',
  entry: 'worker-1',
  nodes: [{ type: 'worker', id: 'worker-1', executionSlotId: 'slot-1', goal: 'Implement' }],
  edges: [],
  control: { maxAttempts: 1, maxRounds: 1 },
};

const modelBindings: WorkflowModelBindings = {
  definitionRevision: 'definition-1',
  bindingRevision: 3,
  bindings: [{
    executionSlotId: 'slot-1',
    agentId: 'codex-acp',
    modelId: 'gpt-5',
    permissionModeId: 'workspace-write',
    configOptions: { reasoning_effort: 'high' },
  }],
};

const plan: ExecutionPlanViewVm = {
  runMode: 'workflow',
  runStatus: 'running',
  planRevision: 4,
  authoringRevision: 2,
  executionRevision: 1,
  currentEditable: true,
  diverged: false,
  currentWorkflow: workflow,
  currentModelBindings: modelBindings,
  nextWorkflow: workflow,
  nextModelBindings: modelBindings,
  currentAutoConfig: null,
  nextAutoConfig: null,
} as ExecutionPlanViewVm;

const saved: ExecutionPlanSaveResultVm = {
  operationId: 'op-1',
  complete: true,
  planRevision: 5,
  authoringRevision: 3,
  executionRevision: 1,
  targets: [{ target: 'both', committed: true, error: null }],
} as ExecutionPlanSaveResultVm;

vi.mock('@/api', () => ({
  ...api,
  getAcpRawFrames: vi.fn(),
  getAcpSession: vi.fn(),
}));

vi.mock('react-i18next', async (importOriginal) => ({
  ...await importOriginal<typeof import('react-i18next')>(),
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/WorkflowEditor', () => ({
  parseWorkflowJson: (json: string) => JSON.parse(json),
  WorkflowEditor: ({ className, value, modelBindings: bindings, showSaveAction }: {
    className?: string;
    value: WorkflowDsl;
    modelBindings: WorkflowModelBindings;
    showSaveAction?: boolean;
  }) => (
    <div
      data-workflow-editor
      className={className}
      data-workflow-id={value.id}
      data-agent-id={bindings.bindings[0]?.agentId}
      data-show-save-action={String(showSaveAction)}
    />
  ),
}));

vi.mock('@/components/acp/ACPChatDialog', () => ({
  RawFrameViewer: () => null,
  SystemPromptPanel: () => null,
}));

import { ConversationRunWorkspaceResourcePanel } from '@/components/workspace/ConversationRunWorkspaceResourcePanel';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
  window.localStorage.clear();
  window.sessionStorage.clear();
});

afterEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = '';
});

describe('conversation run workflow save contract', () => {
  it('saves the editor draft through the execution plan and refreshes the run once committed', async () => {
    api.getProfiles.mockResolvedValue({ profiles: [] });
    api.getConversationExecutionPlan.mockResolvedValue(plan);
    api.preflightConversationExecutionPlanSave.mockResolvedValue({ blocking: [], warnings: [] });
    api.saveConversationExecutionPlan.mockResolvedValue(saved);
    const onExecutionPlanSaved = vi.fn().mockResolvedValue(undefined);
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);

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
          run={{ projectId: 'project-1', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-1', workflowValid: true } as ConversationRunVm}
          agentRegistry={{ agents: [], catalog: [] }}
          onExecutionPlanSaved={onExecutionPlanSaved}
        />,
      );
    });

    // The execution plan is the single source for the editor baseline; the legacy
    // project-authoring read and the editor's own save button are gone.
    expect(api.getConversationExecutionPlan).toHaveBeenCalledWith('project-1', 'task-1', 'uuid-1', 'run-1');
    const editor = container.querySelector<HTMLElement>('[data-workflow-editor]');
    expect(editor?.dataset.workflowId).toBe('workflow-1');
    expect(editor?.dataset.agentId).toBe('codex-acp');
    expect(editor?.dataset.showSaveAction).toBe('false');

    const saveBar = container.querySelector('[data-execution-plan-save-bar]');
    expect(saveBar?.getAttribute('data-execution-plan-save-bar')).toBe('ready');
    const saveButton = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent === 'executionPlan.save');
    expect(saveButton).toBeDefined();
    await act(async () => saveButton?.click());

    const command = api.saveConversationExecutionPlan.mock.calls[0]?.[0];
    expect(api.preflightConversationExecutionPlanSave).toHaveBeenCalledWith(command);
    expect(command).toMatchObject({
      projectId: 'project-1',
      taskId: 'task-1',
      taskUuid: 'uuid-1',
      runId: 'run-1',
      expectedPlanRevision: 4,
      expectedAuthoringRevision: 2,
      expectedRunStatus: 'running',
      workflow: { workflow, modelBindings },
    });
    expect(onExecutionPlanSaved).toHaveBeenCalledWith(saved);
    await act(async () => root.unmount());
  });

  it('fills the workspace below the save bar instead of a viewport-sized editor', async () => {
    api.getProfiles.mockResolvedValue({ profiles: [] });
    api.getConversationExecutionPlan.mockResolvedValue(plan);
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);

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
          run={{ projectId: 'project-1', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-1', workflowValid: true } as ConversationRunVm}
          agentRegistry={{ agents: [], catalog: [] }}
        />,
      );
    });

    const editor = container.querySelector<HTMLElement>('[data-workflow-editor]');
    const host = editor?.parentElement;
    expect(editor?.className).toContain('h-full');
    expect(editor?.className).toContain('min-h-0');
    expect(host?.className).toContain('min-h-0');
    expect(host?.className).toContain('overflow-hidden');
    expect(host?.className).not.toContain('overflow-auto');
    await act(async () => root.unmount());
  });

  it('refreshes the conversation run projection after a committed execution-plan save', () => {
    const appSource = readFileSync(path.resolve(process.cwd(), 'web/src/App.tsx'), 'utf8');

    expect(appSource).toContain('onExecutionPlanSaved={async () => {');
    expect(appSource).not.toContain('onSaveWorkflow={async (json, modelBindings) => {');
    expect(appSource).toContain("applyConversationRunSnapshot(refreshed, 'workflow-save', {");
  });
});
