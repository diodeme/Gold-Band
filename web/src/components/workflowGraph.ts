/**
 * Shared workflow graph layout primitives used by both WorkflowEditor and GraphView.
 * Both authoring (editable) and runtime (read-only) graphs are projected into one
 * `WorkflowLayoutSpec`; ELK layered owns node placement, orthogonal edge routing,
 * edge-label space and group containment.
 */
import { Position } from '@xyflow/react';
import type { ELK, ElkExtendedEdge, ElkNode, ElkPort, LayoutOptions } from 'elkjs/lib/elk-api';
import type { GraphNodeVm, GraphEdgeVm, GraphVm, WorkflowDsl, WorkflowEdgeDsl } from '../types';

// ── Node sizing (authoring editor values – used as canonical) ──────────────
export const NODE_WIDTH = 220;
export const NODE_HEIGHT = 66;
export const TERMINAL_NODE_WIDTH = 140;
export const TERMINAL_NODE_MAX_WIDTH = 260;
export const TERMINAL_NODE_HEIGHT = 44;
/** Runtime graph nodes use a taller card for status badges. */
export const RUNTIME_NODE_HEIGHT = 138;

/** Source handles of a node that can fail sit at these fractions of its height. */
export const WORKFLOW_NODE_OUTCOME_HANDLE_RATIO = { success: 0.34, failure: 0.66 } as const;

// Edge labels float at the route's half-length point (never inside the ELK
// spec); the height bounds how far a label may sit from its route.
export const WORKFLOW_EDGE_LABEL_HEIGHT = 24;
const TERMINAL_LABEL_FONT_SIZE = 12;
const TERMINAL_LABEL_PADDING = 40;
const EDGE_CORNER_RADIUS = 8;

// ── Terminal sentinel IDs ─────────────────────────────────────────────────
export const END_NODE = '$end';
export const ENTRY_NODE = '$entry';
export const NEW_ROUND_NODE = '$new-round';

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

// ── Layout data model ─────────────────────────────────────────────────────

export type Point = { x: number; y: number };
export type Rect = { x: number; y: number; width: number; height: number };
export type WorkflowLayoutOutcome = 'success' | 'failure' | 'other';
/** ELK layered flow direction; `DOWN` stacks layers vertically for tall containers. */
export type WorkflowLayoutDirection = 'RIGHT' | 'DOWN';

export const WORKFLOW_DIRECTION_HYSTERESIS = 0.1;

/**
 * Resolves the effective layout direction for an `auto` preference from the
 * canvas container aspect ratio. Switching between directions applies
 * hysteresis so dragging a panel across the 1:1 ratio does not re-layout
 * on every pixel; an explicit `previous` keeps the current direction until
 * the ratio leaves the ±hysteresis band.
 */
export function autoWorkflowLayoutDirection(
  width: number,
  height: number,
  previous?: WorkflowLayoutDirection,
): WorkflowLayoutDirection {
  if (!(width > 0) || !(height > 0)) return previous ?? 'RIGHT';
  const ratio = height / width;
  if (previous === 'DOWN') return ratio >= 1 - WORKFLOW_DIRECTION_HYSTERESIS ? 'DOWN' : 'RIGHT';
  if (previous === 'RIGHT') return ratio > 1 + WORKFLOW_DIRECTION_HYSTERESIS ? 'DOWN' : 'RIGHT';
  return ratio >= 1 ? 'DOWN' : 'RIGHT';
}

/** React Flow handle positions for one layout direction. */
export function workflowHandlePositions(direction: WorkflowLayoutDirection) {
  return direction === 'DOWN'
    ? { source: Position.Bottom, target: Position.Top }
    : { source: Position.Right, target: Position.Left };
}

export interface WorkflowLayoutNodeSpec {
  id: string;
  width: number;
  height: number;
  /** `split` exposes success / failure source handles, `none` has no outgoing edges. */
  sourceHandles: 'single' | 'split' | 'none';
  groupId?: string | null;
  /**
   * Keeps a control target in the entry column directly below this node, so it is
   * visible in the initial viewport. Pinned nodes only receive backward edges and
   * therefore accept them on their right side.
   */
  pinBelow?: string | null;
}

export interface WorkflowLayoutEdgeSpec {
  id: string;
  from: string;
  to: string;
  outcome: WorkflowLayoutOutcome;
}

export interface WorkflowLayoutGroupSpec {
  id: string;
  parentId: string | null;
}

/** Minimal semantic input of a layout: model order, sizes, ports, labels, groups and direction. */
export interface WorkflowLayoutSpec {
  nodes: WorkflowLayoutNodeSpec[];
  edges: WorkflowLayoutEdgeSpec[];
  groups: WorkflowLayoutGroupSpec[];
  /** Omitted specs default to the horizontal (`RIGHT`) flow. */
  direction?: WorkflowLayoutDirection;
}

export interface WorkflowLayoutRoute {
  points: Point[];
  path: string;
  labelX: number;
  labelY: number;
}

export interface WorkflowLayout {
  /** Top-left node rects. */
  nodes: Map<string, Rect>;
  groups: Map<string, Rect>;
  edges: Map<string, WorkflowLayoutRoute>;
  bounds: Rect | null;
  direction: WorkflowLayoutDirection;
}

/** Stable identity of a layout input; anything outside the spec cannot move nodes. */
export function workflowLayoutKey(spec: WorkflowLayoutSpec): string {
  return JSON.stringify(spec);
}

export function estimateLabelWidth(text: string, fontSize: number, padding: number): number {
  let width = 0;
  for (const char of text) width += /[⺀-￯]/.test(char) ? fontSize : fontSize * 0.62;
  return Math.ceil(width + padding);
}

export function terminalNodeWidth(label: string): number {
  return Math.min(TERMINAL_NODE_MAX_WIDTH, Math.max(TERMINAL_NODE_WIDTH, estimateLabelWidth(label, TERMINAL_LABEL_FONT_SIZE, TERMINAL_LABEL_PADDING)));
}

// ── ELK conversion ────────────────────────────────────────────────────────

const ELK_LAYOUT_OPTIONS: LayoutOptions = {
  'elk.algorithm': 'layered',
  'elk.direction': 'RIGHT',
  'elk.edgeRouting': 'ORTHOGONAL',
  'elk.hierarchyHandling': 'INCLUDE_CHILDREN',
  'elk.json.shapeCoords': 'ROOT',
  'elk.json.edgeCoords': 'ROOT',
  // Model order is the success topology / runtime sequence: edges against it are the loops.
  'elk.layered.cycleBreaking.strategy': 'MODEL_ORDER',
  'elk.layered.considerModelOrder.strategy': 'NODES_AND_EDGES',
  // INTERACTIVE stacks the model-ordered main chain on one straight track.
  // Port-perfect strategies (BRANDES_KOEPF) straighten edges instead, which
  // drifts nodes whenever a split node's 34% success port meets a single
  // node's 50% target port — violating the "success main chain keeps one
  // column/row" contract (task-workflow.md).
  'elk.layered.nodePlacement.strategy': 'BRANDES_KOEPF',
  'elk.layered.spacing.nodeNodeBetweenLayers': '32',
  'elk.spacing.nodeNode': '48',
  'elk.layered.spacing.edgeNodeBetweenLayers': '24',
  'elk.spacing.edgeNode': '24',
  'elk.spacing.edgeEdge': '14',
  'elk.layered.spacing.edgeEdgeBetweenLayers': '14',
  'elk.spacing.edgeLabel': '4',
  'elk.edgeLabels.placement': 'CENTER',
  'elk.layered.edgeLabels.centerLabelPlacementStrategy': 'MEDIAN_LAYER',
  'elk.padding': '[top=40,left=40,bottom=40,right=40]',
};
const ELK_GROUP_PADDING = '[top=40,left=20,bottom=20,right=20]';
// Semi-interactive crossing minimization keeps the in-layer order implied by
// `elk.position` for the pinned pair only; every other node is still optimized.
const PIN_ANCHOR_POSITION = '(0,0)';
const PINNED_POSITION = '(0,100000)';
const PINNED_POSITION_DOWN = '(100000,0)';

function port(id: string, x: number, y: number, side: 'EAST' | 'WEST' | 'NORTH' | 'SOUTH'): ElkPort {
  return { id, x, y, width: 0, height: 0, layoutOptions: { 'elk.port.side': side } };
}

function inPortId(nodeId: string) {
  return `${nodeId}::in`;
}

function outPortId(node: WorkflowLayoutNodeSpec, outcome: WorkflowLayoutOutcome) {
  if (node.sourceHandles !== 'split') return `${node.id}::out`;
  return `${node.id}::${outcome === 'failure' ? 'failure' : 'success'}`;
}

/**
 * Every main-chain port — targets, single-outcome sources, and terminal
 * entries — shares the split success lane so mixed split/single chains get
 * node columns, straight edges, and handle dots all on one fraction. INTERACTIVE
 * placement keeps the columns; matching ports keep the hops straight without
 * any label-side jogs.
 */
export function workflowTargetHandleRatio(): number {
  return WORKFLOW_NODE_OUTCOME_HANDLE_RATIO.success;
}

function elkNode(node: WorkflowLayoutNodeSpec, pinAnchor: boolean, direction: WorkflowLayoutDirection): ElkNode {
  const vertical = direction === 'DOWN';
  const inRatio = workflowTargetHandleRatio();
  const inX = node.width * inRatio;
  const inY = node.height * inRatio;
  // Ports mirror across the flow axis: RIGHT flows WEST→EAST, DOWN flows NORTH→SOUTH.
  // The pinned new-round target accepts backward edges on its downstream side.
  const ports = [
    vertical
      ? node.pinBelow
        ? port(inPortId(node.id), inX, node.height, 'SOUTH')
        : port(inPortId(node.id), inX, 0, 'NORTH')
      : node.pinBelow
        ? port(inPortId(node.id), node.width, inY, 'EAST')
        : port(inPortId(node.id), 0, inY, 'WEST'),
  ];
  if (node.sourceHandles === 'split') {
    ports.push(
      vertical
        ? port(`${node.id}::success`, node.width * WORKFLOW_NODE_OUTCOME_HANDLE_RATIO.success, node.height, 'SOUTH')
        : port(`${node.id}::success`, node.width, node.height * WORKFLOW_NODE_OUTCOME_HANDLE_RATIO.success, 'EAST'),
      vertical
        ? port(`${node.id}::failure`, node.width * WORKFLOW_NODE_OUTCOME_HANDLE_RATIO.failure, node.height, 'SOUTH')
        : port(`${node.id}::failure`, node.width, node.height * WORKFLOW_NODE_OUTCOME_HANDLE_RATIO.failure, 'EAST'),
    );
  } else if (node.sourceHandles === 'single') {
    ports.push(
      vertical
        ? port(`${node.id}::out`, node.width * WORKFLOW_NODE_OUTCOME_HANDLE_RATIO.success, node.height, 'SOUTH')
        : port(`${node.id}::out`, node.width, node.height * WORKFLOW_NODE_OUTCOME_HANDLE_RATIO.success, 'EAST'),
    );
  }
  return {
    id: node.id,
    width: node.width,
    height: node.height,
    ports,
    layoutOptions: {
      'elk.portConstraints': 'FIXED_POS',
      ...(node.pinBelow ? { 'elk.layered.layering.layerConstraint': 'FIRST', 'elk.position': vertical ? PINNED_POSITION_DOWN : PINNED_POSITION } : {}),
      ...(pinAnchor ? { 'elk.position': PIN_ANCHOR_POSITION } : {}),
    },
  };
}

/** Converts a layout spec into an ELK graph. Groups keep the model order of their first member. */
export function buildElkGraph(spec: WorkflowLayoutSpec): ElkNode {
  const nodeById = new Map(spec.nodes.map((node) => [node.id, node]));
  const groupById = new Map(spec.groups.map((group) => [group.id, group]));
  const pinAnchors = new Set(spec.nodes.flatMap((node) => (node.pinBelow && nodeById.has(node.pinBelow) ? [node.pinBelow] : [])));
  const groupChain = (groupId: string | null | undefined): string[] => {
    const chain: string[] = [];
    let current = groupId && groupById.has(groupId) ? groupId : null;
    while (current && !chain.includes(current)) {
      chain.unshift(current);
      const parent = groupById.get(current)?.parentId;
      current = parent && groupById.has(parent) ? parent : null;
    }
    return chain;
  };

  type Child = { kind: 'node' | 'group'; id: string };
  const childrenByContainer = new Map<string | null, Child[]>();
  const seen = new Set<string>();
  const addChild = (container: string | null, child: Child) => {
    const key = `${child.kind}:${child.id}`;
    if (seen.has(key)) return;
    seen.add(key);
    childrenByContainer.set(container, [...(childrenByContainer.get(container) ?? []), child]);
  };
  for (const node of spec.nodes) {
    let container: string | null = null;
    for (const groupId of groupChain(node.groupId)) {
      addChild(container, { kind: 'group', id: groupId });
      container = groupId;
    }
    addChild(container, { kind: 'node', id: node.id });
  }
  const buildChildren = (container: string | null): ElkNode[] => (childrenByContainer.get(container) ?? []).map((child) => (
    child.kind === 'node'
      ? elkNode(nodeById.get(child.id)!, pinAnchors.has(child.id), spec.direction ?? 'RIGHT')
      : { id: child.id, layoutOptions: { 'elk.padding': ELK_GROUP_PADDING }, children: buildChildren(child.id) }
  ));

  const edges: ElkExtendedEdge[] = spec.edges.flatMap((edge) => {
    const source = nodeById.get(edge.from);
    if (!source || source.sourceHandles === 'none' || !nodeById.has(edge.to)) return [];
    const priority = edge.outcome === 'failure' ? '0' : '10';
    return [{
      id: edge.id,
      sources: [outPortId(source, edge.outcome)],
      targets: [inPortId(edge.to)],
      // Edge labels are NOT attached to ELK: the inline label dummy rarely
      // lands on the port line, and the orthogonal router then detours every
      // edge through it — an S-shaped 5px jog that reads as squiggly lines.
      // Labels are placed by the app at the route's half-length point instead
      // (readElkLayout).
      labels: [],
      layoutOptions: {
        // Keep the success spine straight; failure / retry edges may bend.
        'elk.layered.priority.straightness': priority,
        'elk.layered.priority.direction': priority,
      },
    }];
  });

  return {
    id: 'root',
    layoutOptions: {
      ...ELK_LAYOUT_OPTIONS,
      'elk.direction': spec.direction ?? 'RIGHT',
      'elk.layered.crossingMinimization.semiInteractive': String(pinAnchors.size > 0),
    },
    children: buildChildren(null),
    edges,
  };
}

/** Reads absolute rects and routes from an ELK result laid out with ROOT coordinates. */
export function readElkLayout(result: ElkNode, direction: WorkflowLayoutDirection): WorkflowLayout {
  const nodes = new Map<string, Rect>();
  const groups = new Map<string, Rect>();
  const edges = new Map<string, WorkflowLayoutRoute>();
  const visit = (container: ElkNode) => {
    for (const child of container.children ?? []) {
      const rect = { x: child.x ?? 0, y: child.y ?? 0, width: child.width ?? 0, height: child.height ?? 0 };
      // Only groups contain children; empty groups are never emitted by buildElkGraph.
      (child.children?.length ? groups : nodes).set(child.id, rect);
      visit(child);
    }
    for (const edge of (container.edges ?? []) as ElkExtendedEdge[]) {
      const section = edge.sections?.[0];
      if (!section) continue;
      const points = [section.startPoint, ...(section.bendPoints ?? []), section.endPoint].map(({ x, y }) => ({ x, y }));
      const label = edge.labels?.[0];
      const labelPoint = label?.x !== undefined && label.y !== undefined
        ? { x: label.x + (label.width ?? 0) / 2, y: label.y + (label.height ?? 0) / 2 }
        : pointAtHalfLength(points);
      edges.set(edge.id, { points, path: roundedOrthogonalPath(points), labelX: labelPoint.x, labelY: labelPoint.y });
    }
  };
  visit(result);
  return { nodes, groups, edges, bounds: layoutBounds([...nodes.values(), ...groups.values()]), direction };
}

export async function layoutWorkflowGraph(spec: WorkflowLayoutSpec, elk: ELK): Promise<WorkflowLayout> {
  const direction = spec.direction ?? 'RIGHT';
  return readElkLayout(await elk.layout(buildElkGraph(spec)), direction);
}

function layoutBounds(rects: Rect[]): Rect | null {
  if (rects.length === 0) return null;
  const left = Math.min(...rects.map((rect) => rect.x));
  const top = Math.min(...rects.map((rect) => rect.y));
  const right = Math.max(...rects.map((rect) => rect.x + rect.width));
  const bottom = Math.max(...rects.map((rect) => rect.y + rect.height));
  return { x: left, y: top, width: right - left, height: bottom - top };
}

export function roundedOrthogonalPath(points: Point[], radius = EDGE_CORNER_RADIUS): string {
  if (points.length === 0) return '';
  let path = `M ${points[0].x} ${points[0].y}`;
  for (let index = 1; index < points.length - 1; index += 1) {
    const previous = points[index - 1];
    const current = points[index];
    const next = points[index + 1];
    const inLength = Math.hypot(current.x - previous.x, current.y - previous.y);
    const outLength = Math.hypot(next.x - current.x, next.y - current.y);
    const r = Math.min(radius, inLength / 2, outLength / 2);
    if (r <= 0) {
      path += ` L ${current.x} ${current.y}`;
      continue;
    }
    const before = { x: current.x - ((current.x - previous.x) / inLength) * r, y: current.y - ((current.y - previous.y) / inLength) * r };
    const after = { x: current.x + ((next.x - current.x) / outLength) * r, y: current.y + ((next.y - current.y) / outLength) * r };
    path += ` L ${before.x} ${before.y} Q ${current.x} ${current.y} ${after.x} ${after.y}`;
  }
  const last = points[points.length - 1];
  return `${path} L ${last.x} ${last.y}`;
}

function pointAtHalfLength(points: Point[]): Point {
  const lengths = points.slice(1).map((point, index) => Math.hypot(point.x - points[index].x, point.y - points[index].y));
  let remaining = lengths.reduce((sum, length) => sum + length, 0) / 2;
  for (let index = 0; index < lengths.length; index += 1) {
    if (remaining <= lengths[index]) {
      const ratio = lengths[index] === 0 ? 0 : remaining / lengths[index];
      return {
        x: points[index].x + (points[index + 1].x - points[index].x) * ratio,
        y: points[index].y + (points[index + 1].y - points[index].y) * ratio,
      };
    }
    remaining -= lengths[index];
  }
  return points[points.length - 1];
}

export function calculateCenteredViewport(bounds: { x: number; y: number; width: number; height: number }, viewport: { width: number; height: number }, padding: number, maxZoom: number, horizontalAnchor: number, verticalAnchor: number) {
  const availableWidth = viewport.width * Math.max(0.1, 1 - padding * 2);
  const availableHeight = viewport.height * Math.max(0.1, 1 - padding * 2);
  const fitZoom = Math.min(availableWidth / bounds.width, availableHeight / bounds.height);
  const zoom = Math.min(fitZoom, maxZoom);
  const centerX = bounds.x + bounds.width / 2;
  const centerY = bounds.y + bounds.height / 2;
  return {
    x: viewport.width * horizontalAnchor - centerX * zoom,
    y: viewport.height * verticalAnchor - centerY * zoom,
    zoom,
  };
}

/**
 * Fits layout bounds like React Flow's `fitView` (padding is a fraction of the bounds).
 * When the graph cannot fit at `minZoom`, it keeps the entry side (left when flowing
 * RIGHT, top when flowing DOWN) — where the entry and the pinned new Round target
 * live — in view instead of clipping both ends.
 */
export function entryAnchoredViewport(bounds: Rect, viewport: { width: number; height: number }, options: { padding: number; minZoom: number; maxZoom: number; direction: WorkflowLayoutDirection }) {
  const scale = 1 + options.padding;
  const fitZoom = Math.min(viewport.width / (bounds.width * scale), viewport.height / (bounds.height * scale));
  const zoom = Math.min(options.maxZoom, Math.max(options.minZoom, fitZoom));
  const marginX = (viewport.width - viewport.width / scale) / 2;
  const marginY = (viewport.height - viewport.height / scale) / 2;
  const centerX = viewport.width / 2 - (bounds.x + bounds.width / 2) * zoom;
  const centerY = viewport.height / 2 - (bounds.y + bounds.height / 2) * zoom;
  if (options.direction === 'DOWN') {
    return {
      x: centerX,
      y: Math.max(centerY, marginY - bounds.y * zoom),
      zoom,
    };
  }
  return {
    x: Math.max(centerX, marginX - bounds.x * zoom),
    y: centerY,
    zoom,
  };
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

/** Resolves the node a `$new-round` edge restarts from. */
export function newRoundEntryNodeId(workflow: Pick<WorkflowDsl, 'entry'>, edge: Pick<WorkflowEdgeDsl, 'new_round_entry'>): string {
  return !edge.new_round_entry || edge.new_round_entry === ENTRY_NODE ? workflow.entry : edge.new_round_entry;
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

export function runtimeEdgeId(edge: Pick<GraphEdgeVm, 'from' | 'to'>, index: number): string {
  return `${edge.from}-${edge.to}-${index}`;
}

/**
 * Runtime layout input. Status, traversal counts and blocked reasons are excluded
 * so status refreshes never re-run layout; edge labels never enter the spec —
 * they ride the route's half-length point and cannot churn the cache.
 */
export function runtimeGraphLayoutSpec(graph: GraphVm, translate: (value: string) => string, direction: WorkflowLayoutDirection = 'RIGHT'): WorkflowLayoutSpec {
  const order = runtimeNodeOrder(graph.nodes);
  const nodes = [...graph.nodes].sort((a, b) => order.get(a.id)! - order.get(b.id)!);
  const nodeIds = new Set(nodes.map((node) => node.id));
  // Terminals mirror the authoring contract: $end is a passive sink and the
  // new-round node pins back to the entry row, so its long backward edge
  // never crowds the tail layer and push the spine off its track.
  const isTerminal = (node: GraphNodeVm) => node.id === END_NODE
    || node.id === NEW_ROUND_NODE
    || node.nodeType === 'terminal';
  const hasIncoming = (id: string) => graph.edges.some((edge) => edge.to === id && nodeIds.has(edge.from));
  const entryNodeId = nodes.find((node) => !isTerminal(node) && !hasIncoming(node.id))?.id ?? null;
  return {
    direction,
    nodes: nodes.map((node) => {
      const terminal = isTerminal(node);
      return {
        id: node.id,
        width: terminal ? TERMINAL_NODE_WIDTH : NODE_WIDTH,
        height: terminal ? TERMINAL_NODE_HEIGHT : RUNTIME_NODE_HEIGHT,
        sourceHandles: terminal ? 'none' : 'single',
        ...(node.id === NEW_ROUND_NODE && entryNodeId ? { pinBelow: entryNodeId } : {}),
        groupId: node.dynamicGroupId ?? null,
      };
    }),
    edges: graph.edges.flatMap((edge, index) => {
      if (!nodeIds.has(edge.from) || !nodeIds.has(edge.to)) return [];
      const label = edge.label?.toLowerCase() ?? '';
      return [{
        id: runtimeEdgeId(edge, index),
        from: edge.from,
        to: edge.to,
        outcome: label === 'failure' ? 'failure' : label === 'success' ? 'success' : 'other',
      }];
    }),
    groups: (graph.groups ?? []).map((group) => ({ id: group.id, parentId: group.parentGroupId ?? null })),
  };
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
