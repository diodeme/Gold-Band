import ELK from 'elkjs/lib/elk.bundled.js';
import { describe, expect, it } from 'vitest';
import {
  NEW_ROUND_NODE,
  NODE_HEIGHT,
  NODE_WIDTH,
  END_NODE,
  TERMINAL_NODE_HEIGHT,
  WORKFLOW_EDGE_LABEL_HEIGHT,
  layoutWorkflowGraph,
  type Point,
  type Rect,
  type WorkflowLayout,
  type WorkflowLayoutSpec,
} from '../src/components/workflowGraph';

const elk = new ELK();

function node(id: string, patch: Partial<WorkflowLayoutSpec['nodes'][number]> = {}): WorkflowLayoutSpec['nodes'][number] {
  return { id, width: NODE_WIDTH, height: NODE_HEIGHT, sourceHandles: 'split', ...patch };
}

function terminal(id: string, pinBelow: string | null = null): WorkflowLayoutSpec['nodes'][number] {
  return { id, width: 140, height: TERMINAL_NODE_HEIGHT, sourceHandles: 'none', pinBelow };
}

function edge(from: string, to: string, outcome: 'success' | 'failure', index: number): WorkflowLayoutSpec['edges'][number] {
  return { id: `${from}-${to}-${index}`, from, to, outcome, labelWidth: 68 };
}

/** Default full workflow shape: a success spine with retry loops and a new Round from acceptance. */
const defaultWorkflowSpec: WorkflowLayoutSpec = {
  nodes: [node('plan'), node('dev'), node('review'), node('test'), node('accept'), node('cleanup', { sourceHandles: 'single' }), terminal(END_NODE), terminal(NEW_ROUND_NODE, 'plan')],
  edges: [
    edge('plan', 'dev', 'success', 0),
    edge('dev', 'review', 'success', 1),
    edge('review', 'test', 'success', 2),
    edge('review', 'dev', 'failure', 3),
    edge('test', 'accept', 'success', 4),
    edge('test', 'dev', 'failure', 5),
    edge('accept', 'cleanup', 'success', 6),
    edge('accept', NEW_ROUND_NODE, 'failure', 7),
    edge('cleanup', END_NODE, 'success', 8),
  ],
  groups: [],
};

function overlaps(a: Rect, b: Rect) {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

function segmentCrossesRect(a: Point, b: Point, rect: Rect) {
  const inset = 1;
  const left = rect.x + inset;
  const right = rect.x + rect.width - inset;
  const top = rect.y + inset;
  const bottom = rect.y + rect.height - inset;
  if (a.y === b.y) return a.y > top && a.y < bottom && Math.max(a.x, b.x) > left && Math.min(a.x, b.x) < right;
  return a.x > left && a.x < right && Math.max(a.y, b.y) > top && Math.min(a.y, b.y) < bottom;
}

function distanceToRoute(point: Point, points: Point[]) {
  let best = Number.POSITIVE_INFINITY;
  for (let i = 1; i < points.length; i += 1) {
    const [a, b] = [points[i - 1], points[i]];
    const x = Math.min(Math.max(point.x, Math.min(a.x, b.x)), Math.max(a.x, b.x));
    const y = Math.min(Math.max(point.y, Math.min(a.y, b.y)), Math.max(a.y, b.y));
    best = Math.min(best, Math.hypot(point.x - x, point.y - y));
  }
  return best;
}

function expectReadableLayout(spec: WorkflowLayoutSpec, layout: WorkflowLayout) {
  const rects = [...layout.nodes.entries()];
  expect(rects.map(([id]) => id).sort()).toEqual(spec.nodes.map((item) => item.id).sort());
  for (let i = 0; i < rects.length; i += 1) {
    for (const [otherId, other] of rects.slice(i + 1)) {
      expect(overlaps(rects[i][1], other), `${rects[i][0]} overlaps ${otherId}`).toBe(false);
    }
  }
  for (const item of spec.edges) {
    const route = layout.edges.get(item.id);
    expect(route, item.id).toBeDefined();
    if (item.labelWidth) {
      expect(distanceToRoute({ x: route!.labelX, y: route!.labelY }, route!.points), `${item.id} label`).toBeLessThanOrEqual(WORKFLOW_EDGE_LABEL_HEIGHT);
    }
    for (let i = 1; i < route!.points.length; i += 1) {
      for (const [id, rect] of rects) {
        if (id === item.from || id === item.to) continue;
        expect(segmentCrossesRect(route!.points[i - 1], route!.points[i], rect), `${item.id} crosses ${id}`).toBe(false);
      }
    }
  }
}

describe('ELK workflow layout', () => {
  it('lays out the default workflow without overlaps or edges through nodes', async () => {
    const layout = await layoutWorkflowGraph(defaultWorkflowSpec, elk);

    expectReadableLayout(defaultWorkflowSpec, layout);
    const spine = ['plan', 'dev', 'review', 'test', 'accept', 'cleanup', END_NODE].map((id) => layout.nodes.get(id)!);
    for (let i = 1; i < spine.length; i += 1) expect(spine[i].x).toBeGreaterThan(spine[i - 1].x + spine[i - 1].width);
    // Target handles share the success handle height, so success hops are straight lines.
    for (const id of ['plan-dev-0', 'dev-review-1', 'review-test-2', 'test-accept-4']) {
      expect(layout.edges.get(id)!.points, id).toHaveLength(2);
    }
    expect(new Set(['plan', 'dev', 'review', 'test', 'accept'].map((id) => layout.nodes.get(id)!.y)).size).toBe(1);
  });

  it('pins the new Round target directly below the entry so it is visible on first view', async () => {
    const layout = await layoutWorkflowGraph(defaultWorkflowSpec, elk);
    const entry = layout.nodes.get('plan')!;
    const newRound = layout.nodes.get(NEW_ROUND_NODE)!;
    const firstSuccessor = layout.nodes.get('dev')!;

    expect(newRound.y).toBeGreaterThanOrEqual(entry.y + entry.height);
    expect(newRound.x + newRound.width).toBeLessThanOrEqual(firstSuccessor.x);
    expect(newRound.x).toBeLessThanOrEqual(entry.x + entry.width);
    // The restart edge enters from the right because the pinned node sits behind its sources.
    const route = layout.edges.get(`accept-${NEW_ROUND_NODE}-7`)!;
    expect(route.points.at(-1)!.x).toBeCloseTo(newRound.x + newRound.width, 0);
  });

  it('places routes for every edge and labels on their route', async () => {
    const layout = await layoutWorkflowGraph(defaultWorkflowSpec, elk);

    for (const item of defaultWorkflowSpec.edges) {
      const route = layout.edges.get(item.id)!;
      expect(route.path.startsWith('M ')).toBe(true);
      expect(Number.isFinite(route.labelX) && Number.isFinite(route.labelY)).toBe(true);
    }
    expect(layout.bounds).not.toBeNull();
  });

  it('encloses dynamic group members and nested groups in their frames', async () => {
    const spec: WorkflowLayoutSpec = {
      nodes: [
        node('bootstrap', { sourceHandles: 'single' }),
        node('a1', { sourceHandles: 'single', groupId: 'feature-a' }),
        node('a2', { sourceHandles: 'single', groupId: 'feature-a' }),
        node('a-review', { sourceHandles: 'single', groupId: 'feature-a-review' }),
        node('final', { sourceHandles: 'single' }),
      ],
      edges: [
        { id: 'e0', from: 'bootstrap', to: 'a1', outcome: 'success' },
        { id: 'e1', from: 'bootstrap', to: 'a2', outcome: 'success' },
        { id: 'e2', from: 'a1', to: 'a-review', outcome: 'success' },
        { id: 'e3', from: 'a2', to: 'a-review', outcome: 'success' },
        { id: 'e4', from: 'a-review', to: 'final', outcome: 'success' },
      ],
      groups: [
        { id: 'feature-a', parentId: null },
        { id: 'feature-a-review', parentId: 'feature-a' },
      ],
    };
    const layout = await layoutWorkflowGraph(spec, elk);
    const contains = (outer: Rect, inner: Rect) => inner.x >= outer.x && inner.y >= outer.y
      && inner.x + inner.width <= outer.x + outer.width && inner.y + inner.height <= outer.y + outer.height;

    expectReadableLayout(spec, layout);
    const outer = layout.groups.get('feature-a')!;
    const nested = layout.groups.get('feature-a-review')!;
    expect(contains(outer, nested)).toBe(true);
    for (const id of ['a1', 'a2']) expect(contains(outer, layout.nodes.get(id)!)).toBe(true);
    expect(contains(nested, layout.nodes.get('a-review')!)).toBe(true);
    for (const id of ['bootstrap', 'final']) expect(overlaps(outer, layout.nodes.get(id)!)).toBe(false);
  });

  it('skips edges whose endpoints are not in the graph instead of failing the layout', async () => {
    const spec: WorkflowLayoutSpec = {
      nodes: [node('dev', { sourceHandles: 'single' })],
      edges: [{ id: 'dangling', from: 'dev', to: 'missing', outcome: 'success' }],
      groups: [],
    };
    const layout = await layoutWorkflowGraph(spec, elk);

    expect(layout.nodes.has('dev')).toBe(true);
    expect(layout.edges.has('dangling')).toBe(false);
  });
});
