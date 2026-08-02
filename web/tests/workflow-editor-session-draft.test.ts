import { describe, expect, it } from 'vitest';
import { workflowEditorSessionDraftIsDirty } from '@/lib/workflow-editor-session-draft';
import type { WorkflowDsl } from '@/types';

const workflow: WorkflowDsl = {
  id: 'workflow-1',
  entry: 'node-1',
  nodes: [{ type: 'worker', id: 'node-1', provider: 'claude-acp', goal: 'Implement' }],
  edges: [],
  control: { maxAttempts: 1, maxRounds: 1 },
};

describe('workflow editor session draft', () => {
  it('keeps a canonical untouched draft clean', () => {
    expect(workflowEditorSessionDraftIsDirty(workflow, {
      workflow,
      tab: 'canvas',
      jsonDraft: JSON.stringify(workflow, null, 2),
    })).toBe(false);
  });

  it('treats invalid or uncommitted JSON text as an unsaved change', () => {
    expect(workflowEditorSessionDraftIsDirty(workflow, {
      workflow,
      tab: 'json',
      jsonDraft: '{ "id": "unfinished"',
    })).toBe(true);
  });

  it('detects semantic canvas changes even when JSON is canonical', () => {
    const changed = { ...workflow, nodes: [{ ...workflow.nodes[0], goal: 'Changed' }] } as WorkflowDsl;
    expect(workflowEditorSessionDraftIsDirty(workflow, {
      workflow: changed,
      tab: 'canvas',
      jsonDraft: JSON.stringify(changed, null, 2),
    })).toBe(true);
  });
});
