import { describe, expect, it } from 'vitest';
import {
  runtimeGraphEdgeClassName,
  runtimeGraphEdgeDisplayLabel,
  runtimeGraphLayoutSpec,
  workflowLayoutKey,
} from '@/components/workflowGraph';
import type { GraphNodeVm, GraphVm, RuntimeDisplayVm } from '@/types';

const neutralDisplay: RuntimeDisplayVm = {
  code: 'pending',
  tone: 'neutral',
  icon: 'dot',
  terminal: false,
  resumable: false,
  blockingError: false,
};

function node(id: string, patch: Partial<GraphNodeVm> = {}): GraphNodeVm {
  return {
    id,
    nodeId: id,
    sequence: patch.sequence ?? 0,
    label: id,
    nodeType: 'worker',
    runtimeDisplay: neutralDisplay,
    artifactCount: 0,
    attachmentCount: 0,
    current: false,
    ...patch,
  };
}

function graph(patch: Partial<GraphVm> = {}): GraphVm {
  return {
    nodes: [node('dev', { sequence: 1 }), node('test', { sequence: 2 })],
    edges: [{ from: 'dev', to: 'test', label: 'success' }],
    groups: [],
    ...patch,
  };
}

const layoutKey = (value: GraphVm) => workflowLayoutKey(runtimeGraphLayoutSpec(value, (label) => label));

describe('runtime graph layout key', () => {
  it('ignores runtime-only node state so status refreshes do not rerun layout', () => {
    const before = graph();
    const after = graph({
      nodes: [
        node('dev', {
          sequence: 1,
          status: 'running',
          current: true,
          runtimeDisplay: {
            ...neutralDisplay,
            code: 'running',
            tone: 'running',
          },
        }),
        node('test', { sequence: 2, status: 'pending' }),
      ],
    });

    expect(layoutKey(after)).toBe(layoutKey(before));
  });

  it('ignores traversal counts so repeated loops do not rerun layout', () => {
    const before = graph();
    const after = graph({ edges: [{ from: 'dev', to: 'test', label: 'success', traversalCount: 4 }] });

    expect(layoutKey(after)).toBe(layoutKey(before));
  });

  it('changes when edge labels change because success and failure edges affect layout', () => {
    const before = graph();
    const after = graph({
      edges: [{ from: 'dev', to: 'test', label: 'failure' }],
    });

    expect(layoutKey(after)).not.toBe(layoutKey(before));
  });

  it('changes when dynamic group containment changes', () => {
    const before = graph();
    const after = graph({
      nodes: [node('dev', { sequence: 1, dynamicGroupId: 'g1' }), node('test', { sequence: 2 })],
      groups: [{ id: 'g1', parentGroupId: null }],
    });

    expect(layoutKey(after)).not.toBe(layoutKey(before));
  });
});

describe('runtime graph edge presentation', () => {
  const translate = (value: string) => ({ success: '成功', failure: '失败' })[value] ?? value;

  it('keeps localized success and failure labels visible', () => {
    expect(runtimeGraphEdgeDisplayLabel({
      label: 'success',
    }, translate)).toBe('成功');
    expect(runtimeGraphEdgeDisplayLabel({
      label: 'failure',
    }, translate)).toBe('失败');
  });

  it('keeps traversal and blocked edge details attached to the localized label', () => {
    expect(runtimeGraphEdgeDisplayLabel({
      label: 'failure',
      traversalCount: 3,
    }, translate)).toBe('失败 ×3');
    expect(runtimeGraphEdgeDisplayLabel({
      label: 'failure',
      blockedReason: { reasonKind: 'limit', title: 'limit', message: 'limit', proposedCount: 2, limit: 1 },
    }, translate)).toBe('失败 · 2/1');
  });

  it('keeps flow and running edge classes together for CSS-only animation', () => {
    expect(runtimeGraphEdgeClassName(false, false)).toBe('workflow-edge-flow');
    expect(runtimeGraphEdgeClassName(false, true)).toBe('workflow-edge-flow workflow-edge-branch');
    expect(runtimeGraphEdgeClassName(true, false)).toBe('workflow-edge-flow workflow-edge-running');
  });
});
