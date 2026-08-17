/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ConversationRunVm, WorkflowDsl, WorkflowModelBindings, WorkflowVm } from '@/types';

const api = vi.hoisted(() => ({
  getAgentRegistry: vi.fn(),
  getProfiles: vi.fn(),
  getWorkflow: vi.fn(),
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

const workflowVm = {
  workflowJson: JSON.stringify(workflow),
  modelBindings,
} as WorkflowVm;

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
  WorkflowEditor: ({ value, modelBindings: bindings, onSave }: {
    value: WorkflowDsl;
    modelBindings: WorkflowModelBindings;
    onSave: (next: WorkflowDsl, bindings: WorkflowModelBindings) => Promise<void>;
  }) => (
    <button
      type="button"
      data-save-workflow
      data-workflow-id={value.id}
      data-agent-id={bindings.bindings[0]?.agentId}
      onClick={() => void onSave(value, bindings)}
    >
      Save
    </button>
  ),
}));

vi.mock('@/components/acp/ACPChatDialog', () => ({
  RawFrameViewer: () => null,
  SystemPromptPanel: () => null,
}));

import { ConversationRunWorkspaceResourcePanel } from '@/components/workspace/ConversationRunWorkspaceResourcePanel';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = '';
});

describe('conversation run workflow save contract', () => {
  it('forwards the editor model bindings with the serialized workflow', async () => {
    api.getProfiles.mockResolvedValue({ profiles: [] });
    api.getWorkflow.mockResolvedValue(workflowVm);
    const onSaveWorkflow = vi.fn().mockResolvedValue(workflowVm);
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
          run={{ projectId: 'project-1', taskId: 'task-1', workflowValid: true } as ConversationRunVm}
          agentRegistry={{ agents: [], catalog: [] }}
          onSaveWorkflow={onSaveWorkflow}
        />,
      );
    });

    const saveButton = container.querySelector<HTMLButtonElement>('[data-save-workflow]');
    expect(saveButton).not.toBeNull();
    expect(saveButton?.dataset.workflowId).toBe('workflow-1');
    expect(saveButton?.dataset.agentId).toBe('codex-acp');
    expect(api.getWorkflow).toHaveBeenCalledWith('task-1', 'project-1');
    await act(async () => saveButton?.click());

    expect(onSaveWorkflow).toHaveBeenCalledWith(JSON.stringify(workflow), modelBindings);
    await act(async () => root.unmount());
  });

  it('passes bindings to the Task save API before refreshing the conversation run', () => {
    const appSource = readFileSync(path.resolve(process.cwd(), 'web/src/App.tsx'), 'utf8');

    expect(appSource).toContain('onSaveWorkflow={async (json, modelBindings) => {');
    expect(appSource).toContain('const saved = await saveTaskWorkflow(conversationPage.projectId, conversationPage.taskId, dsl, modelBindings);');
    expect(appSource).toContain('return saved;');
  });
});
