import { describe, expect, it } from 'vitest';
import { routeWorkflowBranchEdges } from '../src/components/workflowGraph';

const nodes = ['source', 'near', 'far'].map((id) => ({ id, width: 100, height: 60 }));
const positions = new Map([
  ['source', { x: 100, y: 200 }],
  ['near', { x: 400, y: 320 }],
  ['far', { x: 1000, y: 80 }],
]);
const edges = [
  { index: 0, sourceId: 'source', targetId: 'near', branch: false },
  { index: 1, sourceId: 'source', targetId: 'far', branch: false },
];

describe('same-source forward routing', () => {
  it('aligns the first bends of nearby and distant branches', () => {
    const routes = routeWorkflowBranchEdges(nodes, positions, edges);
    expect(routes.get(0)).toBeDefined();
    expect(routes.get(1)).toBeDefined();
    const near = routes.get(0)!.points;
    const far = routes.get(1)!.points;
    expect(near[1].x).toBe(far[1].x);
    expect(near[1].x).toBeGreaterThan(150);
    expect(near[1].x).toBeLessThan(350);
    expect(near[1].y).toBe(200);
    expect(far[1].y).toBe(200);
    expect(routes.get(0)!.path).toContain(`L ${near[1].x},`);
    expect(routes.get(1)!.path).toContain(`L ${near[1].x},`);
    expect(near.at(-1)).toEqual({ x: 350, y: 320 });
    expect(far.at(-1)).toEqual({ x: 950, y: 80 });
  });

  it('does not introduce a fanout route for a single target', () => {
    expect(routeWorkflowBranchEdges(nodes, positions, edges.slice(0, 1)).size).toBe(0);
  });

  it('does not count duplicate edges to one target as fanout', () => {
    expect(routeWorkflowBranchEdges(nodes, positions, [edges[0], { ...edges[0], index: 2 }]).size).toBe(0);
  });

  it('keeps alignment independent of edge order and preserves handle offsets', () => {
    const offsetEdges = edges.map((edge) => ({ ...edge, sourceYOffset: 10, targetYOffset: -10 }));
    const routes = routeWorkflowBranchEdges(nodes, positions, offsetEdges.toReversed());
    expect(routes.get(0)!.points[1].x).toBe(routes.get(1)!.points[1].x);
    expect(routes.get(0)!.points[0]).toEqual({ x: 150, y: 210 });
    expect(routes.get(1)!.points.at(-1)).toEqual({ x: 950, y: 70 });
    expect(routes.get(0)!.detour).toBe(false);
  });

  it('uses obstacle routing when an aligned segment would cross a node', () => {
    const obstacles = [...nodes, { id: 'obstacle', width: 100, height: 60 }];
    const layout = new Map([...positions, ['obstacle', { x: 600, y: 80 }] as const]);
    const route = routeWorkflowBranchEdges(obstacles, layout, edges).get(1)!;
    expect(route).toBeDefined();
    expect(route.detour).toBe(true);
    for (let i = 1; i < route.points.length; i += 1) {
      const a = route.points[i - 1];
      const b = route.points[i];
      expect(Math.min(a.x, b.x) < 650 && Math.max(a.x, b.x) > 550
        && Math.min(a.y, b.y) < 110 && Math.max(a.y, b.y) > 50).toBe(false);
    }
  });

  it('retains obstacle-aware routing for backward edges', () => {
    const route = routeWorkflowBranchEdges(nodes, positions, [
      ...edges, { index: 2, sourceId: 'far', targetId: 'source', branch: true },
    ]).get(2)!;
    expect(route.detour).toBe(true);
    expect(route.points[0]).toEqual({ x: 1050, y: 80 });
    expect(route.points.at(-1)).toEqual({ x: 50, y: 200 });
  });
});
