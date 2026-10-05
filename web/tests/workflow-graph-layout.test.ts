import { describe, expect, it } from 'vitest';
import { workflowSuccessTopologyOrder } from '../src/components/workflowGraph';
import type { WorkflowDsl } from '../src/types';

function worker(id: string) {
  return {
    type: 'worker' as const,
    id,
    provider: 'claude-acp',
    profile: 'developer',
    goal: `Run ${id}`,
  };
}

function orderOf(workflow: WorkflowDsl) {
  return workflowSuccessTopologyOrder(workflow);
}

describe('workflowSuccessTopologyOrder', () => {
  it('places a newly prepended entry before older nodes even when it was appended to nodes', () => {
    const order = orderOf({
      version: '0.1',
      id: 'prepended-entry-layout',
      entry: 'plan',
      control: {},
      nodes: [worker('dev'), worker('accept'), worker('plan')],
      edges: [
        { from: 'plan', to: 'dev', on: 'success' },
        { from: 'dev', to: 'accept', on: 'success' },
        { from: 'accept', to: '$end', on: 'success' },
      ],
    });

    expect(order.get('plan')).toBeLessThan(order.get('dev')!);
    expect(order.get('dev')).toBeLessThan(order.get('accept')!);
  });

  it('keeps failure edges classified as backward branches against the success path', () => {
    const order = orderOf({
      version: '0.1',
      id: 'failure-branch-layout',
      entry: 'plan',
      control: {},
      nodes: [worker('plan'), worker('dev'), worker('review')],
      edges: [
        { from: 'plan', to: 'dev', on: 'success' },
        { from: 'dev', to: 'review', on: 'success' },
        { from: 'review', to: 'dev', on: 'failure' },
        { from: 'review', to: '$end', on: 'success' },
      ],
    });

    expect(order.get('review')).toBeGreaterThan(order.get('dev')!);
  });
});
