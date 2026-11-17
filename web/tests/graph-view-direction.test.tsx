/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const reactFlowSnapshots = vi.hoisted(() => [] as Array<{
  nodes: Array<{ data: { direction: string } }>;
}>);
const layoutState = vi.hoisted(() => ({
  // Latest finished layout; a direction flip keeps returning the previous
  // layout until the worker resolves — exactly the pending window under test.
  layout: null as null | {
    direction: 'RIGHT' | 'DOWN';
    nodes: Map<string, { x: number; y: number; width: number; height: number }>;
    groups: Map<string, unknown>;
    edges: Map<string, unknown>;
    bounds: { x: number; y: number; width: number; height: number } | null;
  },
  pending: false,
  failed: false,
}));

function finishedLayout(direction: 'RIGHT' | 'DOWN') {
  return {
    direction,
    nodes: new Map([
      ['n1', { x: 0, y: 0, width: 226, height: 138 }],
      ['n2', direction === 'DOWN' ? { x: 0, y: 200, width: 226, height: 138 } : { x: 280, y: 0, width: 226, height: 138 }],
    ]),
    groups: new Map(),
    edges: new Map(),
    bounds: { x: 0, y: 0, width: direction === 'DOWN' ? 226 : 506, height: direction === 'DOWN' ? 338 : 138 },
  };
}

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
import { WorkflowGraphDirectionProvider } from '@/components/workflow-direction-preference';
import type { GraphVm } from '@/types';

const graph: GraphVm = {
  nodes: [
    { id: 'n1', label: 'Step one', nodeId: 'node-1', nodeType: 'agent', retryable: false, artifactCount: 0, attachmentCount: 0, current: false, runtimeDisplay: { code: '', tone: 'neutral', icon: 'dot', terminal: false, resumable: false, reasonCode: null, blockingError: false } },
    { id: 'n2', label: 'Step two', nodeId: 'node-2', nodeType: 'agent', retryable: false, artifactCount: 0, attachmentCount: 0, current: false, runtimeDisplay: { code: '', tone: 'neutral', icon: 'dot', terminal: false, resumable: false, reasonCode: null, blockingError: false } },
  ],
  edges: [{ from: 'n1', to: 'n2', label: 'success' }],
  groups: [],
};

let root: Root | null = null;

function renderWithPreference(preference: 'horizontal' | 'vertical') {
  act(() => {
    root!.render(
      <WorkflowGraphDirectionProvider value={{ preference, setPreference: () => {} }}>
        <GraphView graph={graph} variant="workflow" />
      </WorkflowGraphDirectionProvider>,
    );
  });
}

beforeEach(() => {
  reactFlowSnapshots.length = 0;
  layoutState.layout = null;
  layoutState.pending = false;
  layoutState.failed = false;
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
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

describe('GraphView layout direction flip', () => {
  it('keeps handles and anchors on the finished layout direction until the new layout lands', () => {
    layoutState.layout = finishedLayout('RIGHT');
    renderWithPreference('horizontal');
    expect(nodeDirections()).toEqual(['RIGHT', 'RIGHT']);

    // The preference flips instantly; the worker layout is still the old one.
    renderWithPreference('vertical');
    // Handles, node data and viewport anchors must not flip ahead of the
    // node positions they belong to.
    expect(nodeDirections()).toEqual(['RIGHT', 'RIGHT']);

    // The new layout resolves and the render direction follows it.
    layoutState.layout = finishedLayout('DOWN');
    renderWithPreference('vertical');
    expect(nodeDirections()).toEqual(['DOWN', 'DOWN']);
  });
});

function nodeDirections() {
  const snapshot = reactFlowSnapshots[reactFlowSnapshots.length - 1];
  return snapshot.nodes
    .filter((node) => 'direction' in node.data)
    .map((node) => node.data.direction);
}
