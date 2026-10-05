import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import {
  Background,
  BaseEdge,
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
  type ReactFlowInstance,
  type Viewport,
} from '@xyflow/react';
import type { GraphNodeVm, GraphVm } from '../types';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import {
  calculateCenteredViewport,
  runtimeEdgeId,
  runtimeGraphEdgeClassName,
  runtimeGraphEdgeDisplayLabel,
  runtimeGraphLayoutSpec,
  runtimeEdgeColor,
  type WorkflowLayout,
  type WorkflowLayoutRoute,
} from './workflowGraph';
import { useWorkflowLayout } from '@/hooks/useWorkflowLayout';
import { WorkflowLayoutStatus } from '@/components/WorkflowLayoutStatus';
import { displayStatus } from '../i18n';
import { Badge } from '@/components/ui/badge';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { EmptyState } from '@/components/PageScaffold';
import { cn } from '@/lib/utils';
import { statusBadgeClass } from '@/lib/status';
import { GraphControls } from '@/components/GraphControls';

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
  workflowNode: memo(WorkflowNode),
  workflowGroup: memo(WorkflowGroup),
};

const edgeTypes = {
  runtimeEdge: memo(RuntimeEdge),
};

export function GraphView({ graph, selectedNodeId, activeNodeId, onNodeSelect, onNodeOpenDetail, onNodeOpenSession, onNodeOpenLog, onNodeContextMenuStart, variant = 'grid' }: GraphViewProps) {
  const { t } = useTranslation();
  const mode: GraphMode = variant === 'actual' ? 'interactive' : 'readonly';
  const layoutSpec = useMemo(() => runtimeGraphLayoutSpec(graph, (value) => displayStatus(t, value)), [graph, t]);
  const layoutState = useWorkflowLayout(layoutSpec);
  const graphLayout = layoutState.layout;
  const { nodes, edges } = useMemo(() => createLayoutedGraph(graph, graphLayout, selectedNodeId, activeNodeId, mode, t), [activeNodeId, graph, graphLayout, mode, selectedNodeId, t]);
  const [menu, setMenu] = useState<{ x: number; y: number; node: GraphNodeVm } | null>(null);
  const [containerElement, setContainerElement] = useState<HTMLDivElement | null>(null);
  const contextMenuTimerRef = useRef<number | null>(null);
  const [viewportSize, setViewportSize] = useState({ width: 0, height: 0 });
  const [viewport, setViewport] = useState<Viewport>({ x: 0, y: 0, zoom: 1 });
  const [flowInstance, setFlowInstance] = useState<ReactFlowInstance<Node<WorkflowNodeData>, Edge> | null>(null);
  const fitViewOptions = useMemo(() => ({ padding: variant === 'workflow' ? 0.2 : 0.22, maxZoom: variant === 'workflow' ? WORKFLOW_FIT_MAX_ZOOM : ACTUAL_FIT_MAX_ZOOM }), [variant]);
  const viewportHorizontalAnchor = variant === 'actual' ? 0.40 : 0.5;
  const viewportVerticalAnchor = variant === 'actual' ? 0.32 : 0.5;
  const graphBounds = graphLayout?.bounds ?? null;
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
  }, [centeredViewport, graphLayout]);

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
    if (!isWorkflowNode(node)) return;
    if (mode === 'interactive' && onNodeOpenDetail) {
      onNodeOpenDetail(node.data.node);
      return;
    }
    onNodeSelect?.(node.data.node);
  }, [mode, onNodeOpenDetail, onNodeSelect]);

  const handleNodeDoubleClick = useCallback((_: React.MouseEvent, node: Node<WorkflowNodeData>) => {
    if (!isWorkflowNode(node)) return;
    onNodeOpenDetail?.(node.data.node);
  }, [onNodeOpenDetail]);

  const handleNodeContextMenu = useCallback((event: React.MouseEvent, node: Node<WorkflowNodeData>) => {
    if (mode !== 'interactive' || !isWorkflowNode(node)) return;
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
        minZoom={Math.min(MIN_ZOOM, centeredViewport?.zoom ?? MIN_ZOOM)}
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
        onInit={setFlowInstance}
        className="workflow-graph"
      >
        <Background color="var(--border)" gap={28} size={1} />
        <GraphControls
          disabled={!flowInstance}
          onZoomIn={() => { void flowInstance?.zoomIn(); }}
          onZoomOut={() => { void flowInstance?.zoomOut(); }}
          onFitView={() => { if (centeredViewport) setViewport(centeredViewport); }}
        />
      </ReactFlow>
      <WorkflowLayoutStatus state={layoutState} />
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

type WorkflowGroupData = { label: string; width: number; height: number };

function createLayoutedGraph(graph: GraphVm, layout: WorkflowLayout | null, selectedNodeId: string | null | undefined, activeNodeId: string | null | undefined, mode: GraphMode, t: TFunction) {
  const activeNode = graph.nodes.find((node) => matchesNodeId(node, activeNodeId));
  const runningActiveNode = activeNode?.runtimeDisplay?.tone === 'running';
  const activeNodeKey = activeNode?.id ?? activeNode?.nodeId ?? activeNodeId ?? null;
  const groups: Node<WorkflowGroupData>[] = [...(layout?.groups ?? [])].map(([id, rect]) => ({
    id: `group:${id}`,
    type: 'workflowGroup',
    position: { x: rect.x, y: rect.y },
    data: { label: id, width: rect.width, height: rect.height },
    zIndex: -1,
    draggable: false,
    selectable: false,
    focusable: false,
  }));
  const nodes: Node<WorkflowNodeData>[] = graph.nodes.map((node) => {
    const rect = layout?.nodes.get(node.id);
    const displayStatusValue = node.runtimeDisplay?.code ?? null;
    const displayTone = node.runtimeDisplay?.tone ?? 'neutral';
    const displayIcon = node.runtimeDisplay?.icon ?? 'dot';
    const active = matchesNodeId(node, activeNodeId) || node.current;
    const running = active && (runningActiveNode || displayTone === 'running');
    return {
      id: node.id,
      type: 'workflowNode',
      position: rect ? { x: rect.x, y: rect.y } : { x: 0, y: 0 },
      // A node added after the last finished layout appears once its position is known.
      hidden: !rect,
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

  const edges: Edge[] = graph.edges.map((edge, index) => {
    const id = runtimeEdgeId(edge, index);
    const activeEdge = Boolean(runningActiveNode && activeNodeKey && edge.to === activeNodeKey);
    const color = runtimeEdgeColor(edge, activeEdge);
    const branch = (edge.label?.toLowerCase() ?? '') !== 'success';
    const label = runtimeGraphEdgeDisplayLabel(edge, (value) => displayStatus(t, value));
    const edgeClassName = runtimeGraphEdgeClassName(activeEdge, branch);
    return {
      id,
      source: edge.from,
      target: edge.to,
      label,
      type: 'runtimeEdge',
      markerEnd: { type: MarkerType.ArrowClosed, width: 18, height: 18, color },
      style: { stroke: color, strokeWidth: activeEdge ? 2.4 : 1.8 },
      className: edgeClassName,
      data: {
        color,
        label,
        route: layout?.edges.get(id),
        edgeClassName,
      },
    };
  });

  return { nodes: [...groups, ...nodes] as Node<WorkflowNodeData>[], edges };
}

function WorkflowGroup({ data }: NodeProps<Node<WorkflowGroupData>>) {
  return (
    <div data-theme-role="workflow-group" className="pointer-events-none rounded-2xl border border-dashed border-muted-foreground/60 bg-muted/75" style={{ width: data.width, height: data.height }}>
      <span className="ml-3 mt-2 inline-block max-w-[calc(100%-1.5rem)] truncate rounded-md bg-muted-foreground/12 px-2 py-0.5 font-mono text-[11px] font-medium text-foreground/80">{data.label}</span>
    </div>
  );
}

function RuntimeEdge({ sourceX, sourceY, targetX, targetY, sourcePosition, targetPosition, markerEnd, style, data }: EdgeProps) {
  const route = data?.route as WorkflowLayoutRoute | undefined;
  const color = typeof data?.color === 'string' ? data.color : style?.stroke;
  const label = typeof data?.label === 'string' ? data.label : null;
  const edgeClassName = typeof data?.edgeClassName === 'string' ? data.edgeClassName : null;
  const [smoothPath, smoothLabelX, smoothLabelY] = getSmoothStepPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition });
  const path = route?.path ?? smoothPath;
  const labelX = route?.labelX ?? smoothLabelX;
  const labelY = route?.labelY ?? smoothLabelY;
  return (
    <>
      <BaseEdge path={path} markerEnd={markerEnd} style={style} className={cn('workflow-edge-flow', edgeClassName)} />
      {label ? (
        <EdgeLabelRenderer>
          <span
            className="workflow-edge-label pointer-events-none absolute z-20 rounded-full border bg-background px-2 py-0.5 font-mono text-[11px] font-semibold shadow-sm"
            style={{ color, transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }}
          >
            {label}
          </span>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
}

/** Group frames share the canvas but carry no runtime node. */
function isWorkflowNode(node: Node<WorkflowNodeData>) {
  return node.type === 'workflowNode';
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
          {iconKey ? <img src={agentIconSrc(iconKey)} alt="" className={agentIconClass(iconKey, 'size-4 shrink-0 rounded-sm')} /> : null}
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
