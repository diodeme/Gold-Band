/** @vitest-environment jsdom */
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GraphVm } from '@/types';

const flow = vi.hoisted(() => ({ props: null as any, controls: null as any }));
vi.mock('@xyflow/react', async importOriginal => ({
  ...await importOriginal<typeof import('@xyflow/react')>(),
  ReactFlow: (props: any) => { flow.props = props; return <div>{props.children}</div>; },
  Background: () => null,
}));
vi.mock('@/components/GraphControls', () => ({ GraphControls: (props: any) => { flow.controls = props; return null; } }));
vi.mock('react-i18next', async importOriginal => ({
  ...await importOriginal<typeof import('react-i18next')>(),
  useTranslation: () => ({ t: (key: string) => key }),
}));
import { GraphView } from '@/components/GraphView';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let resized: ResizeObserverCallback;
let root: ReturnType<typeof createRoot> | undefined;
const graph = {
  nodes: Array.from({ length: 58 }, (_, index) => ({
    id: `node-${index}`, nodeId: `node-${index}`, sequence: index, label: `Node ${index}`,
    nodeType: 'worker', current: index === 57, artifactCount: 0, attachmentCount: 0,
    runtimeDisplay: { code: 'success', tone: 'success', icon: 'check' },
  })),
  edges: Array.from({ length: 57 }, (_, index) => ({ from: `node-${index}`, to: `node-${index + 1}`, label: 'success' })),
} as GraphVm;

async function mount(props: Record<string, unknown> = {}) {
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(390);
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(760);
  vi.stubGlobal('ResizeObserver', class {
    constructor(callback: ResizeObserverCallback) { resized = callback; }
    observe() {}
    disconnect() {}
  });
  const container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
  await act(async () => root!.render(<GraphView graph={graph} variant="actual" {...props} />));
}

afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.innerHTML = '';
});

describe('runtime graph reading navigation', () => {
  it('restores completed manual reading after the graph is remounted', async () => {
    let saved: unknown;
    await mount({ onReadingPositionChange: (position: unknown) => { saved = position; } });
    const viewport = { x: -250, y: -50, zoom: 1 };
    await act(async () => {
      flow.props.onMoveStart?.({ type: 'pointerdown' }, viewport);
      flow.props.onViewportChange(viewport);
      expect(saved).toBeUndefined();
      flow.props.onMoveEnd?.({ type: 'pointerup' }, viewport);
    });
    expect(saved).toBeDefined();
    await act(async () => root!.unmount());
    root = undefined;
    await mount({ initialReadingPosition: saved });
    expect(flow.props.viewport).toEqual(viewport);
  });
  it('initially shows the current node at a readable scale in a long graph', async () => {
    await mount();
    expect(flow.props.viewport.zoom).toBeGreaterThanOrEqual(0.8);
    const current = flow.props.nodes.find((node: any) => node.id === 'node-57');
    const { x, y, zoom } = flow.props.viewport;
    expect(current.position.x * zoom + x).toBeGreaterThanOrEqual(0);
    expect((current.position.x + 220) * zoom + x).toBeLessThanOrEqual(390);
    expect(current.position.y * zoom + y).toBeGreaterThanOrEqual(0);
  });

  it('keeps user zoom and logical center when its container resizes', async () => {
    await mount();
    const previous = { x: -250, y: -50, zoom: 1 };
    await act(async () => {
      flow.props.onMoveStart?.({ type: 'pointerdown' }, previous);
      flow.props.onViewportChange(previous);
    });
    await act(async () => resized([{ contentRect: { width: 780, height: 760 } } as ResizeObserverEntry], {} as ResizeObserver));
    expect(flow.props.viewport.zoom).toBe(1);
    expect(flow.props.viewport.x).toBe(previous.x + (780 - 390) / 2);
    expect(flow.props.viewport.y).toBe(previous.y);
  });

  it('allows a complete overview and returns to the current node', async () => {
    await mount();
    await act(async () => flow.controls.onFitView());
    expect(flow.props.viewport.zoom).toBeLessThan(0.1);
    for (const node of flow.props.nodes) {
      const { x, zoom } = flow.props.viewport;
      expect(node.position.x * zoom + x).toBeGreaterThanOrEqual(0);
      expect((node.position.x + 220) * zoom + x).toBeLessThanOrEqual(390);
    }
    await act(async () => flow.controls.onFocusNode());
    expect(flow.props.viewport.zoom).toBeGreaterThanOrEqual(0.8);
  });

  it('does not interrupt manual reading when execution status changes', async () => {
    await mount();
    const viewport = { x: -500, y: 100, zoom: 0.9 };
    await act(async () => {
      flow.props.onMoveStart({ type: 'pointerdown' }, viewport);
      flow.props.onViewportChange(viewport);
    });
    await act(async () => root!.render(<GraphView graph={{ ...graph, nodes: graph.nodes.map(node => ({ ...node, current: node.id === 'node-12' })) }} variant="actual" />));
    expect(flow.props.viewport).toEqual(viewport);
  });
});
