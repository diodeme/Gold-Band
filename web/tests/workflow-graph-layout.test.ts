import { describe, expect, it } from 'vitest';
import { layoutSuccessPath } from "../src/components/workflowGraph";
import { workflowSuccessTopologyOrder } from "@/lib/workflow-validation";
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
  it('keeps acceptance successors in the forward layout across group handoffs', () => {
    const ids = ['bootstrap', 'b1', 'b2', 'a1', 'a2', 'a-merge', 'a-accept',
      'followup', 'b-merge', 'b-accept', 'c1', 'c2', 'c-merge', 'c-accept', 'final-check'];
    const pairs = [['bootstrap', 'b1'], ['bootstrap', 'b2'], ['b1', 'a1'], ['b1', 'a2'],
      ['a1', 'a-merge'], ['a2', 'a-merge'], ['a-merge', 'a-accept'], ['a-accept', 'followup'],
      ['followup', 'b-merge'], ['b2', 'b-merge'], ['b-merge', 'b-accept'],
      ['b-accept', 'c1'], ['b-accept', 'c2'], ['c1', 'c-merge'], ['c2', 'c-merge'],
      ['c-merge', 'c-accept'], ['c-accept', 'final-check']];
    const positions = layoutSuccessPath(ids.map((id) => ({ id, width: 260, height: 138 })),
      pairs.map(([from, to]) => ({ from, to })), new Set(ids));
    for (const [from, to] of pairs) {
      expect(positions.get(to)!.x - positions.get(from)!.x).toBeGreaterThan(260);
    }
    for (let i = 0; i < ids.length; i += 1) {
      for (const other of ids.slice(i + 1)) {
        const a = positions.get(ids[i])!;
        const b = positions.get(other)!;
        expect(Math.abs(a.x - b.x) >= 260 || Math.abs(a.y - b.y) >= 138).toBe(true);
      }
    }
  });

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
