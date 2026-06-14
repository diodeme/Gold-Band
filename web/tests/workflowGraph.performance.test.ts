import { describe, expect, it } from 'vitest';
import { runtimeGraphTopologySignature } from '@/components/workflowGraph';
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
    ...patch,
  };
}

describe('runtime graph topology signature', () => {
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

    expect(runtimeGraphTopologySignature(before, 'actual')).toBe(
      runtimeGraphTopologySignature(after, 'actual'),
    );
  });

  it('changes when edge labels change because success and failure edges affect layout', () => {
    const before = graph();
    const after = graph({
      edges: [{ from: 'dev', to: 'test', label: 'failure' }],
    });

    expect(runtimeGraphTopologySignature(before, 'actual')).not.toBe(
      runtimeGraphTopologySignature(after, 'actual'),
    );
  });
});
