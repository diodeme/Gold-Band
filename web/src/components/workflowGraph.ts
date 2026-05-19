/**
 * Shared workflow graph layout primitives used by both WorkflowEditor and GraphView.
 * Both authoring (editable) and runtime (read-only) graphs use the same
 * success-edge-only dagre layout, lane-routed branch edges, and visual tokens.
 */
import dagre from 'dagre';
import { Position } from '@xyflow/react';
import type { GraphNodeVm, GraphEdgeVm, WorkflowDsl, WorkflowEdgeDsl } from '../types';

// ── Node sizing (authoring editor values – used as canonical) ──────────────
export const NODE_WIDTH = 220;
export const NODE_HEIGHT = 66;
export const TERMINAL_NODE_WIDTH = 140;
export const TERMINAL_NODE_HEIGHT = 44;

// ── Dagre spacing ─────────────────────────────────────────────────────────
export const LAYOUT_NODE_SEP = 72;
export const LAYOUT_RANK_SEP = 116;
export const LAYOUT_MARGIN_X = 56;
export const LAYOUT_MARGIN_Y = 120;

// ── Terminal sentinel IDs ─────────────────────────────────────────────────
export const END_NODE = '$end';
export const NEW_ROUND_NODE = '$new-round';

// ── Lane routing helpers ──────────────────────────────────────────────────

/** Determine whether a non-success edge goes backward in node order. */
export function isBackwardEdge(
  from: string,
  to: string,
  nodeOrder: Map<string, number>,
): boolean {
  const s = nodeOrder.get(from);
  const t = nodeOrder.get(to);
  return s !== undefined && t !== undefined && t < s;
}

/** Compute lane index for each non-success backward edge (for routed rendering). */
export function computeBackwardLanes(
  edges: Array<{ from: string; to: string; on: string }>,
  nodeOrder: Map<string, number>,
): Map<number, number> {
  const lanes = new Map<number, number>();
  edges
    .map((edge, index) => ({ edge, index }))
    .filter(({ edge }) => edge.on !== 'success' && isBackwardEdge(edge.from, edge.to, nodeOrder))
    .forEach(({ index }, lane) => lanes.set(index, lane));
  return lanes;
}

// ── Success-edge-only dagre layout ────────────────────────────────────────

export interface DagreNodeSpec {
  id: string;
  width: number;
  height: number;
}

/**
 * Run dagre LR layout using only success/forward edges for rank constraints.
 * Returns a map of nodeId → { x, y } center positions.
 */
export function layoutSuccessPath(
  nodes: DagreNodeSpec[],
  edges: Array<{ from: string; to: string; on?: string }>,
  nodeIds: Set<string>,
): Map<string, { x: number; y: number }> {
  const g = new dagre.graphlib.Graph();
  g.setDefaultEdgeLabel(() => ({}));
  g.setGraph({
    rankdir: 'LR',
    nodesep: LAYOUT_NODE_SEP,
    ranksep: LAYOUT_RANK_SEP,
    marginx: LAYOUT_MARGIN_X,
    marginy: LAYOUT_MARGIN_Y,
  });
  for (const n of nodes) g.setNode(n.id, { width: n.width, height: n.height });
  for (const e of edges) {
    if (e.on !== undefined && e.on !== 'success') continue;
    if (nodeIds.has(e.from) && nodeIds.has(e.to)) g.setEdge(e.from, e.to);
  }
  dagre.layout(g);
  const result = new Map<string, { x: number; y: number }>();
  for (const n of nodes) {
    const pos = g.node(n.id);
    if (pos) result.set(n.id, { x: pos.x, y: pos.y });
  }
  return result;
}

// ── Authoring (WorkflowDsl) graph conversion helpers ──────────────────────

export interface AuthoringNodeInfo {
  id: string;
  terminal: boolean;
}

/** Collect terminal pseudo-nodes and build the full authoring node list. */
export function collectAuthoringNodes(workflow: WorkflowDsl): AuthoringNodeInfo[] {
  const terminalIds = [END_NODE, NEW_ROUND_NODE].filter((tid) =>
    workflow.edges.some((e) => e.to === tid),
  );
  return [
    ...workflow.nodes.map((n) => ({ id: n.id, terminal: false })),
    ...terminalIds.map((id) => ({ id, terminal: true })),
  ];
}

/** Node order map from workflow.nodes array index. */
export function workflowNodeOrder(workflow: WorkflowDsl): Map<string, number> {
  return new Map(workflow.nodes.map((n, i) => [n.id, i]));
}

/** Edge color CSS variable for authoring edges. */
export function authoringEdgeColor(outcome: WorkflowEdgeDsl['on']): string {
  if (outcome === 'failure') return 'var(--destructive)';
  if (outcome === 'invalid') return 'var(--muted-foreground)';
  return 'var(--muted-foreground)';
}

// ── Runtime (GraphVm) graph conversion helpers ────────────────────────────

/**
 * Build node order from runtime graph nodes, preferring `sequence` field
 * for stable ordering. Falls back to array index.
 */
export function runtimeNodeOrder(nodes: GraphNodeVm[]): Map<string, number> {
  const sorted = [...nodes].sort((a, b) => (a.sequence ?? 0) - (b.sequence ?? 0));
  return new Map(sorted.map((n, i) => [n.id, i]));
}

/**
 * Determine which runtime edges are "primary" (success-like / forward)
 * vs "branch" (failure / backward) for layout purposes.
 */
export function isRuntimePrimaryEdge(
  edge: GraphEdgeVm,
  nodeOrder: Map<string, number>,
): boolean {
  const label = edge.label?.toLowerCase() ?? '';
  if (label === 'success') return true;
  // Non-success forward edges still participate in layout so they don't overlap
  return !isBackwardEdge(edge.from, edge.to, nodeOrder);
}

/** Edge color CSS variable for runtime edges. */
export function runtimeEdgeColor(
  edge: GraphEdgeVm,
  active: boolean,
): string {
  if (active) return 'var(--gold-running)';
  const label = edge.label?.toLowerCase() ?? '';
  if (label === 'failure') return 'var(--destructive)';
  return 'var(--muted-foreground)';
}

/** Position helper: center of a node at (x, y) with given size. */
export function topLeft(x: number, y: number, w: number, h: number) {
  return { x: x - w / 2, y: y - h / 2 };
}

/** Shared ReactFlow node positions. */
export const SOURCE_POS = Position.Right;
export const TARGET_POS = Position.Left;
