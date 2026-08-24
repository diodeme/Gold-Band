/**
 * Shared workflow graph layout primitives used by both WorkflowEditor and GraphView.
 * Both authoring (editable) and runtime (read-only) graphs use the same
 * success-edge-only dagre layout, lane-routed branch edges, and visual tokens.
 */
import dagre from 'dagre';
import { getSmoothStepPath, Position, type Node, type Rect } from '@xyflow/react';
import {
  getSmartEdge,
  pathfindingJumpPointNoDiagonal,
  svgDrawSmoothStepLinePath,
} from '@tisoap/react-flow-smart-edge';
import type { GraphNodeVm, GraphEdgeVm, GraphVm, WorkflowDsl, WorkflowEdgeDsl } from '../types';

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

// Edge labels are rendered at 11px with horizontal padding. These conservative
// bounds cover both zh-CN and en labels and let the router reserve real space
// without measuring the DOM on every viewport update.
export const WORKFLOW_EDGE_LABEL_WIDTH = 68;
export const WORKFLOW_EDGE_LABEL_HEIGHT = 24;
export const WORKFLOW_EDGE_LABEL_GAP = 8;

// ── Terminal sentinel IDs ─────────────────────────────────────────────────
export const END_NODE = '$end';
export const ENTRY_NODE = '$entry';
export const NEW_ROUND_NODE = '$new-round';

// ── Branch routing helpers ────────────────────────────────────────────────

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

// ── Success-edge-only dagre layout ────────────────────────────────────────

export interface DagreNodeSpec {
  id: string;
  width: number;
  height: number;
}

export interface WorkflowGraphBranchRouteSpec {
  index: number;
  sourceId: string;
  targetId: string;
  sourceYOffset?: number;
  targetYOffset?: number;
  /** False for a compact primary edge whose label must still reserve space. */
  branch?: boolean;
}

export interface WorkflowGraphBranchRoute {
  path: string;
  labelX: number;
  labelY: number;
  points: Array<{ x: number; y: number }>;
}

const WORKFLOW_EDGE_ROUTING_OPTIONS = {
  gridRatio: 10,
  nodePadding: 18,
  drawEdge: svgDrawSmoothStepLinePath({ borderRadius: 8 }),
  generatePath: pathfindingJumpPointNoDiagonal,
} as const;

type Point = { x: number; y: number };

function edgeLabelRect(point: Point): Rect {
  return {
    x: point.x - WORKFLOW_EDGE_LABEL_WIDTH / 2,
    y: point.y - WORKFLOW_EDGE_LABEL_HEIGHT / 2,
    width: WORKFLOW_EDGE_LABEL_WIDTH,
    height: WORKFLOW_EDGE_LABEL_HEIGHT,
  };
}

function expandRect(rect: Rect, padding: number): Rect {
  return {
    x: rect.x - padding,
    y: rect.y - padding,
    width: rect.width + padding * 2,
    height: rect.height + padding * 2,
  };
}

function rectsIntersect(left: Rect, right: Rect): boolean {
  return left.x < right.x + right.width
    && left.x + left.width > right.x
    && left.y < right.y + right.height
    && left.y + left.height > right.y;
}

function simplifyOrthogonalPoints(points: Point[]): Point[] {
  return points.filter((point, index) => {
    if (index === 0 || index === points.length - 1) return true;
    const previous = points[index - 1];
    const next = points[index + 1];
    return !((previous.x === point.x && point.x === next.x) || (previous.y === point.y && point.y === next.y));
  });
}

/**
 * Place a horizontal label on the clearest routed segment. Smart Edge owns
 * obstacle-aware pathfinding; this small adapter keeps the label's own bounds
 * clear of nodes and labels instead of treating its center as a zero-size point.
 */
function placeEdgeLabel(
  points: Point[],
  nodeAreas: Rect[],
  reservedLabelAreas: Rect[],
  fallback: Point,
): Point {
  const routePoints = simplifyOrthogonalPoints(points);
  const segmentLengths = routePoints.slice(1).map((point, index) => (
    Math.abs(point.x - routePoints[index].x) + Math.abs(point.y - routePoints[index].y)
  ));
  const totalLength = segmentLengths.reduce((sum, length) => sum + length, 0);
  const collisionAreas = [
    ...nodeAreas.map((rect) => expandRect(rect, WORKFLOW_EDGE_LABEL_GAP)),
    ...reservedLabelAreas.map((rect) => expandRect(rect, WORKFLOW_EDGE_LABEL_GAP)),
  ];
  const candidates: Array<{ point: Point; score: number }> = [];
  let traversed = 0;

  routePoints.slice(1).forEach((end, index) => {
    const start = routePoints[index];
    const length = segmentLengths[index];
    const horizontal = start.y === end.y;
    const halfExtent = horizontal ? WORKFLOW_EDGE_LABEL_WIDTH / 2 : WORKFLOW_EDGE_LABEL_HEIGHT / 2;
    const usableLength = length - (halfExtent + WORKFLOW_EDGE_LABEL_GAP) * 2;
    if (usableLength >= 0) {
      const sampleCount = Math.max(1, Math.ceil(usableLength / 20));
      for (let sample = 0; sample <= sampleCount; sample += 1) {
        const distance = halfExtent + WORKFLOW_EDGE_LABEL_GAP + usableLength * (sample / Math.max(1, sampleCount));
        const direction = end.x !== start.x ? Math.sign(end.x - start.x) : Math.sign(end.y - start.y);
        const point = horizontal
          ? { x: start.x + direction * distance, y: start.y }
          : { x: start.x, y: start.y + direction * distance };
        const area = edgeLabelRect(point);
        if (collisionAreas.some((obstacle) => rectsIntersect(area, obstacle))) continue;
        const pathDistance = traversed + distance;
        candidates.push({
          point,
          score: Math.abs(pathDistance - totalLength / 2) + (horizontal ? 0 : 120),
        });
      }
    }
    traversed += length;
  });

  candidates.sort((left, right) => left.score - right.score);
  if (candidates[0]) return candidates[0].point;
  return fallback;
}

/**
 * Route branch edges around node rectangles. Primary success edges intentionally
 * stay on React Flow's compact smooth-step path and are not passed here.
 */
export function routeWorkflowBranchEdges(
  nodes: DagreNodeSpec[],
  layoutPositions: ReadonlyMap<string, { x: number; y: number }>,
  edges: WorkflowGraphBranchRouteSpec[],
): Map<number, WorkflowGraphBranchRoute> {
  const nodeById = new Map(nodes.map((node) => [node.id, node]));
  const obstacles: Node<Record<string, unknown>>[] = nodes.flatMap((node) => {
    const position = layoutPositions.get(node.id);
    if (!position) return [];
    return [{
      id: node.id,
      position: topLeft(position.x, position.y, node.width, node.height),
      data: {},
      measured: { width: node.width, height: node.height },
      width: node.width,
      height: node.height,
    }];
  });
  const nodeAreas = nodes.flatMap((node) => {
    const position = layoutPositions.get(node.id);
    if (!position) return [];
    return [{
      x: position.x - node.width / 2,
      y: position.y - node.height / 2,
      width: node.width,
      height: node.height,
    }];
  });
  const routes = new Map<number, WorkflowGraphBranchRoute>();
  const reservedLabelAreas: Rect[] = [];

  edges.filter((edge) => edge.branch === false).forEach((edge) => {
    const sourceNode = nodeById.get(edge.sourceId);
    const targetNode = nodeById.get(edge.targetId);
    const sourcePosition = layoutPositions.get(edge.sourceId);
    const targetPosition = layoutPositions.get(edge.targetId);
    if (!sourceNode || !targetNode || !sourcePosition || !targetPosition) return;
    const [, labelX, labelY] = getSmoothStepPath({
      sourceX: sourcePosition.x + sourceNode.width / 2,
      sourceY: sourcePosition.y + (edge.sourceYOffset ?? 0),
      sourcePosition: Position.Right,
      targetX: targetPosition.x - targetNode.width / 2,
      targetY: targetPosition.y + (edge.targetYOffset ?? 0),
      targetPosition: Position.Left,
    });
    reservedLabelAreas.push(edgeLabelRect({ x: labelX, y: labelY }));
  });

  edges.filter((edge) => edge.branch !== false).forEach((edge) => {
    const sourceNode = nodeById.get(edge.sourceId);
    const targetNode = nodeById.get(edge.targetId);
    const sourcePosition = layoutPositions.get(edge.sourceId);
    const targetPosition = layoutPositions.get(edge.targetId);
    if (!sourceNode || !targetNode || !sourcePosition || !targetPosition) return;

    const sourceX = sourcePosition.x + sourceNode.width / 2;
    const sourceY = sourcePosition.y + (edge.sourceYOffset ?? 0);
    const targetX = targetPosition.x - targetNode.width / 2;
    const targetY = targetPosition.y + (edge.targetYOffset ?? 0);
    const endpointClearance = WORKFLOW_EDGE_ROUTING_OPTIONS.nodePadding + WORKFLOW_EDGE_ROUTING_OPTIONS.gridRatio;
    const sourceArea = expandRect({
      x: sourcePosition.x - sourceNode.width / 2,
      y: sourcePosition.y - sourceNode.height / 2,
      width: sourceNode.width,
      height: sourceNode.height,
    }, endpointClearance);
    const targetArea = expandRect({
      x: targetPosition.x - targetNode.width / 2,
      y: targetPosition.y - targetNode.height / 2,
      width: targetNode.width,
      height: targetNode.height,
    }, endpointClearance);
    // Smart Edge opens an escape corridor from each handle through contiguous
    // obstacles. A label touching that corridor must stay a placement-only
    // obstacle, otherwise the escape step can also open the next node.
    const routeAvoidAreas = reservedLabelAreas.filter((area) => (
      !rectsIntersect(area, sourceArea) && !rectsIntersect(area, targetArea)
    ));
    const routed = getSmartEdge({
      nodes: obstacles,
      sourceX,
      sourceY,
      targetX,
      targetY,
      sourcePosition: Position.Right,
      targetPosition: Position.Left,
      options: { ...WORKFLOW_EDGE_ROUTING_OPTIONS, avoidAreas: routeAvoidAreas },
    });
    if (routed instanceof Error) return;

    const rawPoints = [
      { x: sourceX, y: sourceY },
      ...routed.points.map(([x, y]) => ({ x, y })),
      { x: targetX, y: targetY },
    ];
    const points = rawPoints.filter((point, index) => index === 0 || point.x !== rawPoints[index - 1].x || point.y !== rawPoints[index - 1].y);
    const labelPosition = placeEdgeLabel(
      points,
      nodeAreas,
      reservedLabelAreas,
      { x: routed.edgeCenterX, y: routed.edgeCenterY },
    );
    reservedLabelAreas.push(edgeLabelRect(labelPosition));
    routes.set(edge.index, {
      path: routed.svgPathString,
      labelX: labelPosition.x,
      labelY: labelPosition.y,
      points,
    });
  });

  return routes;
}

/**
 * Run dagre LR layout using only success/forward edges for rank constraints.
 * Returns a map of nodeId → { x, y } center positions.
 */
export function layoutSuccessPath(
  nodes: DagreNodeSpec[],
  edges: Array<{ from: string; to: string; on?: string }>,
  nodeIds: Set<string>,
  nodeOrder?: Map<string, number>,
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
    if (nodeOrder && isBackwardEdge(e.from, e.to, nodeOrder)) continue;
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

/** Node order derived from the authoring graph's success path instead of array append order. */
export function workflowSuccessTopologyOrder(workflow: Pick<WorkflowDsl, 'entry' | 'nodes' | 'edges'>): Map<string, number> {
  const nodeIds = workflow.nodes.map((node) => node.id).filter(Boolean);
  const nodeIdSet = new Set(nodeIds);
  const adjacency = new Map<string, string[]>();
  const indegree = new Map<string, number>();

  nodeIds.forEach((id) => {
    adjacency.set(id, []);
    indegree.set(id, 0);
  });

  workflow.edges.forEach((edge) => {
    if (edge.on !== 'success') return;
    if (!nodeIdSet.has(edge.from) || !nodeIdSet.has(edge.to)) return;
    adjacency.get(edge.from)?.push(edge.to);
    indegree.set(edge.to, (indegree.get(edge.to) ?? 0) + 1);
  });

  const queued = new Set<string>();
  const queue: string[] = [];
  const pushRoot = (id: string) => {
    if (!nodeIdSet.has(id) || queued.has(id)) return;
    queued.add(id);
    queue.push(id);
  };

  pushRoot(workflow.entry);
  nodeIds.forEach((id) => {
    if ((indegree.get(id) ?? 0) === 0) pushRoot(id);
  });

  const ordered: string[] = [];
  while (queue.length > 0) {
    const id = queue.shift()!;
    ordered.push(id);
    adjacency.get(id)?.forEach((nextId) => {
      indegree.set(nextId, (indegree.get(nextId) ?? 0) - 1);
      if ((indegree.get(nextId) ?? 0) === 0) pushRoot(nextId);
    });
  }

  nodeIds.forEach((id) => {
    if (!queued.has(id)) ordered.push(id);
  });

  return new Map(ordered.map((id, index) => [id, index]));
}

/** Edge color CSS variable for authoring edges. */
export function authoringEdgeColor(outcome: WorkflowEdgeDsl['on']): string {
  if (outcome === 'failure') return 'var(--destructive)';
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

export function runtimeGraphTopologySignature(
  graph: GraphVm,
  variant: string,
): string {
  const nodes = graph.nodes
    .map((node) => `${node.id}:${node.sequence ?? ''}`)
    .join('|');
  const edges = graph.edges
    .map((edge) => `${edge.from}>${edge.to}:${edge.label?.toLowerCase() ?? ''}`)
    .join('|');
  return `${variant}:${nodes}:${edges}`;
}

export function runtimeGraphEdgeDisplayLabel(
  edge: Pick<GraphEdgeVm, 'label' | 'traversalCount' | 'blockedReason'>,
  translate: (value: string) => string,
): string | undefined {
  const baseLabel = edge.label ? translate(edge.label) : '';
  if (edge.blockedReason) {
    const limitLabel = `${edge.blockedReason.proposedCount ?? '-'}/${edge.blockedReason.limit ?? '-'}`;
    return baseLabel ? `${baseLabel} · ${limitLabel}` : limitLabel;
  }
  if (edge.traversalCount && edge.traversalCount > 1) {
    return baseLabel ? `${baseLabel} ×${edge.traversalCount}` : `×${edge.traversalCount}`;
  }
  return baseLabel || undefined;
}

export function runtimeGraphEdgeClassName(active: boolean, branch: boolean) {
  return [
    'workflow-edge-flow',
    branch ? 'workflow-edge-branch' : '',
    active ? 'workflow-edge-running' : '',
  ].filter(Boolean).join(' ');
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
