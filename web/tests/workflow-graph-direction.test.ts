import { readFileSync } from 'node:fs';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import ELK from 'elkjs/lib/elk.bundled.js';

import {
  NODE_WIDTH,
  autoWorkflowLayoutDirection,
  buildElkGraph,
  entryAnchoredViewport,
  layoutWorkflowGraph,
  workflowHandlePositions,
  type WorkflowLayoutSpec,
} from '@/components/workflowGraph';
import { resolveWorkflowLayoutDirection } from '@/components/workflow-direction-preference';

const elk = new ELK();

function spec(direction?: 'RIGHT' | 'DOWN'): WorkflowLayoutSpec {
  return {
    ...(direction ? { direction } : {}),
    nodes: [
      { id: 'dev', width: NODE_WIDTH, height: 66, sourceHandles: 'split' },
      { id: '$end', width: 140, height: 44, sourceHandles: 'none' },
      { id: '$new-round', width: 140, height: 44, sourceHandles: 'none', pinBelow: 'dev' },
    ],
    edges: [
      { id: 'e1', from: 'dev', to: '$end', outcome: 'success' },
      { id: 'e2', from: 'dev', to: '$new-round', outcome: 'failure' },
    ],
    groups: [],
  };
}

describe('workflow graph layout direction', () => {
  it('picks the initial direction from the container aspect ratio', () => {
    expect(autoWorkflowLayoutDirection(800, 600)).toBe('RIGHT');
    expect(autoWorkflowLayoutDirection(600, 800)).toBe('DOWN');
    expect(autoWorkflowLayoutDirection(0, 800)).toBe('RIGHT');
  });

  it('applies hysteresis so dragging across the 1:1 ratio does not thrash layouts', () => {
    // Already horizontal: stays horizontal until the ratio clearly exceeds 1.1.
    expect(autoWorkflowLayoutDirection(100, 105, 'RIGHT')).toBe('RIGHT');
    expect(autoWorkflowLayoutDirection(100, 115, 'RIGHT')).toBe('DOWN');
    // Already vertical: stays vertical until the ratio clearly drops below 0.9.
    expect(autoWorkflowLayoutDirection(100, 95, 'DOWN')).toBe('DOWN');
    expect(autoWorkflowLayoutDirection(100, 85, 'DOWN')).toBe('RIGHT');
  });

  it('maps the user preference onto the effective direction', () => {
    expect(resolveWorkflowLayoutDirection('horizontal', 600, 800)).toBe('RIGHT');
    expect(resolveWorkflowLayoutDirection('vertical', 800, 600)).toBe('DOWN');
    expect(resolveWorkflowLayoutDirection('auto', 800, 600)).toBe('RIGHT');
    expect(resolveWorkflowLayoutDirection('auto', 600, 800)).toBe('DOWN');
  });

  it('mirrors React Flow handle positions per direction', () => {
    expect(workflowHandlePositions('RIGHT')).toEqual({ source: 'right', target: 'left' });
    expect(workflowHandlePositions('DOWN')).toEqual({ source: 'bottom', target: 'top' });
  });

  it('defaults omitted directions to the horizontal flow', () => {
    const graph = buildElkGraph(spec());
    expect(graph.layoutOptions?.['elk.direction']).toBe('RIGHT');
  });

  it('puts every main-chain port on the shared success lane', () => {
    // Mixed split/single chains must produce straight hops: single-outcome
    // sources and terminal entries share the split success fraction instead
    // of centering at 50%, which used to bend every mixed edge and drift
    // the columns.
    const graph = buildElkGraph(spec('DOWN'));
    const portAt = (id: string) => graph.children!.find((node) => node.id === id)!.ports!;
    const build = portAt('dev');
    expect(build.find((port) => port.id === 'dev::success')!.x).toBeCloseTo(NODE_WIDTH * 0.34);
    // $end is a single-width terminal whose entry joins the same lane.
    const end = portAt('$end');
    expect(end[0].x).toBeCloseTo(140 * 0.34);
    // $new-round is pinned below dev and accepts backward edges on its
    // downstream side, still on the lane.
    const newRound = portAt('$new-round');
    expect(newRound[0].x).toBeCloseTo(140 * 0.34);
  });

  it('builds a vertical ELK graph with mirrored ports and pinned position', () => {
    const graph = buildElkGraph(spec('DOWN'));
    expect(graph.layoutOptions?.['elk.direction']).toBe('DOWN');

    const dev = graph.children!.find((node) => node.id === 'dev')!;
    const end = graph.children!.find((node) => node.id === '$end')!;
    const newRound = graph.children!.find((node) => node.id === '$new-round')!;

    const portSides = (node: typeof dev) => node.ports!.map((port) => port.layoutOptions?.['elk.port.side']);
    expect(portSides(dev)).toEqual(['NORTH', 'SOUTH', 'SOUTH']);
    expect(portSides(end)).toEqual(['NORTH']);
    // The pinned new-round target accepts backward edges on its downstream side.
    expect(portSides(newRound)).toEqual(['SOUTH']);

    // Split success/failure source ports split along the width axis in DOWN flow.
    const devSuccess = dev.ports!.find((port) => port.id === 'dev::success')!;
    const devFailure = dev.ports!.find((port) => port.id === 'dev::failure')!;
    expect(devSuccess.x).toBe(NODE_WIDTH * 0.34);
    expect(devFailure.x).toBe(NODE_WIDTH * 0.66);
    expect(devSuccess.y).toBe(66);
    expect(devFailure.y).toBe(66);

    // The pinned node is placed far right in the first (top) layer.
    expect(newRound.layoutOptions?.['elk.position']).toBe('(100000,0)');
    expect(newRound.layoutOptions?.['elk.layered.layering.layerConstraint']).toBe('FIRST');
  });

  it('keeps the entry side visible when the graph cannot fit', () => {
    const bounds = { x: 0, y: 0, width: 2000, height: 600 };
    const viewport = { width: 400, height: 800 };
    const horizontal = entryAnchoredViewport(bounds, viewport, { padding: 0.2, minZoom: 0.3, maxZoom: 0.92, direction: 'RIGHT' });
    expect(horizontal.x).toBeGreaterThan(-0.001);
    const vertical = entryAnchoredViewport(bounds, viewport, { padding: 0.2, minZoom: 0.3, maxZoom: 0.92, direction: 'DOWN' });
    expect(vertical.y).toBeGreaterThan(-0.001);
    // In the vertical flow the horizontal axis stays centered.
    expect(vertical.x).toBeLessThan(horizontal.x);
  });

  // Contract (task-workflow.md): the success main chain keeps one column in
  // DOWN flow and one row in RIGHT flow — even when the chain mixes split
  // nodes (success/failure ports at 34%/66%) with single-outcome nodes (50%).
  // Port-perfect edge straightening must not drift the nodes instead.
  it('keeps the mixed success main chain on one column or row without overlaps', async () => {
    const mixed: WorkflowLayoutSpec = {
      direction: 'DOWN',
      nodes: [
        { id: 'dev', width: NODE_WIDTH, height: 66, sourceHandles: 'split' },
        { id: 'build', width: NODE_WIDTH, height: 66, sourceHandles: 'single' },
        { id: 'review', width: NODE_WIDTH, height: 66, sourceHandles: 'split' },
        { id: '$end', width: 140, height: 44, sourceHandles: 'none' },
        { id: '$new-round', width: 140, height: 44, sourceHandles: 'none', pinBelow: 'dev' },
      ],
      edges: [
        { id: 'e1', from: 'dev', to: 'build', outcome: 'success' },
        { id: 'e2', from: 'build', to: 'review', outcome: 'success' },
        { id: 'e3', from: 'review', to: '$end', outcome: 'success' },
        { id: 'e4', from: 'review', to: 'build', outcome: 'failure' },
        { id: 'e5', from: 'dev', to: '$new-round', outcome: 'failure' },
      ],
      groups: [],
    };

    // The worker spine shares the track exactly. The narrow $end terminal is
    // allowed to sit wherever its inbound edge is straight: same-width ports
    // line up only for same-width nodes, and straightening the terminal hop
    // reads better than forcing the round terminal onto the column.
    const vertical = await layoutWorkflowGraph(mixed, elk);
    const chainX = ['dev', 'build', 'review'].map((id) => vertical.nodes.get(id)!.x);
    expect(Math.max(...chainX) - Math.min(...chainX)).toBeLessThanOrEqual(1);

    const horizontal = await layoutWorkflowGraph({ ...mixed, direction: 'RIGHT' }, elk);
    const chainY = ['dev', 'build', 'review'].map((id) => horizontal.nodes.get(id)!.y);
    expect(Math.max(...chainY) - Math.min(...chainY)).toBeLessThanOrEqual(1);

    // Branching topologies still produce overlap-free placements.
    const parallel: WorkflowLayoutSpec = {
      direction: 'DOWN',
      nodes: [
        { id: 'dev', width: NODE_WIDTH, height: 66, sourceHandles: 'split' },
        { id: 'fast', width: NODE_WIDTH, height: 66, sourceHandles: 'single' },
        { id: 'deep', width: NODE_WIDTH, height: 66, sourceHandles: 'single' },
        { id: 'mid', width: NODE_WIDTH, height: 66, sourceHandles: 'single' },
        { id: 'merge', width: NODE_WIDTH, height: 66, sourceHandles: 'split' },
        { id: '$end', width: 140, height: 44, sourceHandles: 'none' },
        { id: '$new-round', width: 140, height: 44, sourceHandles: 'none', pinBelow: 'dev' },
      ],
      edges: [
        { id: 'p1', from: 'dev', to: 'fast', outcome: 'success' },
        { id: 'p2', from: 'dev', to: 'deep', outcome: 'failure' },
        { id: 'p3', from: 'deep', to: 'mid', outcome: 'success' },
        { id: 'p4', from: 'fast', to: 'merge', outcome: 'success' },
        { id: 'p5', from: 'mid', to: 'merge', outcome: 'success' },
        { id: 'p6', from: 'merge', to: '$end', outcome: 'success' },
        { id: 'p7', from: 'merge', to: '$new-round', outcome: 'failure' },
      ],
      groups: [],
    };
    const branched = await layoutWorkflowGraph(parallel, elk);
    const rects = [...branched.nodes.values()];
    for (let i = 0; i < rects.length; i += 1) {
      for (let j = i + 1; j < rects.length; j += 1) {
        const a = rects[i];
        const b = rects[j];
        const overlaps = a.x < b.x + b.width && a.x + a.width > b.x
          && a.y < b.y + b.height && a.y + a.height > b.y;
        expect(overlaps).toBe(false);
      }
    }
  });
});

describe('workflow direction preference persistence', () => {
  it('wires the preference into the personalization schema through the app shell', () => {
    const app = readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    expect(app).toContain('WorkflowGraphDirectionProvider');
    expect(app).toContain('saveWorkflowGraphDirection(next)');

    const types = readFileSync(path.resolve(__dirname, '../src/types.ts'), 'utf8');
    expect(types).toContain("schemaVersion: 5;");
    expect(types).toContain('interfaceZoomPercent: number;');
    expect(types).toContain("workflowGraphDirection: WorkflowGraphDirectionPreference;");
  });

  it('persists direction toggles through the single-field save, not a full round-trip', () => {
    // A graph-control toggle is a background gesture: the full-save path
    // captures the frontend personalization at click time and can revert a
    // just-persisted shortcut zoom that has not re-rendered yet.
    const app = readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    expect(app).toContain('.then(() => saveWorkflowGraphDirection(next))');
    expect(app).not.toContain('{ ...preferences.personalization, workflowGraphDirection: next }');

    const commandsRs = readFileSync(path.resolve(__dirname, '../../src-tauri/src/commands.rs'), 'utf8');
    expect(commandsRs).toContain('pub fn save_workflow_graph_direction(');
    expect(commandsRs).toContain('personalization.workflow_graph_direction = input.direction;');
    const mainRs = readFileSync(path.resolve(__dirname, '../../src-tauri/src/main.rs'), 'utf8');
    expect(mainRs).toContain('save_workflow_graph_direction,');
  });
});
