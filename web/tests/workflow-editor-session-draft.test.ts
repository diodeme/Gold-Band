import { describe, expect, it } from 'vitest';
import { workflowEditorSessionDraftIsDirty } from '@/lib/workflow-editor-session-draft';
import type { WorkflowDsl, WorkflowModelBindings } from '@/types';

const workflow: WorkflowDsl = {
  id: 'workflow-1',
  entry: 'node-1',
  nodes: [{ type: 'worker', id: 'node-1', provider: 'claude-acp', goal: 'Implement' }],
  edges: [],
  control: { maxAttempts: 1, maxRounds: 1 },
};

const modelBindings: WorkflowModelBindings = {
  definitionRevision: 'definition-1',
  bindingRevision: 1,
  bindings: [{ executionSlotId: 'slot-1', agentId: 'claude-acp' }],
};

describe('workflow editor session draft', () => {
  it('keeps a canonical untouched draft clean', () => {
    expect(workflowEditorSessionDraftIsDirty(workflow, modelBindings, {
      workflow,
      modelBindings,
      tab: 'canvas',
      jsonDraft: JSON.stringify(workflow, null, 2),
    })).toBe(false);
  });

  it('treats invalid or uncommitted JSON text as an unsaved change', () => {
    expect(workflowEditorSessionDraftIsDirty(workflow, modelBindings, {
      workflow,
      modelBindings,
      tab: 'json',
      jsonDraft: '{ "id": "unfinished"',
    })).toBe(true);
  });

  it('detects semantic canvas changes even when JSON is canonical', () => {
    const changed = { ...workflow, nodes: [{ ...workflow.nodes[0], goal: 'Changed' }] } as WorkflowDsl;
    expect(workflowEditorSessionDraftIsDirty(workflow, modelBindings, {
      workflow: changed,
      modelBindings,
      tab: 'canvas',
      jsonDraft: JSON.stringify(changed, null, 2),
    })).toBe(true);
  });

  it('detects model binding changes independently from the workflow definition', () => {
    expect(workflowEditorSessionDraftIsDirty(workflow, modelBindings, {
      workflow,
      modelBindings: {
        ...modelBindings,
        bindings: [{ executionSlotId: 'slot-1', agentId: 'codex-acp' }],
      },
      tab: 'canvas',
      jsonDraft: JSON.stringify(workflow, null, 2),
    })).toBe(true);
  });
});
