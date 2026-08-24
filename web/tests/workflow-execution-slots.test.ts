import { describe, expect, it, vi } from 'vitest';
import { createAuthoringWorkerNode, normalizeWorkflowExecutionSlots, upsertWorkerModelBinding } from '../src/components/WorkflowEditor';
import type { WorkflowDsl, WorkflowWorkerNodeDsl } from '../src/types';

function workflow(nodes: WorkflowWorkerNodeDsl[]): WorkflowDsl {
  return {
    version: '0.1',
    id: 'workflow',
    entry: nodes[0]?.id ?? '',
    control: {},
    nodes,
    edges: [],
  };
}

describe('Workflow JSON execution slots', () => {
  it('creates every authoring worker with a stable slot so Agent selection can bind immediately', () => {
    const current = workflow([{ type: 'worker', id: 'existing', executionSlotId: 'slot-existing' }]);
    const node = createAuthoringWorkerNode(current, 'node-2', () => 'slot-new');

    expect(node).toMatchObject({ type: 'worker', id: 'node-2', executionSlotId: 'slot-new' });
    expect(upsertWorkerModelBinding(
      { definitionRevision: '', bindingRevision: 0, bindings: [] },
      node.executionSlotId!,
      { agentId: 'claude-acp' },
    ).bindings).toEqual([{ executionSlotId: 'slot-new', agentId: 'claude-acp' }]);
  });

  it('keeps generated slot identity when the node id is unique-suffixed', () => {
    const current = workflow([{ type: 'worker', id: 'node-2', executionSlotId: 'slot-existing' }]);
    const node = createAuthoringWorkerNode(current, 'node-2', () => 'slot-new');

    expect(node.id).toBe('node-2-2');
    expect(node.executionSlotId).toBe('slot-new');
  });

  it('reuses slots by node id and creates a slot only for a new worker', () => {
    const previous = workflow([
      { type: 'worker', id: 'existing', executionSlotId: 'slot-existing' },
    ]);
    const next = workflow([
      { type: 'worker', id: 'existing' },
      { type: 'worker', id: 'new-worker' },
    ]);
    const createSlotId = vi.fn(() => 'slot-new');

    const normalized = normalizeWorkflowExecutionSlots(next, previous, createSlotId);

    expect(normalized.nodes).toEqual([
      { type: 'worker', id: 'existing', executionSlotId: 'slot-existing' },
      { type: 'worker', id: 'new-worker', executionSlotId: 'slot-new' },
    ]);
    expect(createSlotId).toHaveBeenCalledTimes(1);
  });

  it('keeps generated slots stable across later JSON changes', () => {
    const next = workflow([{ type: 'worker', id: 'new-worker' }]);
    const createSlotId = vi.fn(() => 'slot-new');
    const first = normalizeWorkflowExecutionSlots(next, workflow([]), createSlotId);

    const second = normalizeWorkflowExecutionSlots(next, first, createSlotId);

    expect(second.nodes[0]).toMatchObject({ executionSlotId: 'slot-new' });
    expect(createSlotId).toHaveBeenCalledTimes(1);
  });

  it('preserves duplicate non-empty slots so validation can reject them', () => {
    const next = workflow([
      { type: 'worker', id: 'first', executionSlotId: 'duplicate-slot' },
      { type: 'worker', id: 'second', executionSlotId: 'duplicate-slot' },
    ]);
    const createSlotId = vi.fn(() => 'unused');

    const normalized = normalizeWorkflowExecutionSlots(next, workflow([]), createSlotId);

    expect(normalized.nodes.map((node) => node.executionSlotId)).toEqual([
      'duplicate-slot',
      'duplicate-slot',
    ]);
    expect(createSlotId).not.toHaveBeenCalled();
  });
});
