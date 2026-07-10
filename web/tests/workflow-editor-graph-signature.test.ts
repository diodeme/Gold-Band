import { describe, expect, it } from 'vitest';
import { authoringWorkflowGraphSignature } from '@/components/WorkflowEditor';
import type { WorkflowDsl, WorkflowWorkerNodeDsl } from '@/types';

function worker(id: string, patch: Partial<WorkflowWorkerNodeDsl> = {}): WorkflowWorkerNodeDsl {
  return {
    type: 'worker',
    id,
    provider: 'claude-acp',
    profile: 'developer',
    goal: `Run ${id}`,
    ...patch,
  };
}

function workflow(patch: Partial<WorkflowDsl> = {}): WorkflowDsl {
  return {
    version: '0.1',
    id: 'authoring-signature',
    entry: 'dev',
    control: {},
    nodes: [worker('dev'), worker('test')],
    edges: [
      { from: 'dev', to: 'test', on: 'success' },
      { from: 'test', to: '$end', on: 'success' },
    ],
    ...patch,
  };
}

describe('authoringWorkflowGraphSignature', () => {
  it('ignores inspector-only worker configuration so text input does not refresh the canvas projection', () => {
    const before = workflow();
    const after = workflow({
      control: { max_attempts: 3 },
      nodes: [
        worker('dev', {
          goal: 'A much longer draft goal typed in the inspector',
          model: 'gpt-5.4(xhigh)',
          profile: 'architect',
          permission_mode: 'ask',
          output: { kind: 'json', artifact: 'dev-result', schema: { type: 'object' } },
          success_condition: { expression: '$.ok == true' },
        }),
        worker('test'),
      ],
    });

    expect(authoringWorkflowGraphSignature(after)).toBe(authoringWorkflowGraphSignature(before));
  });

  it('changes when topology or canvas presentation fields change', () => {
    const before = workflow();
    const providerChanged = workflow({
      nodes: [worker('dev', { provider: 'codex-acp' }), worker('test')],
    });
    const edgeChanged = workflow({
      edges: [
        { from: 'dev', to: 'test', on: 'failure' },
        { from: 'test', to: '$end', on: 'success' },
      ],
    });

    expect(authoringWorkflowGraphSignature(providerChanged)).not.toBe(authoringWorkflowGraphSignature(before));
    expect(authoringWorkflowGraphSignature(edgeChanged)).not.toBe(authoringWorkflowGraphSignature(before));
  });
});
