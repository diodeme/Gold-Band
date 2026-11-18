import { describe, expect, it } from 'vitest';
import ELK from 'elkjs/lib/elk.bundled.js';
import {
  layoutWorkflowGraph,
  runtimeGraphLayoutSpec,
  type WorkflowLayoutSpec,
} from '@/components/workflowGraph';
import { createAuthoringGraph } from '@/components/WorkflowEditor';
import type { GraphVm, WorkflowDsl } from '@/types';

const elk = new ELK();

/** The shipped default workflow template: a mixed split/single spine with
 * retry loops and a new-round terminal — the exact topology users reported
 * as "edges offset from the center line" in both directions. */
export const defaultWorkflow: WorkflowDsl = {
  version: '0.1',
  id: 'task-workflow',
  entry: 'interview',
  control: { max_attempts: 10, max_rounds: 3 },
  nodes: [
    { type: 'worker', id: 'interview', profile: 'pf-builtin-interview', goal: 'Clarify the requirement.' },
    { type: 'worker', id: 'plan', profile: 'pf-builtin-plan', goal: 'Produce a plan.', manual_check: true },
    { type: 'worker', id: 'dev', profile: 'pf-builtin-dev', goal: 'Implement the requirement.' },
    { type: 'worker', id: 'review', profile: 'pf-builtin-review', goal: 'Review.', output: { kind: 'json', artifact: 'review-result', schema: { reason: 'String', result: 'boolean' } }, success_condition: { expression: '$.result == true' } },
    { type: 'worker', id: 'test', profile: 'pf-builtin-test', goal: 'Test.', output: { kind: 'json', artifact: 'test-result', schema: { reason: 'String', result: 'boolean' } }, success_condition: { expression: '$.result == true' } },
    { type: 'worker', id: 'accept', profile: 'pf-builtin-accept', goal: 'Accept.', output: { kind: 'json', artifact: 'accept-result', schema: { reason: 'String', result: 'boolean' } }, success_condition: { expression: '$.result == true' } },
    { type: 'worker', id: 'cleanup', profile: 'pf-builtin-cleanup', goal: 'Clean up.' },
  ],
  edges: [
    { from: 'interview', to: 'plan', on: 'success' },
    { from: 'plan', to: 'dev', on: 'success' },
    { from: 'dev', to: 'review', on: 'success' },
    { from: 'review', to: 'test', on: 'success' },
    { from: 'review', to: 'dev', on: 'failure', session: 'continue' },
    { from: 'test', to: 'accept', on: 'success' },
    { from: 'test', to: 'dev', on: 'failure', session: 'continue' },
    { from: 'accept', to: 'cleanup', on: 'success' },
    { from: 'cleanup', to: '$end', on: 'success' },
    { from: 'accept', to: '$new-round', on: 'failure', new_round_entry: 'dev' },
  ],
};

const spine = ['interview', 'plan', 'dev', 'review', 'test', 'accept', 'cleanup'];

const successEdgeIds = [
  'interview-plan-0',
  'plan-dev-1',
  'dev-review-2',
  'review-test-3',
  'test-accept-5',
  'accept-cleanup-7',
];

async function assertAlignedSpine(spec: WorkflowLayoutSpec) {
  const layout = await layoutWorkflowGraph(spec, elk);
  const rects = spine.map((id) => layout.nodes.get(id)!);
  // Same track: every spine node shares one cross-axis coordinate (±1px).
  const cross = spec.direction === 'DOWN' ? rects.map((r) => r.x) : rects.map((r) => r.y);
  expect(Math.max(...cross) - Math.min(...cross), `${spec.direction} spine cross-axis`).toBeLessThanOrEqual(1);
  // Straight hops: every success edge is a two-point line now that all
  // main-chain ports share the success lane.
  for (const id of successEdgeIds) {
    const route = layout.edges.get(id);
    if (!route) continue;
    expect(route.points.length, `${spec.direction} ${id}`).toBeLessThanOrEqual(2);
  }
  return layout;
}

describe('workflow main-chain alignment through the production spec builders', () => {
  it('keeps the authoring spine straight in both directions', async () => {
    const hidden = new Set<string>();
    for (const direction of ['RIGHT', 'DOWN'] as const) {
      const graph = createAuthoringGraph(defaultWorkflow, hidden, (key) => key, direction);
      await assertAlignedSpine(graph.spec ?? graph as never);
    }
  });

  it('keeps the runtime execution spine straight in both directions', async () => {
    const graph: GraphVm = {
      nodes: [
        ...defaultWorkflow.nodes.map((node) => ({
          id: node.id,
          nodeId: node.id,
          label: node.id,
          nodeType: 'worker',
          runtimeDisplay: { code: '', tone: 'neutral', icon: 'dot', terminal: false, resumable: false, reasonCode: null, blockingError: false },
          artifactCount: 0,
          attachmentCount: 0,
          current: false,
        })),
        { id: '$end', label: 'end', nodeType: 'terminal', runtimeDisplay: { code: '', tone: 'neutral', icon: 'dot', terminal: true, resumable: false, reasonCode: null, blockingError: false }, artifactCount: 0, attachmentCount: 0, current: false },
        { id: '$new-round', label: 'new round', nodeType: 'terminal', runtimeDisplay: { code: '', tone: 'neutral', icon: 'dot', terminal: true, resumable: false, reasonCode: null, blockingError: false }, artifactCount: 0, attachmentCount: 0, current: false },
      ],
      edges: defaultWorkflow.edges.map((edge) => ({ from: edge.from, to: edge.to, label: edge.on })),
      groups: [],
    };
    for (const direction of ['RIGHT', 'DOWN'] as const) {
      const spec = runtimeGraphLayoutSpec(graph, (value) => value, direction);
      await assertAlignedSpine(spec);
    }
  });
});
