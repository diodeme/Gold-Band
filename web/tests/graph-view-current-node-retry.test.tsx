/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import i18n, { i18nInitialized } from '@/i18n';
import en from '@/locales/en.json';
import type { GraphNodeVm, GraphVm } from '@/types';

const reactFlowSnapshots = vi.hoisted(() => [] as Array<{
  nodes: Array<{ data: { node: GraphNodeVm; retryPending?: boolean } }>;
  onNodeContextMenu?: (event: { preventDefault: () => void }, node: unknown) => void;
}>);
const layoutState = vi.hoisted(() => ({
  layout: {
    direction: 'RIGHT' as const,
    nodes: new Map([
      ['worker', { x: 0, y: 0, width: 226, height: 138 }],
      ['accept', { x: 280, y: 0, width: 226, height: 138 }],
    ]),
    groups: new Map(),
    edges: new Map(),
    bounds: { x: 0, y: 0, width: 506, height: 138 },
  },
  pending: false,
  failed: false,
}));

vi.mock('@xyflow/react', async () => {
  const ReactModule = await import('react');
  return {
    ReactFlow: (props: unknown) => {
      reactFlowSnapshots.push(props as never);
      return ReactModule.createElement('div', { 'data-testid': 'react-flow' });
    },
    Background: () => null,
    Handle: () => null,
    BaseEdge: () => null,
    EdgeLabelRenderer: ({ children }: { children: React.ReactNode }) => children,
    getSmoothStepPath: () => ['', 0, 0],
    MarkerType: { ArrowClosed: 'arrowclosed' },
  };
});

vi.mock('@/hooks/useWorkflowLayout', () => ({
  useWorkflowLayout: () => layoutState,
}));

import { GraphView } from '@/components/GraphView';

const retryableNode: GraphNodeVm = {
  id: 'worker',
  roundId: 'round-001',
  nodeId: 'worker',
  label: 'Worker',
  nodeType: 'worker',
  status: 'paused',
  runtimeDisplay: { code: 'paused', tone: 'danger', icon: 'pause', terminal: false, resumable: true, reasonCode: 'runtime-abnormal', blockingError: false },
  attemptId: 'attempt-001',
  attemptCount: 1,
  attempts: [],
  artifactCount: 0,
  attachmentCount: 0,
  current: true,
  retryable: true,
};

const graph: GraphVm = {
  nodes: [
    retryableNode,
    { ...retryableNode, id: 'accept', nodeId: 'accept', label: 'Accept', current: false, retryable: false, attemptId: null },
  ],
  edges: [{ from: 'worker', to: 'accept', label: 'success' }],
  groups: [],
};

let root: Root | null = null;
let retryResolver: (() => void) | null = null;

function latestReactFlow() {
  return reactFlowSnapshots[reactFlowSnapshots.length - 1];
}

function flowNode(node: GraphNodeVm) {
  return { type: 'workflowNode', data: { node } };
}

function openContextMenu(node: GraphNodeVm) {
  act(() => {
    latestReactFlow().onNodeContextMenu?.({ preventDefault: vi.fn() }, flowNode(node));
  });
}

beforeEach(async () => {
  await i18nInitialized;
  if (!i18n.hasResourceBundle('en', 'translation')) {
    i18n.addResourceBundle('en', 'translation', en, true, true);
  }
  await i18n.changeLanguage('en');
  reactFlowSnapshots.length = 0;
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  const container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root?.unmount());
  root = null;
  retryResolver = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe('GraphView current-node retry', () => {
  it('reruns a historical failure even when the latest attempt succeeded', async () => {
    const onNodeRetry = vi.fn(() => new Promise<void>((resolve) => { retryResolver = resolve; }));
    const historical = { attemptId: 'attempt-001', status: 'completed', outcome: 'failure', runtimeDisplay: retryableNode.runtimeDisplay!, current: false, retryable: true };
    const succeeded = { ...retryableNode, status: 'completed', outcome: 'success', attemptId: 'attempt-002', retryable: false, attempts: [historical] };
    act(() => root!.render(<GraphView graph={{ ...graph, nodes: [succeeded] }} variant="actual" onNodeRetry={onNodeRetry} />));
    openContextMenu(succeeded);
    expect(document.body.textContent).not.toContain('Retry current node');
    const action = Array.from(document.querySelectorAll('button')).find((button) => button.textContent === 'Rerun from attempt-001');
    expect(action).toBeDefined();
    await act(async () => { action!.click(); });
    expect(onNodeRetry).toHaveBeenCalledWith(expect.objectContaining({ attemptId: 'attempt-001', status: 'completed', outcome: 'failure', retryable: true }));
    expect(latestReactFlow().nodes[0].data.retryPending).toBe(true);
    await act(async () => { retryResolver?.(); });
    expect(latestReactFlow().nodes[0].data.retryPending).toBe(false);
  });

  it('labels a completed failed node as a rerun', () => {
    const failed = { ...retryableNode, status: 'completed', outcome: 'failure' };
    act(() => root!.render(<GraphView graph={{ ...graph, nodes: [failed] }} variant="actual" onNodeRetry={vi.fn()} />));
    openContextMenu(failed);
    expect(document.body.textContent).toContain('Rerun from this node');
    expect(document.body.textContent).not.toContain('Retry current node');
  });
  it('shows retry only for the retryable current node and single-flights a pending request', async () => {
    const onNodeRetry = vi.fn(() => new Promise<void>((resolve) => {
      retryResolver = resolve;
    }));
    act(() => {
      root!.render(<GraphView graph={graph} variant="actual" onNodeRetry={onNodeRetry} />);
    });

    openContextMenu(graph.nodes[1]);
    expect(document.body.textContent).not.toContain('Retry current node');

    openContextMenu(retryableNode);
    const retryButton = Array.from(document.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('Retry current node'));
    expect(retryButton?.textContent).toContain('Retry current node');

    await act(async () => {
      retryButton?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(onNodeRetry).toHaveBeenCalledTimes(1);
    expect(onNodeRetry).toHaveBeenCalledWith(expect.objectContaining({
      roundId: 'round-001',
      nodeId: 'worker',
      attemptId: 'attempt-001',
    }));
    expect(latestReactFlow().nodes[0].data.retryPending).toBe(true);

    openContextMenu(retryableNode);
    const pendingRetryButton = Array.from(document.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('Retry current node'));
    expect(pendingRetryButton?.disabled).toBe(true);
    await act(async () => {
      pendingRetryButton?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    expect(onNodeRetry).toHaveBeenCalledTimes(1);

    await act(async () => {
      retryResolver?.();
      await Promise.resolve();
    });
    expect(onNodeRetry).toHaveBeenCalledTimes(1);
    expect(latestReactFlow().nodes[0].data.retryPending).toBe(false);
  });

  it('forwards the same retry callback into the nested fullscreen graph', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'web/src/components/GraphView.tsx'),
      'utf8',
    );
    expect(source).toContain('<GraphView');
    expect(source).toContain('onNodeRetry={onNodeRetry}');
  });
});
