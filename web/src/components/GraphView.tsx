import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import {
  Background,
  BaseEdge,
  Controls,
  EdgeLabelRenderer,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  getSmoothStepPath,
  type Edge,
  type EdgeProps,
  type Node,
  type NodeProps,
  type Viewport,
} from '@xyflow/react';
import type { GraphNodeVm, GraphVm } from '../types';
import {
  NODE_WIDTH,
  NODE_HEIGHT,
  runtimeNodeOrder,
  computeBackwardLanes,
  isRuntimePrimaryEdge,
  layoutSuccessPath,
  runtimeEdgeColor,
  topLeft,
  type DagreNodeSpec,
} from './workflowGraph';
import { displayStatus } from '../i18n';
import { Badge } from '@/components/ui/badge';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { EmptyState } from '@/components/PageScaffold';
import { cn } from '@/lib/utils';
import { statusBadgeClass } from '@/lib/status';

/** Runtime graph nodes use a slightly taller card for status badges. */
const RUNTIME_NODE_HEIGHT = 138;
const MIN_ZOOM = 0.35;
const MAX_ZOOM = 1.2;
const WORKFLOW_FIT_MAX_ZOOM = 0.88;
const ACTUAL_FIT_MAX_ZOOM = 0.82;

type GraphMode = 'readonly' | 'interactive';

type WorkflowNodeData = {
  node: GraphNodeVm;
  selected: boolean;
  active: boolean;
  running: boolean;
  mode: GraphMode;
  currentLabel: string;
  runningLabel: string;
  displayStatusValue: string | null;
  displayTone: string;
  displayIcon: string;
  statusLabel: string;
  artifactLabel: string;
  attachmentLabel: string;
  iconKey?: string | null;
};

interface GraphViewProps {
  graph: GraphVm;
  selectedNodeId?: string | null;
  activeNodeId?: string | null;
  onNodeSelect?: (node: GraphNodeVm) => void;
  onNodeOpenDetail?: (node: GraphNodeVm) => void;
  onNodeOpenSession?: (node: GraphNodeVm) => void;
  onNodeOpenLog?: (node: GraphNodeVm) => void;
  onNodeContextMenuStart?: (node: GraphNodeVm) => number | void;
  variant?: 'grid' | 'workflow' | 'actual';
}

const nodeTypes = {
  workflowNode: WorkflowNode,
};

const edgeTypes = {
  runtimeEdge: RuntimeEdge,
};

export function GraphView({ graph, selectedNodeId, activeNodeId, onNodeSelect, onNodeOpenDetail, onNodeOpenSession, onNodeOpenLog, onNodeContextMenuStart, variant = 'grid' }: GraphViewProps) {
  const { t } = useTranslation();
  const mode: GraphMode = variant === 'actual' ? 'interactive' : 'readonly';
  const { nodes, edges } = useMemo(() => createLayoutedGraph(graph, selectedNodeId, activeNodeId, mode, t), [graph, selectedNodeId, activeNodeId, mode, t]);
  const [menu, setMenu] = useState<{ x: number; y: number; node: GraphNodeVm } | null>(null);
  const [containerElement, setContainerElement] = useState<HTMLDivElement | null>(null);
  const contextMenuTimerRef = useRef<number | null>(null);
  const [viewportSize, setViewportSize] = useState({ width: 0, height: 0 });
  const [viewport, setViewport] = useState<Viewport>({ x: 0, y: 0, zoom: 1 });
  const fitViewOptions = useMemo(() => ({ padding: variant === 'workflow' ? 0.2 : 0.22, maxZoom: variant === 'workflow' ? WORKFLOW_FIT_MAX_ZOOM : ACTUAL_FIT_MAX_ZOOM }), [variant]);
  const viewportHorizontalAnchor = variant === 'actual' ? 0.40 : 0.5;
  const viewportVerticalAnchor = variant === 'actual' ? 0.32 : 0.5;
  const graphSignature = useMemo(() => `${variant}:${graph.nodes.map((node) => node.id).join('|')}:${graph.edges.map((edge) => `${edge.from}>${edge.to}`).join('|')}`, [graph.nodes, graph.edges, variant]);
  const graphBounds = useMemo(() => boundsForNodes(nodes), [graphSignature]);
  const centeredViewport = useMemo(() => {
    if (viewportSize.width === 0 || viewportSize.height === 0 || !graphBounds) return null;
    return calculateCenteredViewport(graphBounds, viewportSize, fitViewOptions.padding, fitViewOptions.maxZoom, viewportHorizontalAnchor, viewportVerticalAnchor);
  }, [fitViewOptions.maxZoom, fitViewOptions.padding, graphBounds, viewportHorizontalAnchor, viewportSize.height, viewportSize.width, viewportVerticalAnchor]);

  useEffect(() => {
    if (!containerElement) return undefined;
    const updateSize = (width: number, height: number) => {
      const next = { width: Math.round(width), height: Math.round(height) };
      setViewportSize((current) => (current.width === next.width && current.height === next.height ? current : next));
    };
    updateSize(containerElement.clientWidth, containerElement.clientHeight);
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      updateSize(entry.contentRect.width, entry.contentRect.height);
    });
    observer.observe(containerElement);
    return () => observer.disconnect();
  }, [containerElement]);

  useEffect(() => {
    if (centeredViewport) setViewport(centeredViewport);
  }, [centeredViewport, graphSignature]);

  useEffect(() => {
    if (!menu) return undefined;
    const close = () => setMenu(null);
    window.addEventListener('click', close);
    window.addEventListener('keydown', close);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('keydown', close);
    };
  }, [menu]);

  useEffect(() => () => {
    if (contextMenuTimerRef.current) window.clearTimeout(contextMenuTimerRef.current);
  }, []);

  const handleNodeClick = useCallback((_: React.MouseEvent, node: Node<WorkflowNodeData>) => {
    if (mode === 'interactive' && onNodeOpenDetail) {
      onNodeOpenDetail(node.data.node);
      return;
    }
    onNodeSelect?.(node.data.node);
  }, [mode, onNodeOpenDetail, onNodeSelect]);

  const handleNodeDoubleClick = useCallback((_: React.MouseEvent, node: Node<WorkflowNodeData>) => {
    onNodeOpenDetail?.(node.data.node);
  }, [onNodeOpenDetail]);

  const handleNodeContextMenu = useCallback((event: React.MouseEvent, node: Node<WorkflowNodeData>) => {
    if (mode !== 'interactive') return;
    event.preventDefault();
    if (contextMenuTimerRef.current) window.clearTimeout(contextMenuTimerRef.current);
    const nextMenu = { x: event.clientX, y: event.clientY, node: node.data.node };
    const delay = onNodeContextMenuStart?.(node.data.node);
    if (delay === undefined) onNodeSelect?.(node.data.node);
    setMenu(null);
    if (delay && delay > 0) {
      contextMenuTimerRef.current = window.setTimeout(() => {
        setMenu(nextMenu);
        contextMenuTimerRef.current = null;
      }, delay);
      return;
    }
    setMenu(nextMenu);
  }, [mode, onNodeContextMenuStart, onNodeSelect]);

  if (graph.nodes.length === 0) {
    return <EmptyState>{t('graph.emptyGraph')}</EmptyState>;
  }

  return (
    <div ref={setContainerElement} className={cn('relative min-w-0 overflow-hidden rounded-xl border bg-muted/15', variant === 'workflow' ? 'h-[360px]' : 'h-full min-h-0')}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        viewport={viewport}
        onViewportChange={setViewport}
        minZoom={MIN_ZOOM}
        maxZoom={MAX_ZOOM}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={mode === 'interactive'}
        panOnDrag
        zoomOnScroll
        zoomOnPinch
        preventScrolling
        proOptions={{ hideAttribution: true }}
        onNodeClick={handleNodeClick}
        onNodeDoubleClick={handleNodeDoubleClick}
        onNodeContextMenu={handleNodeContextMenu}
        className="workflow-graph"
      >
        <Background color="var(--border)" gap={28} size={1} />
        <Controls showInteractive={false} fitViewOptions={fitViewOptions} position="bottom-right" />
      </ReactFlow>
      <div className="pointer-events-none absolute left-4 top-4 rounded-full border bg-card/85 px-3 py-1 font-mono text-[10px] uppercase tracking-[0.18em] text-muted-foreground shadow-sm backdrop-blur">
        {mode === 'interactive' ? t('graph.executionGraph') : t('graph.workflowBlueprint')}
      </div>
      {menu ? (
        <div
          role="menu"
          className="fixed z-50 min-w-36 rounded-md border bg-popover p-1 text-sm text-popover-foreground shadow-md"
          style={{ left: menu.x, top: menu.y }}
          onClick={(event) => event.stopPropagation()}
        >
          <GraphMenuItem onClick={() => onNodeOpenDetail?.(menu.node)}>{t('graph.viewNodeDetail')}</GraphMenuItem>
          <GraphMenuItem disabled={!onNodeOpenLog} onClick={() => onNodeOpenLog?.(menu.node)}>{t('graph.viewLog')}</GraphMenuItem>
          <GraphMenuItem disabled={!menu.node.attemptId} onClick={() => onNodeOpenSession?.(menu.node)}>{t('graph.viewSession')}</GraphMenuItem>
          <GraphMenuItem onClick={() => navigator.clipboard?.writeText(menu.node.nodeId ?? menu.node.id)}>{t('graph.copyNodeId')}</GraphMenuItem>
          <GraphMenuItem disabled>{t('graph.retryFromNode')}</GraphMenuItem>
        </div>
      ) : null}
    </div>
  );
}

function boundsForNodes(nodes: Node<WorkflowNodeData>[]) {
  if (nodes.length === 0) return null;
  const left = Math.min(...nodes.map((node) => node.position.x));
  const top = Math.min(...nodes.map((node) => node.position.y));
  const right = Math.max(...nodes.map((node) => node.position.x + NODE_WIDTH));
  const bottom = Math.max(...nodes.map((node) => node.position.y + RUNTIME_NODE_HEIGHT));
  return { x: left, y: top, width: right - left, height: bottom - top };
}

function calculateCenteredViewport(bounds: { x: number; y: number; width: number; height: number }, viewport: { width: number; height: number }, padding: number, maxZoom: number, horizontalAnchor: number, verticalAnchor: number): Viewport {
  const availableWidth = viewport.width * Math.max(0.1, 1 - padding * 2);
  const availableHeight = viewport.height * Math.max(0.1, 1 - padding * 2);
  const fitZoom = Math.min(availableWidth / bounds.width, availableHeight / bounds.height);
  const zoom = Math.min(Math.max(fitZoom, MIN_ZOOM), maxZoom, MAX_ZOOM);
  const centerX = bounds.x + bounds.width / 2;
  const centerY = bounds.y + bounds.height / 2;
  return {
    x: viewport.width * horizontalAnchor - centerX * zoom,
    y: viewport.height * verticalAnchor - centerY * zoom,
    zoom,
  };
}

function createLayoutedGraph(graph: GraphVm, selectedNodeId: string | null | undefined, activeNodeId: string | null | undefined, mode: GraphMode, t: TFunction) {
  const nodeOrder = runtimeNodeOrder(graph.nodes);
  const nodeIds = new Set(graph.nodes.map((n) => n.id));
  const layoutPositions = layoutSuccessPath(
    graph.nodes.map((n) => ({ id: n.id, width: NODE_WIDTH, height: RUNTIME_NODE_HEIGHT })),
    graph.edges.map((e) => ({ from: e.from, to: e.to, on: isRuntimePrimaryEdge(e, nodeOrder) ? 'success' : e.label?.toLowerCase() ?? '' })),
    nodeIds,
    nodeOrder,
  );

  const activeNode = graph.nodes.find((node) => matchesNodeId(node, activeNodeId));
  const runningActiveNode = activeNode?.runtimeDisplay?.tone === 'running';
  const activeNodeKey = activeNode?.id ?? activeNode?.nodeId ?? activeNodeId ?? null;
  const nodes: Node<WorkflowNodeData>[] = graph.nodes.map((node) => {
    const pos = layoutPositions.get(node.id) ?? { x: 0, y: 0 };
    const displayStatusValue = node.runtimeDisplay?.code ?? null;
    const displayTone = node.runtimeDisplay?.tone ?? 'neutral';
    const displayIcon = node.runtimeDisplay?.icon ?? 'dot';
    const active = matchesNodeId(node, activeNodeId) || node.current;
    const running = active && (runningActiveNode || displayTone === 'running');
    return {
      id: node.id,
      type: 'workflowNode',
      position: topLeft(pos.x, pos.y, NODE_WIDTH, RUNTIME_NODE_HEIGHT),
      data: {
        node,
        selected: selectedNodeId === node.id || selectedNodeId === node.nodeId,
        active,
        running,
        mode,
        currentLabel: t('graph.current'),
        runningLabel: displayStatus(t, 'running'),
        displayStatusValue,
        displayTone,
        displayIcon,
        statusLabel: displayStatus(t, displayStatusValue),
        artifactLabel: t('common.artifacts'),
        attachmentLabel: t('common.attachments'),
        iconKey: node.iconKey,
      },
      draggable: false,
      selectable: mode === 'interactive',
    };
  });

  const backwardLanes = computeBackwardLanes(
    graph.edges.map((edge) => ({ from: edge.from, to: edge.to, on: edge.label?.toLowerCase() ?? '' })),
    nodeOrder,
  );
  const edges: Edge[] = graph.edges.map((edge, index) => {
    const activeEdge = Boolean(runningActiveNode && activeNodeKey && edge.to === activeNodeKey);
    const color = runtimeEdgeColor(edge, activeEdge);
    const label = edge.blockedReason
      ? `${edge.label || ''} · ${edge.blockedReason.proposedCount ?? '-'}/${edge.blockedReason.limit ?? '-'}`
      : edge.traversalCount && edge.traversalCount > 1
        ? `${edge.label || ''} ×${edge.traversalCount}`
        : edge.label || undefined;
    return {
      id: `${edge.from}-${edge.to}-${index}`,
      source: edge.from,
      target: edge.to,
      label,
      type: 'runtimeEdge',
      markerEnd: { type: MarkerType.ArrowClosed, width: 18, height: 18, color },
      style: { stroke: color, strokeWidth: activeEdge ? 2.4 : 1.8 },
      className: activeEdge ? 'workflow-edge-running' : undefined,
      data: {
        color,
        label,
        lane: backwardLanes.get(index),
      },
    };
  });

  return { nodes, edges };
}

function RuntimeEdge({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerEnd, style, data }: EdgeProps) {
  const lane = typeof data?.lane === 'number' ? data.lane : null;
  const color = typeof data?.color === 'string' ? data.color : style?.stroke;
  const label = typeof data?.label === 'string' ? data.label : null;
  const [smoothPath, smoothLabelX, smoothLabelY] = getSmoothStepPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition });
  const laneY = lane === null ? null : Math.min(sourceY, targetY) - 88 - lane * 38;
  const sourceOffsetX = sourceX + 34;
  const targetOffsetX = targetX - 34;
  const path = laneY === null
    ? smoothPath
    : `M ${sourceX},${sourceY} L ${sourceOffsetX},${sourceY} L ${sourceOffsetX},${laneY} L ${targetOffsetX},${laneY} L ${targetOffsetX},${targetY} L ${targetX},${targetY}`;
  const labelX = laneY === null ? smoothLabelX : (sourceOffsetX + targetOffsetX) / 2;
  const labelY = laneY === null ? smoothLabelY : laneY;
  return (
    <>
      <BaseEdge path={path} markerEnd={markerEnd} style={style} className="workflow-edge-flow" />
      {label ? (
        <EdgeLabelRenderer>
          <span
            className="pointer-events-none absolute rounded-full border bg-background/95 px-2 py-0.5 font-mono text-[11px] shadow-sm"
            style={{ color, transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }}
          >
            {label}
          </span>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
}

function matchesNodeId(node: GraphNodeVm, id?: string | null) {
  return Boolean(id && (node.id === id || node.nodeId === id));
}

function WorkflowNode({ data }: NodeProps<Node<WorkflowNodeData>>) {
  const { node, selected, active, running, mode, currentLabel, runningLabel, displayStatusValue, displayTone, displayIcon, statusLabel, artifactLabel, attachmentLabel, iconKey } = data;
  const hasStatus = Boolean(displayStatusValue);
  const isDynamicNode = (node.nodeType ?? '').startsWith('dynamic-');
  return (
    <div
      className={cn(
        'relative flex h-[138px] w-[226px] flex-col overflow-hidden rounded-xl border border-border/65 bg-card text-card-foreground shadow-sm transition-shadow',
        isDynamicNode && 'border-accent/35 bg-accent/5',
        selected && 'border-primary/80 bg-primary/5 ring-2 ring-primary/25 shadow-[0_0_0_1px_rgba(245,158,11,0.26),0_10px_28px_rgba(245,158,11,0.12)]',
        active && !running && !selected && 'border-border/80 bg-card shadow-sm',
        running && 'workflow-node-running border-gold-running/70 bg-gold-running/10 shadow-[0_0_0_1px_color-mix(in_srgb,var(--gold-running)_24%,transparent),0_14px_34px_color-mix(in_srgb,var(--gold-running)_16%,transparent)]',
        mode === 'interactive' && 'cursor-pointer hover:border-primary/45 hover:shadow-md',
      )}
    >
      <Handle type="target" position={Position.Left} className="!size-2 !border-2 !border-card !bg-muted-foreground" />
      <Handle type="source" position={Position.Right} className="!size-2 !border-2 !border-card !bg-muted-foreground" />
      <div className="pointer-events-none absolute left-3 right-3 top-2 z-10 flex items-start justify-between gap-2">
        <div className="flex min-w-0 flex-wrap items-center gap-1.5">
          {iconKey ? <img src={`/agent-icons/${iconKey}.svg`} alt="" className="size-4 shrink-0 rounded-sm" /> : null}
          {isDynamicNode ? <Badge variant="outline" className="h-5 border-accent/35 bg-accent/10 px-1.5 text-[10px] text-accent-foreground">AI-DYNAMIC</Badge> : null}
          {node.attemptCount && node.attemptCount > 1 ? <Badge variant="outline" className="h-5 px-1.5 text-[10px]">attempt ×{node.attemptCount}</Badge> : null}
          {node.artifactCount > 0 ? <Badge variant="secondary" className="h-5 px-1.5 text-[10px]">{artifactLabel}:{node.artifactCount}</Badge> : null}
          {node.attachmentCount > 0 ? <Badge variant="secondary" className="h-5 px-1.5 text-[10px]">{attachmentLabel}:{node.attachmentCount}</Badge> : null}
        </div>
        {running ? <Badge className="h-5 shrink-0 gap-1.5 bg-gold-running px-1.5 text-[10px] text-white"><span className="workflow-running-dot bg-white" />{runningLabel}</Badge> : node.current ? <Badge variant="outline" className={cn('h-5 shrink-0 px-1.5 text-[10px]', displayStatusValue ? statusBadgeClass(displayStatusValue, displayTone) : 'border-primary/35 bg-primary/10 text-primary')}>{displayStatusValue ? statusLabel : currentLabel}</Badge> : null}
      </div>
      <div className="flex min-h-0 flex-1 items-center gap-3 px-4 py-1">
        {hasStatus ? (
          <span aria-label={statusLabel} className={cn('flex size-7 shrink-0 items-center justify-center rounded-full text-sm font-semibold text-white shadow-sm', statusMarkClass(displayTone), running && 'workflow-running-mark')}>
            {statusMark(displayIcon)}
          </span>
        ) : null}
        <div className="min-w-0">
          <Tooltip>
            <TooltipTrigger asChild>
              <p className="line-clamp-2 text-sm font-medium leading-5 text-foreground">{node.label}</p>
            </TooltipTrigger>
            <TooltipContent className="max-w-[360px] whitespace-pre-wrap break-words" sideOffset={6}>{node.label}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <p className="mt-1 truncate font-mono text-[10px] uppercase tracking-[0.16em] text-muted-foreground">{node.nodeId ?? node.id} · {node.nodeType}</p>
            </TooltipTrigger>
            <TooltipContent className="max-w-[360px] whitespace-pre-wrap break-words" sideOffset={6}>{node.nodeId ?? node.id} · {node.nodeType}</TooltipContent>
          </Tooltip>
        </div>
      </div>
    </div>
  );
}

function statusMark(icon: string | null | undefined) {
  if (icon === 'pause') return 'Ⅱ';
  if (icon === 'check') return '✓';
  if (icon === 'error') return '!';
  if (icon === 'dot') return '•';
  return '';
}

function statusMarkClass(tone: string) {
  return cn(
    tone === 'success' && 'bg-gold-success',
    tone === 'running' && 'bg-gold-running',
    tone === 'warning' && 'bg-gold-warning',
    tone === 'danger' && 'bg-gold-danger',
    tone === 'neutral' && 'bg-muted-foreground',
  );
}

function GraphMenuItem({ children, disabled = false, onClick }: { children: React.ReactNode; disabled?: boolean; onClick?: () => void }) {
  return (
    <button
      type="button"
      className="flex w-full items-center rounded-sm px-2 py-1.5 text-left outline-hidden hover:bg-accent hover:text-accent-foreground disabled:pointer-events-none disabled:opacity-50"
      disabled={disabled}
      onClick={() => onClick?.()}
    >
      {children}
    </button>
  );
}
