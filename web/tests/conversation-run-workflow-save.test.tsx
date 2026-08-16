/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ConversationRunVm, WorkflowDsl, WorkflowModelBindings } from '@/types';

const api = vi.hoisted(() => ({
  getAgentRegistry: vi.fn(),
  getProfiles: vi.fn(),
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
  WorkflowEditor: ({ onSave }: {
    onSave: (next: WorkflowDsl, bindings: WorkflowModelBindings) => Promise<void>;
  }) => (
    <button type="button" data-save-workflow onClick={() => void onSave(workflow, modelBindings)}>
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
    const onSaveWorkflow = vi.fn().mockResolvedValue(undefined);
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
          run={{ workflowJson: JSON.stringify(workflow), workflowValid: true } as ConversationRunVm}
          agentRegistry={{ agents: [], catalog: [] }}
          onSaveWorkflow={onSaveWorkflow}
        />,
      );
    });

    const saveButton = container.querySelector<HTMLButtonElement>('[data-save-workflow]');
    expect(saveButton).not.toBeNull();
    await act(async () => saveButton?.click());

    expect(onSaveWorkflow).toHaveBeenCalledWith(JSON.stringify(workflow), modelBindings);
    await act(async () => root.unmount());
  });

  it('passes bindings to the Task save API before refreshing the conversation run', () => {
    const appSource = readFileSync(path.resolve(process.cwd(), 'web/src/App.tsx'), 'utf8');

    expect(appSource).toContain('onSaveWorkflow={async (json, modelBindings) => {');
    expect(appSource).toContain(
      'await saveTaskWorkflow(conversationPage.projectId, conversationPage.taskId, dsl, modelBindings);',
    );
  });
});
