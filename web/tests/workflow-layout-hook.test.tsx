/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ElkNode } from 'elkjs/lib/elk-api';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { WorkflowLayoutSpec } from '@/components/workflowGraph';

type PendingLayout = { graph: ElkNode; resolve: (result: ElkNode) => void };
const pendingLayouts: PendingLayout[] = [];

vi.mock('@/components/workflowLayoutEngine', () => ({
  workflowLayoutEngine: () => Promise.resolve({
    layout: (graph: ElkNode) => new Promise<ElkNode>((resolve) => pendingLayouts.push({ graph, resolve })),
  }),
}));

import { useWorkflowLayout, type WorkflowLayoutState } from '@/hooks/useWorkflowLayout';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function spec(nodeId: string): WorkflowLayoutSpec {
  return { nodes: [{ id: nodeId, width: 100, height: 40, sourceHandles: 'single' }], edges: [], groups: [] };
}

function laidOut(graph: ElkNode, x: number): ElkNode {
  return { ...graph, children: graph.children?.map((child) => ({ ...child, x, y: 0 })) };
}

let latest: WorkflowLayoutState | null = null;
function Probe({ value }: { value: WorkflowLayoutSpec }) {
  latest = useWorkflowLayout(value);
  return null;
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe('useWorkflowLayout', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    pendingLayouts.length = 0;
    latest = null;
    container = document.createElement('div');
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
  });

  it('keeps the previous drawing while a newer graph is pending and ignores superseded results', async () => {
    const first = spec('hook-first');
    const second = spec('hook-second');
    const third = spec('hook-third');

    act(() => root.render(<Probe value={first} />));
    await flush();
    expect(latest).toMatchObject({ layout: null, pending: true, failed: false });

    await act(async () => pendingLayouts[0].resolve(laidOut(pendingLayouts[0].graph, 10)));
    await flush();
    expect(latest?.pending).toBe(false);
    expect(latest?.layout?.nodes.get('hook-first')?.x).toBe(10);

    act(() => root.render(<Probe value={second} />));
    await flush();
    act(() => root.render(<Probe value={third} />));
    await flush();
    expect(latest?.pending).toBe(true);
    expect(latest?.layout?.nodes.has('hook-first')).toBe(true);

    const [secondRequest, thirdRequest] = pendingLayouts.slice(1);
    await act(async () => thirdRequest.resolve(laidOut(thirdRequest.graph, 30)));
    await flush();
    await act(async () => secondRequest.resolve(laidOut(secondRequest.graph, 20)));
    await flush();

    expect(latest?.pending).toBe(false);
    expect(latest?.layout?.nodes.get('hook-third')?.x).toBe(30);
    expect(latest?.layout?.nodes.has('hook-second')).toBe(false);
  });

  it('reuses a finished layout when the same graph is opened again', async () => {
    const value = spec('hook-cached');
    act(() => root.render(<Probe value={value} />));
    await flush();
    await act(async () => pendingLayouts[0].resolve(laidOut(pendingLayouts[0].graph, 5)));
    await flush();
    act(() => root.unmount());

    root = createRoot(container);
    act(() => root.render(<Probe value={spec('hook-cached')} />));
    expect(latest?.pending).toBe(false);
    expect(latest?.layout?.nodes.get('hook-cached')?.x).toBe(5);
    expect(pendingLayouts).toHaveLength(1);
  });
});
