import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { AlertCircle, Check, ChevronDown, ChevronsUpDown, CircleHelp, CornerDownRight, Info, Plus, Redo2, Sparkles, Trash2, Undo2, X } from 'lucide-react';
import {
  Background,
  BaseEdge,
  EdgeLabelRenderer,
  Handle,
  MarkerType,
  NodeToolbar,
  Panel,
  Position,
  ReactFlow,
  getSmoothStepPath,
  type Connection,
  type Edge,
  type EdgeProps,
  type Node,
  type ReactFlowInstance,
  type Viewport,
} from '@xyflow/react';
import { useTranslation } from 'react-i18next';
import { workflowTemplateDisplayName } from '@/lib/workflow-template';
import type { AgentRegistryVm, DynamicAgentRefDsl, DynamicControlDsl, ManagedAgentVm, ProfileVm, WorkflowAiDynamicDynamicAgentStrategyDsl, WorkflowAiDynamicFixedAgentStrategyDsl, WorkflowAiDynamicNodeDsl, WorkflowControlDsl, WorkflowDsl, WorkflowEdgeDsl, WorkflowJsonConditionDsl, WorkflowNodeDsl, WorkflowOutputContractDsl, WorkflowTemplate, WorkflowTemplateStore, WorkflowWorkerNodeDsl } from '../types';
import {
  END_NODE,
  ENTRY_NODE,
  NEW_ROUND_NODE,
  NODE_WIDTH,
  NODE_HEIGHT,
  TERMINAL_NODE_WIDTH,
  TERMINAL_NODE_HEIGHT,
  collectAuthoringNodes,
  workflowSuccessTopologyOrder,
  isBackwardEdge,
  authoringEdgeColor,
  layoutSuccessPath,
  routeWorkflowBranchEdges,
  topLeft,
  SOURCE_POS,
  TARGET_POS,
  type WorkflowGraphBranchRoute,
} from './workflowGraph';
import { AppCard } from '@/components/AppCard';
import {
  AcpModelThoughtSelects,
  findAcpThoughtLevel,
  updateAcpConfigOptionOverride,
} from '@/components/acp/AcpModelThoughtSelects';
import { AcpSingleConfigMenu } from '@/components/acp/AcpSingleConfigMenu';
import { CodeBlock, EmptyState } from '@/components/PageScaffold';
import { Badge } from '@/components/ui/badge';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { displayAppError } from '../i18n';
import { cn } from '@/lib/utils';
import { formatLocalDateTime } from '@/lib/datetime';
import { DEFAULT_AGENT_ICON_KEY, agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import { GraphControls } from '@/components/GraphControls';

export function workflowAgentIconKeys(agents: readonly ManagedAgentVm[]): ReadonlyMap<string, string> {
  return new Map(agents.map((agent) => [agent.agentType, agent.iconKey?.trim() || DEFAULT_AGENT_ICON_KEY]));
}

const UNSPECIFIED_PERMISSION_MODE = '__unspecified_permission_mode__';

type EditorTab = 'canvas' | 'json';

export interface WorkflowEditorSessionDraft {
  workflow: WorkflowDsl;
  tab: EditorTab;
  jsonDraft: string;
  viewport?: Viewport;
}
type EdgeOutcome = 'success' | 'failure';
type SessionMode = 'new' | 'continue';
type EditorNodeData = {
  label: string;
  kind: string;
  terminal?: boolean;
  iconKey?: string;
  entryCandidate?: boolean;
  entryLabel?: string;
  selected?: boolean;
  supportsFailureOutcome?: boolean;
  successLabel?: string;
  failureLabel?: string;
  quickAddLabel?: string;
  deleteLabel?: string;
  onQuickAdd?: (outcome: EdgeOutcome) => void;
  onDelete?: () => void;
};
type WorkflowEdgeData = { outcome: WorkflowEdgeDsl['on']; route?: WorkflowGraphBranchRoute };
export type WorkflowValidationIssue = { message: string; fieldKey?: string; nodeId?: string; nodeIds?: string[]; edgeIndex?: number };
export type WorkflowValidationResult = {
  valid: boolean;
  issues: WorkflowValidationIssue[];
  fieldErrors: Record<string, string[]>;
  sanitizedWorkflow: WorkflowDsl;
};
type TerminalMenu = { x: number; y: number };
const edgeTypes = { workflowRouted: WorkflowRoutedEdge };
const editorNodeTypes = { editorCanvas: EditorCanvasNode };
const SCHEMA_VALIDATION_DELAY_MS = 2000;
const WORKFLOW_HISTORY_LIMIT = 50;
const WORKFLOW_EDITOR_COMPACT_WIDTH = 820;
const WORKFLOW_EDITOR_MIN_ZOOM = 0.3;
const WORKFLOW_EDITOR_MAX_ZOOM = 1.4;
const WORKFLOW_EDITOR_FIT_MAX_ZOOM = 0.92;
const WORKFLOW_EDITOR_DRAFT_DELAY_MS = 180;
const WORKFLOW_NODE_SINGLE_OUTCOME_TOP = '50%';
const WORKFLOW_NODE_SPLIT_OUTCOME_TOP = { success: '34%', failure: '66%' } as const;
const WORKFLOW_NODE_SPLIT_OUTCOME_RATIO = { success: 0.34, failure: 0.66 } as const;

export type WorkflowEditorHistory = { past: WorkflowDsl[]; future: WorkflowDsl[] };

export function mergeBufferedNodePatches(
  pending: Partial<WorkflowNodeDsl>,
  immediate: Partial<WorkflowNodeDsl> = {},
): Partial<WorkflowNodeDsl> {
  return { ...pending, ...immediate };
}

export function recordWorkflowHistory(history: WorkflowEditorHistory, current: WorkflowDsl, limit = WORKFLOW_HISTORY_LIMIT): WorkflowEditorHistory {
  return { past: [...history.past.slice(-(limit - 1)), current], future: [] };
}

export function undoWorkflowHistory(history: WorkflowEditorHistory, current: WorkflowDsl, limit = WORKFLOW_HISTORY_LIMIT): { history: WorkflowEditorHistory; workflow: WorkflowDsl } | null {
  const workflow = history.past.at(-1);
  if (!workflow) return null;
  return {
    workflow,
    history: {
      past: history.past.slice(0, -1),
      future: [current, ...history.future].slice(0, limit),
    },
  };
}

export function redoWorkflowHistory(history: WorkflowEditorHistory, current: WorkflowDsl, limit = WORKFLOW_HISTORY_LIMIT): { history: WorkflowEditorHistory; workflow: WorkflowDsl } | null {
  const workflow = history.future[0];
  if (!workflow) return null;
  return {
    workflow,
    history: {
      past: [...history.past.slice(-(limit - 1)), current],
      future: history.future.slice(1),
    },
  };
}

export function isWorkflowAgentDoctorReady(agent: ManagedAgentVm): boolean {
  return agent.diagnostic?.available === true;
}

export function workflowEditorSupportedAgents(agentRegistry: AgentRegistryVm | null): ManagedAgentVm[] {
  return agentRegistry?.agents ?? [];
}

export function workerAgentSelectionPatch(provider: string): Partial<WorkflowWorkerNodeDsl> {
  return {
    provider,
    permission_mode: undefined,
    model: undefined,
    config_options: undefined,
  };
}

export function nodeSupportsFailureOutcome(node: WorkflowNodeDsl | undefined): boolean {
  return node?.type === 'worker' && Boolean(node.output || node.success_condition);
}

export function removeTerminalFromWorkflow(workflow: WorkflowDsl, terminalId: string): WorkflowDsl {
  if (terminalId !== END_NODE && terminalId !== NEW_ROUND_NODE) return workflow;
  return { ...workflow, edges: workflow.edges.filter((edge) => edge.to !== terminalId) };
}

export function optionalWorkerConfigOptions(
  options: Record<string, string>,
): Record<string, string> | undefined {
  return Object.keys(options).length > 0 ? options : undefined;
}

function AgentSelectItemContent({ agent, unavailableLabel }: { agent: ManagedAgentVm; unavailableLabel: string }) {
  const unavailableReason = isWorkflowAgentDoctorReady(agent)
    ? null
    : (agent.diagnostic?.reason ?? unavailableLabel);
  return (
    <span className="flex min-w-0 flex-col items-start">
      <span>{agent.displayName}</span>
      {unavailableReason ? <span className="max-w-[24rem] truncate text-xs text-destructive">{unavailableReason}</span> : null}
    </span>
  );
}

function EditorCanvasNode({ data }: { data: EditorNodeData }) {
  if (data.terminal) {
    return (
      <div data-theme-role="workflow-node" className="flex size-full items-center justify-center rounded-full border border-dashed border-border/80 bg-muted/20 text-xs tracking-wide text-muted-foreground">
        <Handle type="target" position={Position.Left} className="!size-2 !border-2 !border-card !bg-muted-foreground" />
        {data.label}
      </div>
    );
  }
  const successHandleTop = data.supportsFailureOutcome ? WORKFLOW_NODE_SPLIT_OUTCOME_TOP.success : WORKFLOW_NODE_SINGLE_OUTCOME_TOP;
  return (
      <div data-theme-role="workflow-node" data-selected={data.selected} className="relative flex size-full flex-col items-center justify-center gap-1 border border-border bg-card px-3 py-2">
      <NodeToolbar isVisible={data.selected} position={Position.Bottom} offset={10} className="flex items-center gap-1 rounded-lg border bg-popover p-1 text-popover-foreground shadow-md">
        <Tooltip>
          <TooltipTrigger asChild>
            <Button type="button" variant="ghost" size="sm" className="nodrag nopan h-7 gap-1 px-2 text-xs" onClick={() => data.onQuickAdd?.('success')}>
              <CornerDownRight className="size-3.5 text-emerald-600 dark:text-emerald-400" />
              {data.successLabel}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{data.quickAddLabel}: {data.successLabel}</TooltipContent>
        </Tooltip>
        {data.supportsFailureOutcome ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button type="button" variant="ghost" size="sm" className="nodrag nopan h-7 gap-1 px-2 text-xs" onClick={() => data.onQuickAdd?.('failure')}>
                <CornerDownRight className="size-3.5 text-destructive" />
                {data.failureLabel}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{data.quickAddLabel}: {data.failureLabel}</TooltipContent>
          </Tooltip>
        ) : null}
        <Tooltip>
          <TooltipTrigger asChild>
            <Button type="button" variant="ghost" size="icon-sm" className="nodrag nopan size-7 text-muted-foreground hover:text-destructive" aria-label={data.deleteLabel} onClick={() => data.onDelete?.()}>
              <Trash2 className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{data.deleteLabel}</TooltipContent>
        </Tooltip>
      </NodeToolbar>
      {data.entryCandidate ? (
        <Badge variant="outline" className="pointer-events-none absolute -left-1 -top-2 z-10 h-5 rounded-full bg-background px-1.5 text-[10px] font-medium">
          {data.entryLabel}
        </Badge>
      ) : null}
      <Handle type="target" position={Position.Left} className="!size-2 !border-2 !border-card !bg-muted-foreground" />
      <Tooltip>
        <TooltipTrigger asChild>
          <Handle id="success" type="source" position={Position.Right} className="workflow-handle-success !size-2.5 !border-2 !border-card !bg-emerald-500" style={{ top: successHandleTop }} />
        </TooltipTrigger>
        <TooltipContent>{data.successLabel}</TooltipContent>
      </Tooltip>
      {data.supportsFailureOutcome ? (
        <Tooltip>
          <TooltipTrigger asChild>
            <Handle id="failure" type="source" position={Position.Right} className="workflow-handle-failure !size-2.5 !border-2 !border-card !bg-destructive" style={{ top: WORKFLOW_NODE_SPLIT_OUTCOME_TOP.failure }} />
          </TooltipTrigger>
          <TooltipContent>{data.failureLabel}</TooltipContent>
        </Tooltip>
      ) : null}
      <div className="flex items-center gap-1.5">
        {data.iconKey ? (
          <span className="grid size-5 shrink-0 place-items-center rounded-md border border-border/60 bg-muted/30 shadow-sm">
            <img src={agentIconSrc(data.iconKey)} alt="" className={agentIconClass(data.iconKey, 'size-4')} />
          </span>
        ) : null}
        <span className="text-[13px] font-medium text-foreground">{data.label}</span>
      </div>
      <span className="truncate font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground">{data.kind}</span>
    </div>
  );
}

interface WorkflowEditorProps {
  className?: string;
  value: WorkflowDsl;
  agentRegistry: AgentRegistryVm | null;
  profiles?: ProfileVm[];
  onOpenProfileManagement?: () => void;
  onSave: (workflow: WorkflowDsl) => Promise<void> | void;
  onChange?: (workflow: WorkflowDsl) => void;
  onApplyDefaultTemplate?: (workflow: WorkflowDsl) => void;
  defaultWorkflow?: WorkflowDsl | null;
  workflowTemplates?: WorkflowTemplateStore | null;
  currentTemplateId?: string | null;
  currentTemplateName?: string | null;
  validateTemplateDuplicateId?: boolean;
  allowAiDynamic?: boolean;
  saving?: boolean;
  showSaveAction?: boolean;
  validationRequestId?: number;
  initialSessionDraft?: WorkflowEditorSessionDraft | null;
  onSessionDraftChange?: (draft: WorkflowEditorSessionDraft) => void;
}

export function WorkflowEditor({ className, value, agentRegistry, profiles = [], onOpenProfileManagement, onSave, onChange, onApplyDefaultTemplate, defaultWorkflow, workflowTemplates, currentTemplateId = null, currentTemplateName = null, validateTemplateDuplicateId = true, allowAiDynamic = false, saving, showSaveAction = true, validationRequestId = 0, initialSessionDraft, onSessionDraftChange }: WorkflowEditorProps) {
  const { t } = useTranslation();
  const initialWorkflow = useMemo(() => normalizeWorkflowEntryFromTopology(normalizeWorkflowSchemas(value)), [value]);
  const restoredWorkflow = initialSessionDraft?.workflow ?? initialWorkflow;
  const [workflow, setWorkflow] = useState<WorkflowDsl>(restoredWorkflow);
  const [tab, setTab] = useState<EditorTab>(initialSessionDraft?.tab ?? 'canvas');
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [selectedTerminalId, setSelectedTerminalId] = useState<string | null>(null);
  const [flowInstance, setFlowInstance] = useState<ReactFlowInstance<Node<EditorNodeData>, Edge> | null>(null);
  const [pendingFocusNodeId, setPendingFocusNodeId] = useState<string | null>(null);
  const [visibleTerminalIds, setVisibleTerminalIds] = useState<Set<string>>(new Set());
  const [compactPane, setCompactPane] = useState<'canvas' | 'inspector'>('canvas');
  const [isCompact, setIsCompact] = useState(false);
  const [viewportRevision, setViewportRevision] = useState(0);
  const [historyRevision, setHistoryRevision] = useState(0);
  const [terminalMenu, setTerminalMenu] = useState<TerminalMenu | null>(null);
  const [validationDialogOpen, setValidationDialogOpen] = useState(false);
  const [pendingValidation, setPendingValidation] = useState<WorkflowValidationResult | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string[]>>({});
  const [invalidNodeIds, setInvalidNodeIds] = useState<Set<string>>(new Set());
  const [jsonDraft, setJsonDraft] = useState(() => initialSessionDraft?.jsonDraft ?? JSON.stringify(restoredWorkflow, null, 2));
  const [jsonError, setJsonError] = useState<string | null>(null);
  const [liveValidation, setLiveValidation] = useState<WorkflowValidationResult | null>(null);
  const [newRoundEntryDrafts, setNewRoundEntryDrafts] = useState<Record<number, string>>(() => newRoundEntryDraftsFromWorkflow(restoredWorkflow));
  const handledValidationRequestIdRef = useRef(0);
  const restoredDraftAppliedRef = useRef(Boolean(initialSessionDraft));
  const editorContainerRef = useRef<HTMLDivElement | null>(null);
  const workflowRef = useRef(workflow);
  const onChangeRef = useRef(onChange);
  const onSessionDraftChangeRef = useRef(onSessionDraftChange);
  const externalChangeTimerRef = useRef<number | null>(null);
  const historyRef = useRef<WorkflowEditorHistory>({ past: [], future: [] });
  const viewportRef = useRef<Viewport>(initialSessionDraft?.viewport ?? { x: 0, y: 0, zoom: 1 });
  const hasStableViewportRef = useRef(Boolean(initialSessionDraft?.viewport));
  const initialFitFrameRef = useRef<number | null>(null);
  const canvasActionsRef = useRef<{
    quickAdd: (nodeId: string, outcome: EdgeOutcome) => void;
    deleteNode: (nodeId: string) => void;
  }>({ quickAdd: () => undefined, deleteNode: () => undefined });
  const clearCanvasSelection = useCallback(() => {
    setSelectedNodeId(null);
    setSelectedEdgeId(null);
    setSelectedTerminalId(null);
    setPendingFocusNodeId(null);
    const activeElement = document.activeElement;
    if (activeElement instanceof HTMLElement && editorContainerRef.current?.contains(activeElement)) activeElement.blur();
  }, []);
  workflowRef.current = workflow;
  onChangeRef.current = onChange;
  onSessionDraftChangeRef.current = onSessionDraftChange;
  const agents = useMemo(() => workflowEditorSupportedAgents(agentRegistry), [agentRegistry]);
  const agentIconKeys = useMemo(() => workflowAgentIconKeys(agents), [agents]);
  const doctorReadyAgents = useMemo(() => agents.filter(isWorkflowAgentDoctorReady), [agents]);
  const selectedNode = selectedNodeId ? workflow.nodes.find((node) => node.id === selectedNodeId) ?? null : null;
  const selectedEdgeIndex = selectedEdgeId ? Number(selectedEdgeId.split(':').at(-1)) : -1;
  const selectedEdge = selectedEdgeIndex >= 0 ? workflow.edges[selectedEdgeIndex] ?? null : null;
  const canSave = workflow.nodes.length > 0 && workflow.entry.trim() !== '' && doctorReadyAgents.length > 0;
  const workflowTopologySignature = useMemo(() => authoringWorkflowTopologySignature(workflow), [workflow]);
  const workflowGraphSignature = useMemo(() => authoringWorkflowGraphSignature(workflow), [workflow]);
  const invalidNodeSignature = useMemo(() => stringSetSignature(invalidNodeIds), [invalidNodeIds]);
  const visibleTerminalSignature = useMemo(() => stringSetSignature(visibleTerminalIds), [visibleTerminalIds]);
  const graphLayout = useMemo(
    () => createAuthoringGraphLayout(workflow, visibleTerminalIds),
    [visibleTerminalSignature, workflowTopologySignature],
  );
  const handleCanvasQuickAdd = useCallback((nodeId: string, outcome: EdgeOutcome) => canvasActionsRef.current.quickAdd(nodeId, outcome), []);
  const handleCanvasDelete = useCallback((nodeId: string) => canvasActionsRef.current.deleteNode(nodeId), []);
  const { nodes, edges } = useMemo(
    () => createAuthoringFlowProjection(workflow, graphLayout, selectedNodeId, selectedEdgeId, invalidNodeIds, agentIconKeys, t, handleCanvasQuickAdd, handleCanvasDelete, selectedTerminalId),
    [agentIconKeys, graphLayout, handleCanvasDelete, handleCanvasQuickAdd, invalidNodeSignature, selectedEdgeId, selectedNodeId, selectedTerminalId, t, workflowGraphSignature],
  );
  useEffect(() => {
    if (!onSessionDraftChangeRef.current) return undefined;
    const timer = window.setTimeout(() => {
      onSessionDraftChangeRef.current?.({ workflow, tab, jsonDraft, viewport: viewportRef.current });
    }, WORKFLOW_EDITOR_DRAFT_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [jsonDraft, tab, viewportRevision, workflow]);

  useEffect(() => {
    const container = editorContainerRef.current;
    if (!container) return undefined;
    const publishLayoutMode = (width: number) => {
      const nextCompact = width < WORKFLOW_EDITOR_COMPACT_WIDTH;
      setIsCompact((current) => current === nextCompact ? current : nextCompact);
    };
    publishLayoutMode(container.clientWidth);
    const observer = new ResizeObserver(([entry]) => {
      if (entry) publishLayoutMode(entry.contentRect.width);
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setLiveValidation(validateWorkflowForSave(workflow, profiles, doctorReadyAgents, t, workflowTemplates ?? null, currentTemplateId, currentTemplateName, validateTemplateDuplicateId));
    }, 320);
    return () => window.clearTimeout(timer);
  }, [currentTemplateId, currentTemplateName, doctorReadyAgents, profiles, t, validateTemplateDuplicateId, workflow, workflowTemplates]);

  useEffect(() => () => {
    if (externalChangeTimerRef.current) {
      window.clearTimeout(externalChangeTimerRef.current);
      onChangeRef.current?.(workflowRef.current);
    }
    if (initialFitFrameRef.current) window.cancelAnimationFrame(initialFitFrameRef.current);
  }, []);

  useEffect(() => {
    if (restoredDraftAppliedRef.current) {
      restoredDraftAppliedRef.current = false;
      return;
    }
    if (JSON.stringify(workflow) === JSON.stringify(initialWorkflow)) return;
    setWorkflow(initialWorkflow);
    setJsonDraft(JSON.stringify(initialWorkflow, null, 2));
    setJsonError(null);
    clearCanvasSelection();
    setVisibleTerminalIds(new Set());
    setTerminalMenu(null);
    setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(initialWorkflow));
    historyRef.current = { past: [], future: [] };
    setHistoryRevision((revision) => revision + 1);
  }, [clearCanvasSelection, initialWorkflow]);

  useEffect(() => {
    if (validationRequestId <= 0 || handledValidationRequestIdRef.current === validationRequestId) return;
    handledValidationRequestIdRef.current = validationRequestId;
    const validation = validateWorkflowForSave(workflow, profiles, doctorReadyAgents, t, workflowTemplates ?? null, currentTemplateId, currentTemplateName, validateTemplateDuplicateId);
    if (validation.valid) return;
    setPendingValidation(validation);
    setValidationDialogOpen(true);
  }, [doctorReadyAgents, profiles, t, validationRequestId, workflow, workflowTemplates]);

  useEffect(() => {
    if (!pendingFocusNodeId || !flowInstance) return;
    const node = nodes.find((item) => item.id === pendingFocusNodeId);
    if (!node) return;
    window.requestAnimationFrame(() => {
      const width = Number(node.style?.width ?? NODE_WIDTH);
      const height = Number(node.style?.height ?? NODE_HEIGHT);
      void flowInstance.setCenter(node.position.x + width / 2, node.position.y + height / 2, { zoom: 1.05, duration: 350 });
      setPendingFocusNodeId(null);
    });
  }, [flowInstance, nodes, pendingFocusNodeId]);

  const queueExternalChange = useCallback((next: WorkflowDsl) => {
    if (externalChangeTimerRef.current) window.clearTimeout(externalChangeTimerRef.current);
    externalChangeTimerRef.current = window.setTimeout(() => {
      externalChangeTimerRef.current = null;
      onChangeRef.current?.(next);
    }, WORKFLOW_EDITOR_DRAFT_DELAY_MS);
  }, []);

  const applyWorkflowWithoutHistory = useCallback((next: WorkflowDsl) => {
    const normalizedNext = normalizeWorkflowEntryFromTopology(next);
    setFieldErrors({});
    setInvalidNodeIds(new Set());
    setJsonError(null);
    setWorkflow(normalizedNext);
    queueExternalChange(normalizedNext);
    return normalizedNext;
  }, [queueExternalChange]);

  const syncWorkflow = useCallback((next: WorkflowDsl) => {
    const current = workflowRef.current;
    historyRef.current = recordWorkflowHistory(historyRef.current, current);
    setHistoryRevision((revision) => revision + 1);
    return applyWorkflowWithoutHistory(next);
  }, [applyWorkflowWithoutHistory]);

  const undoWorkflow = useCallback(() => {
    const result = undoWorkflowHistory(historyRef.current, workflowRef.current);
    if (!result) return;
    historyRef.current = result.history;
    applyWorkflowWithoutHistory(result.workflow);
    clearCanvasSelection();
    setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(result.workflow));
    setHistoryRevision((revision) => revision + 1);
  }, [applyWorkflowWithoutHistory, clearCanvasSelection]);

  const redoWorkflow = useCallback(() => {
    const result = redoWorkflowHistory(historyRef.current, workflowRef.current);
    if (!result) return;
    historyRef.current = result.history;
    applyWorkflowWithoutHistory(result.workflow);
    clearCanvasSelection();
    setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(result.workflow));
    setHistoryRevision((revision) => revision + 1);
  }, [applyWorkflowWithoutHistory, clearCanvasSelection]);

  const closeValidationDialog = (open: boolean) => {
    setValidationDialogOpen(open);
    if (open || !pendingValidation) return;
    setFieldErrors(pendingValidation.fieldErrors);
    setInvalidNodeIds(new Set(pendingValidation.issues.flatMap((issue) => issue.nodeIds ?? (issue.nodeId ? [issue.nodeId] : []))));
    syncWorkflow(pendingValidation.sanitizedWorkflow);
    setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(pendingValidation.sanitizedWorkflow));
    const firstIssue = pendingValidation.issues.find((issue) => issue.nodeId || issue.nodeIds?.length || issue.edgeIndex !== undefined);
    const firstIssueNodeId = firstIssue?.nodeId ?? firstIssue?.nodeIds?.[0];
    if (firstIssueNodeId) {
      setSelectedNodeId(firstIssueNodeId);
      setSelectedEdgeId(null);
      setSelectedTerminalId(null);
      setPendingFocusNodeId(firstIssueNodeId);
    } else if (firstIssue?.edgeIndex !== undefined) {
      const edge = pendingValidation.sanitizedWorkflow.edges[firstIssue.edgeIndex];
      if (edge) {
        setSelectedNodeId(null);
        setSelectedEdgeId(edgeId(edge, firstIssue.edgeIndex));
        setSelectedTerminalId(null);
      }
    }
    setPendingValidation(null);
  };

  const handleConnect = (connection: Connection) => {
    if (!connection.source || !connection.target) return;
    if (connection.source === END_NODE || connection.source === NEW_ROUND_NODE) return;
    const sourceNode = workflow.nodes.find((node) => node.id === connection.source);
    if (connection.sourceHandle === 'failure' && !nodeSupportsFailureOutcome(sourceNode)) return;
    const edge: WorkflowEdgeDsl = {
      from: connection.source,
      to: connection.target,
      on: connection.target === NEW_ROUND_NODE || connection.sourceHandle === 'failure' ? 'failure' : 'success',
    };
    const next = { ...workflow, edges: [...workflow.edges, edge] };
    syncWorkflow(next);
    setSelectedEdgeId(edgeId(edge, next.edges.length - 1));
    setSelectedNodeId(null);
    setSelectedTerminalId(null);
    setTerminalMenu(null);
  };

  const showTerminalTarget = (terminalId: string) => {
    setVisibleTerminalIds((current) => new Set(current).add(terminalId));
    setSelectedTerminalId(terminalId);
    setSelectedNodeId(null);
    setSelectedEdgeId(null);
    setTerminalMenu(null);
  };

  const applyDefaultTemplate = () => {
    if (!defaultWorkflow) return;
    const next = normalizeWorkflowSchemas(cloneWorkflow(defaultWorkflow));
    syncWorkflow(next);
    setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(next));
    onApplyDefaultTemplate?.(next);
    clearCanvasSelection();
  };

  const handleSave = async () => {
    let workflowToSave = workflow;
    if (tab === 'json') {
      const parsed = parseWorkflowJson(jsonDraft);
      if (!parsed) {
        setJsonError(t('workflowEditor.outputSchemaInvalid'));
        return;
      }
      workflowToSave = normalizeWorkflowEntryFromTopology(normalizeWorkflowSchemas(parsed));
      setWorkflow(workflowToSave);
      setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(workflowToSave));
      queueExternalChange(workflowToSave);
    }
    const validation = validateWorkflowForSave(workflowToSave, profiles, doctorReadyAgents, t, workflowTemplates ?? null, currentTemplateId, currentTemplateName, validateTemplateDuplicateId);
    if (!validation.valid) {
      setPendingValidation(validation);
      setValidationDialogOpen(true);
      return;
    }
    setFieldErrors({});
    setInvalidNodeIds(new Set());
    try {
      await onSave(validation.sanitizedWorkflow);
      setWorkflow(validation.sanitizedWorkflow);
      setJsonDraft(JSON.stringify(validation.sanitizedWorkflow, null, 2));
    } catch (error) {
      setPendingValidation({
        valid: false,
        issues: [{ message: displayAppError(t, error) }],
        fieldErrors: {},
        sanitizedWorkflow: validation.sanitizedWorkflow,
      });
      setValidationDialogOpen(true);
    }
  };

  const addWorkerNode = () => {
    const nextIndex = workflow.nodes.length + 1;
    const id = uniqueNodeId(workflow, `node-${nextIndex}`);
    const node: WorkflowWorkerNodeDsl = {
      type: 'worker',
      id,
      provider: null,
      goal: null,
    };
    const next = { ...workflow, entry: workflow.entry || id, nodes: [...workflow.nodes, node] };
    syncWorkflow(next);
    setSelectedNodeId(id);
    setSelectedEdgeId(null);
    setPendingFocusNodeId(id);
  };

  const addAiDynamicNode = () => {
    const id = uniqueNodeId(workflow, 'ai-dynamic');
    const node: WorkflowAiDynamicNodeDsl = {
      type: 'ai-dynamic',
      id,
      agentStrategy: {
        mode: 'fixed',
        provider: '',
      },
      control: defaultDynamicControl(),
      allowedWorkflows: [],
    };
    const next = { ...workflow, entry: workflow.entry || id, nodes: [...workflow.nodes, node] };
    syncWorkflow(next);
    setSelectedNodeId(id);
    setSelectedEdgeId(null);
    setSelectedTerminalId(null);
    setPendingFocusNodeId(id);
  };

  const deleteNodeById = useCallback((nodeId: string) => {
    const currentWorkflow = workflowRef.current;
    const nodes = currentWorkflow.nodes.filter((node) => node.id !== nodeId);
    const next = {
      ...currentWorkflow,
      entry: currentWorkflow.entry === nodeId ? nodes[0]?.id ?? '' : currentWorkflow.entry,
      nodes,
      edges: currentWorkflow.edges
        .filter((edge) => edge.from !== nodeId && edge.to !== nodeId)
        .map((edge) => {
          if (edge.new_round_entry !== nodeId) return edge;
          const updated = { ...edge };
          delete updated.new_round_entry;
          return updated;
        }),
    };
    syncWorkflow(next);
    clearCanvasSelection();
  }, [clearCanvasSelection, syncWorkflow]);

  const deleteTerminalById = useCallback((terminalId: string) => {
    const currentWorkflow = workflowRef.current;
    const next = removeTerminalFromWorkflow(currentWorkflow, terminalId);
    if (next.edges.length !== currentWorkflow.edges.length) syncWorkflow(next);
    setVisibleTerminalIds((current) => {
      const updated = new Set(current);
      updated.delete(terminalId);
      return updated;
    });
    setSelectedTerminalId(null);
    setSelectedNodeId(null);
    setSelectedEdgeId(null);
  }, [syncWorkflow]);

  const quickAddSuccessor = useCallback((sourceId: string, outcome: EdgeOutcome) => {
    const currentWorkflow = workflowRef.current;
    const sourceNode = currentWorkflow.nodes.find((node) => node.id === sourceId);
    if (!sourceNode || (outcome === 'failure' && !nodeSupportsFailureOutcome(sourceNode))) return;
    const id = uniqueNodeId(currentWorkflow, `node-${currentWorkflow.nodes.length + 1}`);
    const node: WorkflowWorkerNodeDsl = { type: 'worker', id, provider: null, goal: null };
    const edge: WorkflowEdgeDsl = { from: sourceId, to: id, on: outcome };
    const next = { ...currentWorkflow, nodes: [...currentWorkflow.nodes, node], edges: [...currentWorkflow.edges, edge] };
    syncWorkflow(next);
    setSelectedNodeId(id);
    setSelectedEdgeId(null);
    setSelectedTerminalId(null);
    setPendingFocusNodeId(id);
  }, [syncWorkflow]);

  const updateNode = (nodeId: string, patch: Partial<WorkflowNodeDsl>) => {
    const nextId = patch.id && patch.id !== nodeId ? sanitizeNodeId(patch.id, workflow, nodeId) : null;
    const nodes = workflow.nodes.map((node) => node.id === nodeId ? { ...node, ...patch, id: nextId ?? node.id } as WorkflowNodeDsl : node);
    const updatedNode = nodes.find((node) => node.id === (nextId ?? nodeId));
    const renamedEdges = nextId ? workflow.edges.map((edge) => ({
      ...edge,
      from: edge.from === nodeId ? nextId : edge.from,
      to: edge.to === nodeId ? nextId : edge.to,
      new_round_entry: edge.new_round_entry === nodeId ? nextId : edge.new_round_entry,
    })) : workflow.edges;
    const edges = nodeSupportsFailureOutcome(updatedNode)
      ? renamedEdges
      : renamedEdges.filter((edge) => edge.from !== (nextId ?? nodeId) || edge.on !== 'failure');
    const next = {
      ...workflow,
      entry: nextId && workflow.entry === nodeId ? nextId : workflow.entry,
      nodes,
      edges,
    };
    syncWorkflow(next);
    if (edges.length !== workflow.edges.length) setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(next));
    if (nextId) setSelectedNodeId(nextId);
  };

  const updateEdge = (index: number, patch: Partial<WorkflowEdgeDsl>) => {
    const currentEdge = workflow.edges[index];
    if (!currentEdge) return;
    if (patch.on === 'failure' && !nodeSupportsFailureOutcome(workflow.nodes.find((node) => node.id === currentEdge.from))) return;
    const updatedEdge = { ...currentEdge, ...patch };
    const draftValue = (patch.new_round_entry ?? currentEdge.new_round_entry)?.trim();
    if (draftValue) {
      setNewRoundEntryDrafts((current) => ({ ...current, [index]: draftValue }));
    }
    if (patch.to === NEW_ROUND_NODE && !updatedEdge.new_round_entry?.trim()) {
      const restored = newRoundEntryDrafts[index];
      if (restored) updatedEdge.new_round_entry = restored;
    }
    if (updatedEdge.on === 'success' && updatedEdge.to === NEW_ROUND_NODE) updatedEdge.to = END_NODE;
    if (updatedEdge.to !== NEW_ROUND_NODE) delete updatedEdge.new_round_entry;
    const next = {
      ...workflow,
      edges: workflow.edges.map((edge, edgeIndex) => edgeIndex === index ? updatedEdge : edge),
    };
    syncWorkflow(next);
    setSelectedEdgeId(next.edges[index] ? edgeId(next.edges[index], index) : null);
  };

  const updateWorkflowControl = (patch: Partial<WorkflowControlDsl>) => {
    const control: WorkflowControlDsl = { ...(workflow.control ?? {}), ...patch };
    if (control.max_attempts == null) delete control.max_attempts;
    if (control.max_rounds == null) delete control.max_rounds;
    syncWorkflow({ ...workflow, control });
  };

  const deleteSelectedEdge = useCallback(() => {
    if (selectedEdgeIndex < 0) return;
    const currentWorkflow = workflowRef.current;
    const next = { ...currentWorkflow, edges: currentWorkflow.edges.filter((_, index) => index !== selectedEdgeIndex) };
    syncWorkflow(next);
    setNewRoundEntryDrafts((current) => shiftNewRoundEntryDraftsAfterDelete(current, selectedEdgeIndex));
    setSelectedEdgeId(null);
  }, [selectedEdgeIndex, syncWorkflow]);

  const deleteSelectedCanvasElement = useCallback(() => {
    if (selectedNodeId) deleteNodeById(selectedNodeId);
    else if (selectedTerminalId) deleteTerminalById(selectedTerminalId);
    else if (selectedEdgeIndex >= 0) deleteSelectedEdge();
  }, [deleteNodeById, deleteSelectedEdge, deleteTerminalById, selectedEdgeIndex, selectedNodeId, selectedTerminalId]);

  canvasActionsRef.current = { quickAdd: quickAddSuccessor, deleteNode: deleteNodeById };

  const focusValidationIssue = useCallback((issue: WorkflowValidationIssue, validation: WorkflowValidationResult) => {
    setFieldErrors(validation.fieldErrors);
    setInvalidNodeIds(new Set(validation.issues.flatMap((item) => item.nodeIds ?? (item.nodeId ? [item.nodeId] : []))));
    const nodeId = issue.nodeId ?? issue.nodeIds?.[0];
    if (nodeId) {
      setSelectedNodeId(nodeId);
      setSelectedEdgeId(null);
      setSelectedTerminalId(null);
      setPendingFocusNodeId(nodeId);
    } else if (issue.edgeIndex !== undefined) {
      const edge = workflowRef.current.edges[issue.edgeIndex];
      if (edge) {
        setSelectedNodeId(null);
        setSelectedEdgeId(edgeId(edge, issue.edgeIndex));
        setSelectedTerminalId(null);
      }
    }
    setCompactPane('inspector');
    setTab('canvas');
  }, []);

  const handleEditorTabChange = useCallback((nextTab: EditorTab) => {
    if (nextTab === tab) return;
    if (nextTab === 'json') {
      setJsonDraft(JSON.stringify(workflowRef.current, null, 2));
      setJsonError(null);
      setTab('json');
      return;
    }
    const parsed = parseWorkflowJson(jsonDraft);
    if (!parsed) {
      setJsonError(t('workflowEditor.outputSchemaInvalid'));
      return;
    }
    syncWorkflow(normalizeWorkflowSchemas(parsed));
    setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(parsed));
    setTab('canvas');
  }, [jsonDraft, syncWorkflow, t, tab]);

  const handleFlowInit = useCallback((instance: ReactFlowInstance<Node<EditorNodeData>, Edge>) => {
    setFlowInstance(instance);
    if (hasStableViewportRef.current) {
      void instance.setViewport(viewportRef.current);
      return;
    }
    initialFitFrameRef.current = window.requestAnimationFrame(() => {
      initialFitFrameRef.current = window.requestAnimationFrame(() => {
        void instance.fitView({ padding: 0.22, maxZoom: WORKFLOW_EDITOR_FIT_MAX_ZOOM, duration: 0 }).then(() => {
          viewportRef.current = instance.getViewport();
          hasStableViewportRef.current = true;
          setViewportRevision((revision) => revision + 1);
        });
      });
    });
  }, []);

  const handleMoveEnd = useCallback((_: MouseEvent | TouchEvent | null, viewport: Viewport) => {
    viewportRef.current = viewport;
    hasStableViewportRef.current = true;
    setViewportRevision((revision) => revision + 1);
  }, []);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target?.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target?.tagName ?? '')) return;
      const mod = event.ctrlKey || event.metaKey;
      if (mod && event.key.toLowerCase() === 'z') {
        event.preventDefault();
        if (event.shiftKey) redoWorkflow();
        else undoWorkflow();
        return;
      }
      if (mod && event.key.toLowerCase() === 'y') {
        event.preventDefault();
        redoWorkflow();
        return;
      }
      if (event.key !== 'Delete' && event.key !== 'Backspace') return;
      if (!selectedNodeId && !selectedTerminalId && selectedEdgeIndex < 0) return;
      event.preventDefault();
      if (selectedNodeId) deleteNodeById(selectedNodeId);
      else if (selectedTerminalId) deleteTerminalById(selectedTerminalId);
      else deleteSelectedEdge();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [deleteNodeById, deleteSelectedEdge, deleteTerminalById, redoWorkflow, selectedEdgeIndex, selectedNodeId, selectedTerminalId, undoWorkflow]);

  const canUndo = historyRevision >= 0 && historyRef.current.past.length > 0;
  const canRedo = historyRevision >= 0 && historyRef.current.future.length > 0;
  const validationIssues = liveValidation?.issues ?? [];

  const canvasSurface = (
    <AppCard className="flex size-full min-h-0 flex-col gap-0 overflow-hidden border-0 bg-transparent py-0 shadow-none">
      <CardHeader className="flex shrink-0 flex-col items-stretch justify-between gap-3 border-b px-4 py-3 @lg/workflow-editor:flex-row @lg/workflow-editor:items-center">
        <div className="min-w-0">
          <CardTitle>{t('workflowEditor.title')}</CardTitle>
          <p className="mt-1 text-xs text-muted-foreground">{t('workflowEditor.subtitle')}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Tabs value={tab} onValueChange={(value) => handleEditorTabChange(value as EditorTab)}>
            <TabsList>
              <TabsTrigger value="canvas">{t('workflowEditor.canvas')}</TabsTrigger>
              <TabsTrigger value="json">JSON</TabsTrigger>
            </TabsList>
          </Tabs>
          <div className="flex items-center rounded-lg border bg-background/70 p-0.5">
            <Tooltip><TooltipTrigger asChild><Button type="button" variant="ghost" size="icon-sm" className="size-7" disabled={!canUndo} onClick={undoWorkflow} aria-label={t('workflowEditor.undo')}><Undo2 className="size-3.5" /></Button></TooltipTrigger><TooltipContent>{t('workflowEditor.undo')} (Ctrl+Z)</TooltipContent></Tooltip>
            <Tooltip><TooltipTrigger asChild><Button type="button" variant="ghost" size="icon-sm" className="size-7" disabled={!canRedo} onClick={redoWorkflow} aria-label={t('workflowEditor.redo')}><Redo2 className="size-3.5" /></Button></TooltipTrigger><TooltipContent>{t('workflowEditor.redo')} (Ctrl+Y)</TooltipContent></Tooltip>
          </div>
          {defaultWorkflow ? <Button variant="outline" size="sm" onClick={applyDefaultTemplate}>{t('workflowEditor.defaultTemplate')}</Button> : null}
          {showSaveAction ? <Button size="sm" disabled={!canSave || saving} onClick={() => void handleSave()}>{t('workflowEditor.saveWorkflow')}</Button> : null}
        </div>
      </CardHeader>
      <CardContent className="min-h-0 flex-1 p-0">
        {tab === 'canvas' ? (
          <div className="relative size-full min-h-0">
            {terminalMenu ? (
              <div className="absolute z-30 w-44 overflow-hidden rounded-xl border bg-popover p-1 text-sm text-popover-foreground shadow-lg" style={{ left: terminalMenu.x, top: terminalMenu.y }}>
                <button type="button" className="flex min-h-9 w-full items-center rounded-md px-3 py-2 text-left hover:bg-accent hover:text-accent-foreground" onClick={() => showTerminalTarget(END_NODE)}>{t('workflowEditor.addEndTarget')}</button>
                <button type="button" className="flex min-h-9 w-full items-center rounded-md px-3 py-2 text-left hover:bg-accent hover:text-accent-foreground" onClick={() => showTerminalTarget(NEW_ROUND_NODE)}>{t('workflowEditor.addNewRoundTarget')}</button>
              </div>
            ) : null}
            <ReactFlow
              nodes={nodes}
              edges={edges}
              onConnect={handleConnect}
              onPaneClick={() => { setTerminalMenu(null); setSelectedNodeId(null); setSelectedEdgeId(null); setSelectedTerminalId(null); }}
              onPaneContextMenu={(event) => {
                event.preventDefault();
                const target = event.currentTarget as Element | null;
                if (!target) return;
                const bounds = target.getBoundingClientRect();
                setTerminalMenu({ x: event.clientX - bounds.left, y: event.clientY - bounds.top });
              }}
              onInit={handleFlowInit}
              onMoveEnd={handleMoveEnd}
              onNodeClick={(_, node) => {
                setSelectedNodeId(node.data.terminal ? null : node.id);
                setSelectedTerminalId(node.data.terminal ? node.id : null);
                setSelectedEdgeId(null);
              }}
              onEdgeClick={(_, edge) => { setSelectedEdgeId(edge.id); setSelectedNodeId(null); setSelectedTerminalId(null); }}
              nodesDraggable={false}
              nodesConnectable
              connectOnClick
              connectionRadius={32}
              elementsSelectable
              nodesFocusable
              edgesFocusable
              deleteKeyCode={null}
              defaultViewport={viewportRef.current}
              minZoom={WORKFLOW_EDITOR_MIN_ZOOM}
              maxZoom={WORKFLOW_EDITOR_MAX_ZOOM}
              proOptions={{ hideAttribution: true }}
              edgeTypes={edgeTypes}
              nodeTypes={editorNodeTypes}
              className="workflow-graph bg-muted/10"
            >
              <Background color="var(--border)" gap={28} size={1} />
              <Panel position="top-left" className="m-3 flex max-w-[calc(100%-1.5rem)] flex-wrap items-center gap-1 rounded-xl border border-border/70 bg-background/85 p-1 shadow-sm backdrop-blur-md">
                <DropdownMenu>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <DropdownMenuTrigger asChild>
                        <Button type="button" size="icon-sm" variant="ghost" className="size-8 rounded-full" aria-label={t('workflowEditor.addNode')}><Plus className="size-4" /></Button>
                      </DropdownMenuTrigger>
                    </TooltipTrigger>
                    <TooltipContent>{t('workflowEditor.addNode')}</TooltipContent>
                  </Tooltip>
                  <DropdownMenuContent align="start" sideOffset={8}>
                    <DropdownMenuItem onClick={addWorkerNode}><Plus className="size-4" />{t('workflowEditor.addWorkerNode')}</DropdownMenuItem>
                    {allowAiDynamic ? <DropdownMenuItem onClick={addAiDynamicNode}><Sparkles className="size-4" />{t('workflowEditor.addAiDynamicNode')}</DropdownMenuItem> : null}
                    <DropdownMenuItem onClick={() => showTerminalTarget(END_NODE)}><Check className="size-4" />{t('workflowEditor.endTarget')}</DropdownMenuItem>
                    <DropdownMenuItem onClick={() => showTerminalTarget(NEW_ROUND_NODE)}><Redo2 className="size-4" />{t('workflowEditor.newRoundTarget')}</DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
                <Button size="sm" variant="ghost" className="h-8 rounded-full px-2.5 text-xs font-medium text-muted-foreground hover:bg-destructive/10 hover:text-destructive disabled:hover:bg-transparent" disabled={!selectedNodeId && !selectedTerminalId && selectedEdgeIndex < 0} onClick={deleteSelectedCanvasElement}><Trash2 className="size-3.5" />{t(selectedEdgeIndex >= 0 ? 'workflowEditor.deleteEdge' : 'workflowEditor.deleteNode')}</Button>
              </Panel>
              <GraphControls
                disabled={!flowInstance}
                onZoomIn={() => { void flowInstance?.zoomIn(); }}
                onZoomOut={() => { void flowInstance?.zoomOut(); }}
                onFitView={() => { void flowInstance?.fitView({ padding: 0.22, maxZoom: WORKFLOW_EDITOR_FIT_MAX_ZOOM }); }}
              />
            </ReactFlow>
          </div>
        ) : (
          <div className="flex size-full min-h-0 flex-col p-4">
            <Textarea
              value={jsonDraft}
              onChange={(event) => {
                const nextDraft = event.target.value;
                setJsonDraft(nextDraft);
                setJsonError(null);
                const parsed = parseWorkflowJson(nextDraft);
                if (!parsed) return;
                const nextWorkflow = normalizeWorkflowSchemas(parsed);
                setWorkflow(nextWorkflow);
                setNewRoundEntryDrafts(newRoundEntryDraftsFromWorkflow(nextWorkflow));
                queueExternalChange(nextWorkflow);
              }}
              className="min-h-0 flex-1 resize-none font-mono text-xs"
              spellCheck={false}
            />
            {jsonError ? <p className="mt-2 text-xs text-destructive">{jsonError}</p> : null}
          </div>
        )}
      </CardContent>
    </AppCard>
  );

  const inspectorSurface = (
    <AppCard className="flex size-full min-h-0 flex-col gap-0 overflow-hidden border-0 bg-transparent py-0 shadow-none">
      <CardHeader className="shrink-0 border-b px-4 py-3"><CardTitle>{t('workflowEditor.inspector')}</CardTitle></CardHeader>
      <CardContent className="min-h-0 flex-1 p-0">
        <ScrollArea className="size-full">
          <div className="space-y-4 p-4">
            {validationIssues.length > 0 && liveValidation ? (
              <Alert variant="destructive" className="bg-destructive/5">
                <AlertCircle />
                <AlertTitle>{t('workflowEditor.validationSummary', { count: validationIssues.length })}</AlertTitle>
                <AlertDescription>
                  {validationIssues.slice(0, 6).map((issue, index) => (
                    <button key={`${issue.message}:${index}`} type="button" className="w-full rounded-md px-1 py-1 text-left text-xs leading-5 underline-offset-2 hover:bg-destructive/10 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" onClick={() => focusValidationIssue(issue, liveValidation)}>{issue.message}</button>
                  ))}
                  {validationIssues.length > 6 ? <p className="px-1 text-xs">{t('workflowEditor.moreValidationIssues', { count: validationIssues.length - 6 })}</p> : null}
                </AlertDescription>
              </Alert>
            ) : null}
            <section className="space-y-3" aria-labelledby="workflow-current-selection-title">
              <div className="space-y-1"><h3 id="workflow-current-selection-title" className="text-sm font-semibold">{t('workflowEditor.currentSelection')}</h3><p className="text-xs text-muted-foreground">{t('workflowEditor.currentSelectionHelp')}</p></div>
              {!agents.length ? <EmptyState>{t('workflowEditor.noAgents')}</EmptyState> : null}
              {selectedNode ? <BufferedNodeInspector key={selectedNode.id} node={selectedNode} agents={agents} profiles={profiles} workflow={workflow} workflowTemplates={workflowTemplates ?? null} fieldErrors={fieldErrors} onUpdate={updateNode} onOpenProfileManagement={onOpenProfileManagement} t={t} /> : null}
              {selectedEdge ? <EdgeInspector edge={selectedEdge} index={selectedEdgeIndex} workflow={workflow} fieldErrors={fieldErrors} onUpdate={updateEdge} t={t} /> : null}
              {selectedTerminalId ? <EmptyState>{t('workflowEditor.terminalSelectionHint')}</EmptyState> : null}
              {!selectedNode && !selectedEdge && !selectedTerminalId ? <EmptyState>{t('workflowEditor.selectHint')}</EmptyState> : null}
            </section>
            <Collapsible defaultOpen className="overflow-hidden rounded-lg bg-muted/20">
              <CollapsibleTrigger className="flex w-full items-center justify-between gap-3 px-3 py-3 text-left text-sm font-semibold hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset"><span>{t('workflowEditor.workflowSettings')}</span><ChevronDown className="size-4 transition-transform [[data-state=open]>&]:rotate-180" /></CollapsibleTrigger>
              <CollapsibleContent className="border-t border-border/50 p-3"><BufferedWorkflowControlInspector control={workflow.control} fieldErrors={fieldErrors} onUpdate={updateWorkflowControl} t={t} /></CollapsibleContent>
            </Collapsible>
          </div>
        </ScrollArea>
      </CardContent>
    </AppCard>
  );

  return (
    <>
      <Dialog open={validationDialogOpen} onOpenChange={closeValidationDialog}>
        <DialogContent>
          <DialogHeader><DialogTitle>{t('workflowEditor.validationDialogTitle')}</DialogTitle><DialogDescription>{t('workflowEditor.validationDialogDescription')}</DialogDescription></DialogHeader>
          <div className="max-h-80 space-y-2 overflow-auto rounded-lg border bg-muted/20 p-3 text-sm">{pendingValidation?.issues.map((issue, index) => <div key={`${issue.message}:${index}`} className="rounded-md bg-background/70 px-3 py-2 text-destructive">{issue.message}</div>)}</div>
          <DialogFooter><Button onClick={() => closeValidationDialog(false)}>{t('workflowEditor.validationDialogClose')}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
      <div ref={editorContainerRef} className={cn('@container/workflow-editor h-[clamp(520px,calc(100dvh-11rem),760px)] min-h-0', className)} data-workflow-editor-layout={isCompact ? 'compact' : 'split'}>
        {isCompact ? (
          <div className="flex size-full min-h-0 flex-col gap-2">
            <Tabs value={compactPane} onValueChange={(value) => setCompactPane(value as 'canvas' | 'inspector')} className="shrink-0">
              <TabsList className="w-full"><TabsTrigger value="canvas">{t('workflowEditor.canvas')}</TabsTrigger><TabsTrigger value="inspector">{t('workflowEditor.inspector')}</TabsTrigger></TabsList>
            </Tabs>
            <div className="min-h-0 flex-1">{compactPane === 'canvas' ? canvasSurface : inspectorSurface}</div>
          </div>
        ) : (
          <ResizablePanelGroup orientation="horizontal" className="size-full gap-0">
            <ResizablePanel id="workflow-canvas" defaultSize="68%" minSize={480} className="min-w-0">{canvasSurface}</ResizablePanel>
            <ResizableHandle withHandle className="mx-1 bg-border/60" />
            <ResizablePanel id="workflow-inspector" defaultSize="32%" minSize={300} maxSize={460} className="min-w-0">{inspectorSurface}</ResizablePanel>
          </ResizablePanelGroup>
        )}
      </div>
    </>
  );
}

function BufferedWorkflowControlInspector({ control, onUpdate, ...props }: { control: WorkflowControlDsl; fieldErrors: Record<string, string[]>; onUpdate: (patch: Partial<WorkflowControlDsl>) => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const [draft, setDraft] = useState(control);
  const pendingPatchRef = useRef<Partial<WorkflowControlDsl>>({});
  const timerRef = useRef<number | null>(null);
  const onUpdateRef = useRef(onUpdate);
  onUpdateRef.current = onUpdate;

  useEffect(() => {
    if (Object.keys(pendingPatchRef.current).length === 0) setDraft(control);
  }, [control]);

  useEffect(() => () => {
    if (timerRef.current) window.clearTimeout(timerRef.current);
    if (Object.keys(pendingPatchRef.current).length > 0) onUpdateRef.current(pendingPatchRef.current);
  }, []);

  const updateDraft = (patch: Partial<WorkflowControlDsl>) => {
    pendingPatchRef.current = { ...pendingPatchRef.current, ...patch };
    setDraft((current) => ({ ...current, ...patch }));
    if (timerRef.current) window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => {
      const pending = pendingPatchRef.current;
      pendingPatchRef.current = {};
      timerRef.current = null;
      onUpdateRef.current(pending);
    }, WORKFLOW_EDITOR_DRAFT_DELAY_MS);
  };

  return <WorkflowControlInspector {...props} control={draft} onUpdate={updateDraft} />;
}

function WorkflowControlInspector({ control, fieldErrors, onUpdate, t }: { control: WorkflowControlDsl; fieldErrors: Record<string, string[]>; onUpdate: (patch: Partial<WorkflowControlDsl>) => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const errorsFor = (field: string) => fieldErrors[`control:${field}`] ?? [];
  const parseLimit = (value: string) => {
    if (!value.trim()) return null;
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.trunc(parsed) : 0;
  };
  return (
    <div className="space-y-3">
      <div className="space-y-1">
        <strong className="text-sm">{t('workflowEditor.workflowControls')}</strong>
        <p className="text-xs leading-5 text-muted-foreground">{t('workflowEditor.workflowControlsHelp')}</p>
      </div>
      <Field label={<HelpLabel label={t('workflowEditor.maxAttempts')} help={t('workflowEditor.maxAttemptsHelp')} />} errors={errorsFor('max_attempts')}>
        <Input
          className={errorClass(errorsFor('max_attempts'))}
          type="number"
          min={1}
          step={1}
          value={control.max_attempts ?? ''}
          placeholder={t('workflow.unlimited')}
          onChange={(event) => onUpdate({ max_attempts: parseLimit(event.target.value) })}
        />
      </Field>
      <Field label={<HelpLabel label={t('workflowEditor.maxRounds')} help={t('workflowEditor.maxRoundsHelp')} />} errors={errorsFor('max_rounds')}>
        <Input
          className={errorClass(errorsFor('max_rounds'))}
          type="number"
          min={1}
          step={1}
          value={control.max_rounds ?? ''}
          placeholder={t('workflow.unlimited')}
          onChange={(event) => onUpdate({ max_rounds: parseLimit(event.target.value) })}
        />
      </Field>
    </div>
  );
}

function BufferedNodeInspector(props: { node: WorkflowNodeDsl; agents: ManagedAgentVm[]; profiles: ProfileVm[]; workflow: WorkflowDsl; workflowTemplates: WorkflowTemplateStore | null; fieldErrors: Record<string, string[]>; onUpdate: (nodeId: string, patch: Partial<WorkflowNodeDsl>) => void; onOpenProfileManagement?: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const { node, onUpdate } = props;
  const [draft, setDraft] = useState(node);
  const pendingPatchRef = useRef<Partial<WorkflowNodeDsl>>({});
  const timerRef = useRef<number | null>(null);
  const nodeIdRef = useRef(node.id);
  const onUpdateRef = useRef(onUpdate);
  nodeIdRef.current = node.id;
  onUpdateRef.current = onUpdate;

  useEffect(() => {
    if (Object.keys(pendingPatchRef.current).length === 0) setDraft(node);
  }, [node]);

  useEffect(() => () => {
    if (timerRef.current) window.clearTimeout(timerRef.current);
    if (Object.keys(pendingPatchRef.current).length > 0) onUpdateRef.current(nodeIdRef.current, pendingPatchRef.current);
  }, []);

  const flush = (immediatePatch: Partial<WorkflowNodeDsl> = {}) => {
    if (timerRef.current) window.clearTimeout(timerRef.current);
    const pending = pendingPatchRef.current;
    pendingPatchRef.current = {};
    timerRef.current = null;
    const merged = mergeBufferedNodePatches(pending, immediatePatch);
    if (Object.keys(merged).length > 0) onUpdateRef.current(nodeIdRef.current, merged);
  };

  const updateDraft = (_nodeId: string, patch: Partial<WorkflowNodeDsl>) => {
    if (patch.id !== undefined) {
      flush(patch);
      return;
    }
    pendingPatchRef.current = { ...pendingPatchRef.current, ...patch };
    setDraft((current) => ({ ...current, ...patch } as WorkflowNodeDsl));
    if (timerRef.current) window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(flush, WORKFLOW_EDITOR_DRAFT_DELAY_MS);
  };

  return <WorkflowNodeInspector {...props} node={draft} onUpdate={updateDraft} />;
}

export function WorkflowNodeInspector(props: { node: WorkflowNodeDsl; agents: ManagedAgentVm[]; profiles: ProfileVm[]; workflow: WorkflowDsl; workflowTemplates: WorkflowTemplateStore | null; fieldErrors: Record<string, string[]>; onUpdate: (nodeId: string, patch: Partial<WorkflowNodeDsl>) => void; onOpenProfileManagement?: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  if (props.node.type === 'ai-dynamic') {
    return <AiDynamicNodeInspector {...props} node={props.node} />;
  }
  return <WorkerNodeInspector {...props} node={props.node} />;
}

function WorkerNodeInspector({ node, agents, profiles, fieldErrors, onUpdate, onOpenProfileManagement, t }: { node: WorkflowWorkerNodeDsl; agents: ManagedAgentVm[]; profiles: ProfileVm[]; workflow: WorkflowDsl; workflowTemplates: WorkflowTemplateStore | null; fieldErrors: Record<string, string[]>; onUpdate: (nodeId: string, patch: Partial<WorkflowNodeDsl>) => void; onOpenProfileManagement?: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const [nodeIdDraft, setNodeIdDraft] = useState(node.id);
  const [nodeIdComposing, setNodeIdComposing] = useState(false);
  const [schemaDraft, setSchemaDraft] = useState('');
  const [schemaError, setSchemaError] = useState<string | null>(null);
  const [schemaDirty, setSchemaDirty] = useState(false);
  const schemaSelfUpdateNodeId = useRef<string | null>(null);

  const validationEnabled = Boolean(node.output || node.success_condition);
  const manualCheckEnabled = Boolean(node.manual_check);
  const resultMode = validationEnabled ? 'ai' : manualCheckEnabled ? 'manual' : 'none';
  const expression = conditionExpression(node.success_condition);
  const selectedAgent = agents.find((agent) => agent.agentType === node.provider) ?? null;
  const updateWorker = (patch: Partial<WorkflowWorkerNodeDsl>) => onUpdate(node.id, patch as Partial<WorkflowNodeDsl>);
  const modelOptions = selectedAgent?.supportedModels ?? [];
  const thoughtLevel = findAcpThoughtLevel(selectedAgent?.configOptions);
  const permissionModes = selectedAgent?.supportedModes ?? [];
  const errorsFor = (field: string) => fieldErrors[`node:${node.id}:${field}`] ?? [];
  const clearValidationPatch = { output: null, success_condition: null };
  const updateOutput = useCallback((patch: Partial<WorkflowOutputContractDsl>) => {
    const artifact = patch.artifact ?? node.output?.artifact ?? `${node.id}-result`;
    updateWorker({
      manual_check: null,
      output: { kind: 'json', artifact, schema: node.output?.schema ?? null, ...patch },
    });
  }, [node.id, node.output?.artifact, node.output?.schema, onUpdate]);
  const commitSchemaDraft = useCallback((value: string) => {
    if (!value.trim()) {
      schemaSelfUpdateNodeId.current = node.id;
      updateOutput({ schema: null });
      setSchemaError(null);
      return true;
    }
    try {
      schemaSelfUpdateNodeId.current = node.id;
      updateOutput({ schema: JSON.parse(value) });
      setSchemaError(null);
      return true;
    } catch {
      setSchemaError(t('workflowEditor.outputSchemaInvalid'));
      return false;
    }
  }, [node.id, t, updateOutput]);
  const beautifySchemaDraft = () => {
    if (!schemaDraft.trim()) {
      setSchemaDirty(false);
      commitSchemaDraft(schemaDraft);
      return;
    }
    try {
      const parsed = JSON.parse(schemaDraft);
      const formatted = JSON.stringify(parsed, null, 2);
      setSchemaDraft(formatted);
      setSchemaDirty(false);
      schemaSelfUpdateNodeId.current = node.id;
      updateOutput({ schema: parsed });
      setSchemaError(null);
    } catch {
      setSchemaError(t('workflowEditor.outputSchemaInvalid'));
    }
  };

  useEffect(() => {
    setNodeIdDraft(node.id);
  }, [node.id]);

  useEffect(() => {
    if (schemaSelfUpdateNodeId.current === node.id) {
      schemaSelfUpdateNodeId.current = null;
      return;
    }
    schemaSelfUpdateNodeId.current = null;
    setSchemaDraft(formatSchema(node.output?.schema));
    setSchemaError(null);
    setSchemaDirty(false);
  }, [node.id, node.output?.schema]);

  useEffect(() => {
    if (!schemaDirty) return;
    const timeout = window.setTimeout(() => {
      commitSchemaDraft(schemaDraft);
      setSchemaDirty(false);
    }, SCHEMA_VALIDATION_DELAY_MS);
    return () => window.clearTimeout(timeout);
  }, [commitSchemaDraft, schemaDirty, schemaDraft]);

  const commitNodeId = (value: string) => {
    if (value === node.id) {
      setNodeIdDraft(node.id);
      return;
    }
    updateWorker({ id: value });
  };
  return (
    <InspectorCollapsible title={t('workflowEditor.nodeConfig')} meta={<Badge variant="outline">worker</Badge>}>
      <Field label={t('workflowEditor.nodeId')} required errors={errorsFor('id')}>
        <Input
          className={errorClass(errorsFor('id'))}
          value={nodeIdDraft}
          onChange={(event) => setNodeIdDraft(event.target.value)}
          onBlur={(event) => commitNodeId(event.target.value)}
          onCompositionStart={() => setNodeIdComposing(true)}
          onCompositionEnd={(event) => {
            setNodeIdComposing(false);
            setNodeIdDraft(event.currentTarget.value);
            commitNodeId(event.currentTarget.value);
          }}
          onKeyDown={(event) => {
            if (event.key !== 'Enter' || nodeIdComposing) return;
            event.currentTarget.blur();
          }}
        />
      </Field>
      <Field label={t('workflowEditor.agent')} required errors={errorsFor('provider')}>
        <Select value={node.provider ?? ''} onValueChange={(provider) => updateWorker(workerAgentSelectionPatch(provider))}>
          <SelectTrigger className={errorClass(errorsFor('provider'))}><SelectValue placeholder={t('workflowEditor.selectAgent')} /></SelectTrigger>
          <SelectContent>{agents.map((agent) => (
            <SelectItem value={agent.agentType} key={agent.agentType} disabled={!isWorkflowAgentDoctorReady(agent)}>
              <AgentSelectItemContent agent={agent} unavailableLabel={t('workflowEditor.agentDoctorUnavailable')} />
            </SelectItem>
          ))}</SelectContent>
        </Select>
        {agents.length === 0 ? <p className="text-xs text-muted-foreground">{t('workflowEditor.noDoctorReadyAgents')}</p> : null}
      </Field>
      {modelOptions.length > 0 || thoughtLevel ? (
        <Field label={t('workflowEditor.model')} errors={errorsFor('model')}>
          <AcpModelThoughtSelects
            models={modelOptions}
            modelValue={node.model}
            thoughtLevel={thoughtLevel}
            thoughtValue={thoughtLevel ? node.config_options?.[thoughtLevel.id] : null}
            compact
            triggerClassName={cn('w-full max-w-none rounded-md', errorClass(errorsFor('model')))}
            onModelChange={(model) => updateWorker({ model: model ?? undefined })}
            onThoughtChange={(optionId, value) => updateWorker({
              config_options: optionalWorkerConfigOptions(
                updateAcpConfigOptionOverride(node.config_options, optionId, value),
              ),
            })}
          />
        </Field>
      ) : null}
      <Field label={<ProfileLabel t={t} onOpenProfileManagement={onOpenProfileManagement} />} required errors={errorsFor('profile')}>
        <ProfilePicker profiles={profiles} value={node.profile ?? null} invalid={errorsFor('profile').length > 0} onChange={(profile) => updateWorker({ profile })} t={t} />
      </Field>
      <Field label={t('workflowEditor.permissionMode')} errors={errorsFor('permission_mode')}>
        <Select value={node.permission_mode ?? UNSPECIFIED_PERMISSION_MODE} onValueChange={(value) => updateWorker({ permission_mode: value === UNSPECIFIED_PERMISSION_MODE ? undefined : value })}>
          <SelectTrigger className={errorClass(errorsFor('permission_mode'))}>
            <SelectValue placeholder={t('workflowEditor.permissionModeUnspecified')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={UNSPECIFIED_PERMISSION_MODE}>{t('workflowEditor.permissionModeUnspecified')}</SelectItem>
            {permissionModes.map((mode) => <SelectItem value={mode.id} key={mode.id}>{mode.name}</SelectItem>)}
          </SelectContent>
        </Select>
      </Field>
      <Field label={t('workflowEditor.goal')} errors={errorsFor('goal')}>
        <Textarea className={errorClass(errorsFor('goal'))} value={node.goal ?? ''} placeholder={t('workflowEditor.defaultNodeGoal')} onChange={(event) => updateWorker({ goal: event.target.value })} />
      </Field>
      <div className="space-y-3 rounded-lg border bg-muted/10 p-3">
        <div className="space-y-1">
          <span className="text-sm font-medium">{t('workflowEditor.resultMode')}</span>
          <p className="text-xs text-muted-foreground">{t('workflowEditor.resultModeDescription')}</p>
        </div>
        <Select
          value={resultMode}
          onValueChange={(mode) => {
            setSchemaDraft('');
            setSchemaError(null);
            setSchemaDirty(false);
            if (mode === 'ai') updateWorker({ ...defaultValidationPatch(node.id), manual_check: null });
            if (mode === 'manual') updateWorker({ ...clearValidationPatch, manual_check: true });
            if (mode === 'none') updateWorker({ ...clearValidationPatch, manual_check: null });
          }}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="none">{t('workflowEditor.resultModeNone')}</SelectItem>
            <SelectItem value="ai">{t('workflowEditor.outputValidation')}</SelectItem>
            <SelectItem value="manual">{t('workflowEditor.manualCheck')}</SelectItem>
          </SelectContent>
        </Select>
        {validationEnabled ? <p className="text-xs leading-5 text-muted-foreground">{t('workflowEditor.outputValidationDescription')}</p> : null}
        {manualCheckEnabled ? <p className="text-xs leading-5 text-muted-foreground">{t('workflowEditor.manualCheckDescription')}</p> : null}
        {validationEnabled ? (
          <div className="space-y-3 rounded-lg border bg-background/55 p-3">
            <Field label={t('workflowEditor.outputArtifact')} required errors={errorsFor('output.artifact')}>
              <Input className={errorClass(errorsFor('output.artifact'))} value={node.output?.artifact ?? ''} onChange={(event) => updateOutput({ artifact: event.target.value })} />
            </Field>
            <Field label={<HelpLabel label={t('workflowEditor.outputSchema')} help={t('workflowEditor.outputSchemaHelp')} />} errors={errorsFor('output.schema')}>
              <div className="relative">
                <Textarea
                  className={cn('min-h-28 pr-11 font-mono text-xs', errorClass(errorsFor('output.schema')))}
                  value={schemaDraft}
                  placeholder={t('workflowEditor.outputSchemaPlaceholder')}
                  onChange={(event) => {
                    setSchemaDraft(event.target.value);
                    setSchemaError(null);
                    setSchemaDirty(true);
                  }}
                  onBlur={() => {
                    if (!schemaDirty) return;
                    commitSchemaDraft(schemaDraft);
                    setSchemaDirty(false);
                  }}
                />
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        type="button"
                        variant="secondary"
                        size="icon-xs"
                        className="absolute right-2 top-2 border border-border/70 bg-background/90 shadow-sm backdrop-blur hover:bg-muted"
                        aria-label={t('workflowEditor.outputSchemaBeautify')}
                        onMouseDown={(event) => event.preventDefault()}
                        onClick={beautifySchemaDraft}
                      >
                        <Sparkles className="size-3.5" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>
                      {t('workflowEditor.outputSchemaBeautify')}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </div>
              {schemaError ? <span className="text-xs text-destructive">{schemaError}</span> : null}
            </Field>
            <Field label={<HelpLabel label={t('workflowEditor.successExpression')} help={t('workflowEditor.successExpressionHelp')} />} required errors={errorsFor('success_condition')}>
              <Input className={cn('font-mono', errorClass(errorsFor('success_condition')))} value={expression} placeholder="$.result == true" onChange={(event) => updateWorker({ manual_check: null, success_condition: { expression: event.target.value } })} />
            </Field>
          </div>
        ) : null}
      </div>
    </InspectorCollapsible>
  );
}

function AiDynamicNodeInspector({ node, agents, profiles, workflowTemplates, fieldErrors, onUpdate, t }: { node: WorkflowAiDynamicNodeDsl; agents: ManagedAgentVm[]; profiles: ProfileVm[]; workflow: WorkflowDsl; workflowTemplates: WorkflowTemplateStore | null; fieldErrors: Record<string, string[]>; onUpdate: (nodeId: string, patch: Partial<WorkflowNodeDsl>) => void; onOpenProfileManagement?: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const [nodeIdDraft, setNodeIdDraft] = useState(node.id);
  const [nodeIdComposing, setNodeIdComposing] = useState(false);
  const control = { ...defaultDynamicControl(), ...(node.control ?? {}) };
  const templates = workflowTemplates?.templates ?? [];
  const errorsFor = (field: string) => fieldErrors[`node:${node.id}:${field}`] ?? [];
  const updateDynamic = (patch: Partial<WorkflowAiDynamicNodeDsl>) => onUpdate(node.id, patch as Partial<WorkflowNodeDsl>);
  const updateControl = (patch: Partial<DynamicControlDsl>) => {
    updateDynamic({ control: { ...control, ...patch } } as Partial<WorkflowAiDynamicNodeDsl>);
  };
  const updateAgentStrategy = (agentStrategy: WorkflowAiDynamicNodeDsl['agentStrategy']) => {
    updateDynamic({ agentStrategy });
  };
  const parseLimit = (value: string) => {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.trunc(parsed) : 0;
  };
  const commitNodeId = (value: string) => {
    if (value === node.id) {
      setNodeIdDraft(node.id);
      return;
    }
    updateDynamic({ id: value });
  };

  useEffect(() => {
    setNodeIdDraft(node.id);
  }, [node.id]);

  return (
    <InspectorCollapsible title={t('workflowEditor.nodeConfig')} meta={<Badge variant="outline">{t('workflowEditor.addAiDynamicNode')}</Badge>}>
      <Field label={t('workflowEditor.nodeId')} required errors={errorsFor('id')}>
        <Input
          className={errorClass(errorsFor('id'))}
          value={nodeIdDraft}
          onChange={(event) => setNodeIdDraft(event.target.value)}
          onBlur={(event) => commitNodeId(event.target.value)}
          onCompositionStart={() => setNodeIdComposing(true)}
          onCompositionEnd={(event) => {
            setNodeIdComposing(false);
            setNodeIdDraft(event.currentTarget.value);
            commitNodeId(event.currentTarget.value);
          }}
          onKeyDown={(event) => {
            if (event.key !== 'Enter' || nodeIdComposing) return;
            event.currentTarget.blur();
          }}
        />
      </Field>
      <Field label={<HelpLabel label={t('workflowEditor.dynamicAgentStrategy')} help={t('workflowEditor.dynamicAgentStrategyHelp')} />} required errors={errorsFor('agentStrategy.mode')}>
        <Select
          value={node.agentStrategy.mode}
          onValueChange={(mode) => {
            const cur = node.agentStrategy;
            if (mode === 'fixed') {
              const nextProvider = cur.mode === 'fixed'
                ? cur.provider
                : cur.bootstrapProvider;
              updateDynamic({
                configOptions: {},
                agentStrategy: { mode: 'fixed', provider: nextProvider, model: undefined, permissionMode: undefined },
              });
              return;
            }
            const curDynamic = node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl;
            const nextBootstrapProvider = cur.mode === 'dynamic'
              ? cur.bootstrapProvider
              : (cur as WorkflowAiDynamicFixedAgentStrategyDsl).provider;
            const nextRoutingPrompt = cur.mode === 'dynamic'
              ? cur.routingPrompt
              : '';
            const nextBootstrapModel = cur.mode === 'dynamic'
              ? cur.bootstrapModel
              : undefined;
            const nextPermissionMode = cur.mode === 'dynamic'
              ? cur.permissionMode
              : (cur as WorkflowAiDynamicFixedAgentStrategyDsl).permissionMode;
            const nextAcceptanceModel = cur.mode === 'dynamic'
              ? cur.acceptanceModel
              : undefined;
            const nextAvailableAgents = cur.mode === 'dynamic'
              ? cur.availableAgents
              : [];
            updateDynamic({
              configOptions: {},
              agentStrategy: {
                mode: 'dynamic',
                bootstrapProvider: nextBootstrapProvider,
                bootstrapModel: nextBootstrapModel,
                permissionMode: nextPermissionMode,
                acceptanceModel: nextAcceptanceModel,
                routingPrompt: nextRoutingPrompt,
                availableAgents: nextAvailableAgents,
              },
            });
          }}
        >
          <SelectTrigger className={errorClass(errorsFor('agentStrategy.mode'))}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="fixed">{t('workflowEditor.dynamicAgentStrategyFixed')}</SelectItem>
            <SelectItem value="dynamic">{t('workflowEditor.dynamicAgentStrategyDynamic')}</SelectItem>
          </SelectContent>
        </Select>
      </Field>
      {node.agentStrategy.mode === 'fixed' ? (
        <>
          <Field label={<HelpLabel label={t('workflowEditor.agent')} help={t('workflowEditor.dynamicFixedAgentHelp')} />} required errors={errorsFor('agentStrategy.provider')}>
            <Select value={node.agentStrategy.provider} onValueChange={(provider) => updateDynamic({
              configOptions: {},
              agentStrategy: { mode: 'fixed', provider, model: undefined, permissionMode: undefined },
            })}>
              <SelectTrigger className={errorClass(errorsFor('agentStrategy.provider'))}><SelectValue placeholder={t('workflowEditor.selectAgent')} /></SelectTrigger>
              <SelectContent>{agents.map((agent) => (
                <SelectItem value={agent.agentType} key={agent.agentType} disabled={!isWorkflowAgentDoctorReady(agent)}>
                  <AgentSelectItemContent agent={agent} unavailableLabel={t('workflowEditor.agentDoctorUnavailable')} />
                </SelectItem>
              ))}</SelectContent>
            </Select>
          </Field>
          {(() => {
            const fixedStrategy = node.agentStrategy as WorkflowAiDynamicFixedAgentStrategyDsl;
            const fixedAgent = agents.find((a) => a.agentType === fixedStrategy.provider);
            const fixedModels = fixedAgent?.supportedModels ?? [];
            const fixedModes = fixedAgent?.supportedModes ?? [];
            const fixedThoughtLevel = findAcpThoughtLevel(fixedAgent?.configOptions);
            if (fixedModels.length > 0 || fixedThoughtLevel || fixedModes.length > 0) {
              return (
                <Field label={t('workflowEditor.model')} errors={errorsFor('agentStrategy.model')}>
                  <div className="flex flex-wrap gap-2">
                    <AcpModelThoughtSelects
                      models={fixedModels}
                      modelValue={fixedStrategy.model}
                      thoughtLevel={fixedThoughtLevel}
                      thoughtValue={fixedThoughtLevel ? node.configOptions?.[fixedThoughtLevel.id] : null}
                      compact
                      triggerClassName={cn('min-w-[12rem] flex-1 rounded-md', errorClass(errorsFor('agentStrategy.model')))}
                      onModelChange={(model) => updateAgentStrategy({ ...fixedStrategy, model: model || undefined })}
                      onThoughtChange={(optionId, value) => updateDynamic({
                        configOptions: updateAcpConfigOptionOverride(node.configOptions, optionId, value),
                      })}
                    />
                    {fixedModes.length > 0 ? (
                      <AcpSingleConfigMenu
                        label={t('acp.permissionMode')}
                        value={fixedStrategy.permissionMode}
                        options={fixedModes}
                        unspecifiedLabel={t('workflowEditor.permissionModeUnspecified')}
                        compact
                        triggerClassName="min-w-[12rem] flex-1 rounded-md"
                        onValueChange={(permissionMode) => updateAgentStrategy({ ...fixedStrategy, permissionMode })}
                      />
                    ) : null}
                  </div>
                </Field>
              );
            }
            return null;
          })()}
        </>
      ) : (
        <>
          <Field label={<HelpLabel label={t('workflowEditor.dynamicBootstrapAgent')} help={t('workflowEditor.dynamicBootstrapAgentHelp')} />} required errors={errorsFor('agentStrategy.bootstrapProvider')}>
            <Select value={node.agentStrategy.bootstrapProvider} onValueChange={(bootstrapProvider) => updateDynamic({
              configOptions: {},
              agentStrategy: {
                ...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl),
                bootstrapProvider,
                bootstrapModel: undefined,
                permissionMode: undefined,
                bootstrapConfigOptions: {},
                acceptanceModel: undefined,
                acceptanceConfigOptions: {},
              },
            })}>
              <SelectTrigger className={errorClass(errorsFor('agentStrategy.bootstrapProvider'))}><SelectValue placeholder={t('workflowEditor.selectAgent')} /></SelectTrigger>
              <SelectContent>{agents.map((agent) => (
                <SelectItem value={agent.agentType} key={agent.agentType} disabled={!isWorkflowAgentDoctorReady(agent)}>
                  <AgentSelectItemContent agent={agent} unavailableLabel={t('workflowEditor.agentDoctorUnavailable')} />
                </SelectItem>
              ))}</SelectContent>
            </Select>
          </Field>
          {(() => {
            const dynamicStrategy = node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl;
            const bootstrapAgent = agents.find((agent) => agent.agentType === dynamicStrategy.bootstrapProvider);
            const bootstrapModels = bootstrapAgent?.supportedModels ?? [];
            const bootstrapThoughtLevel = findAcpThoughtLevel(bootstrapAgent?.configOptions);
            if (bootstrapModels.length === 0 && !bootstrapThoughtLevel) return null;
            return (
              <Field label={t('workflowEditor.dynamicBootstrapModel')} errors={errorsFor('agentStrategy.bootstrapModel')}>
                <AcpModelThoughtSelects
                  models={bootstrapModels}
                  modelValue={dynamicStrategy.bootstrapModel}
                  thoughtLevel={bootstrapThoughtLevel}
                  thoughtValue={bootstrapThoughtLevel ? dynamicStrategy.bootstrapConfigOptions?.[bootstrapThoughtLevel.id] : null}
                  compact
                  triggerClassName={cn('w-full max-w-none rounded-md', errorClass(errorsFor('agentStrategy.bootstrapModel')))}
                  onModelChange={(model) => updateAgentStrategy({ ...dynamicStrategy, bootstrapModel: model || undefined })}
                  onThoughtChange={(optionId, value) => updateAgentStrategy({
                    ...dynamicStrategy,
                    bootstrapConfigOptions: updateAcpConfigOptionOverride(dynamicStrategy.bootstrapConfigOptions, optionId, value),
                  })}
                />
              </Field>
            );
          })()}
          {(() => {
            const dynamicStrategy = node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl;
            const acceptanceAgent = agents.find((agent) => agent.agentType === dynamicStrategy.bootstrapProvider);
            const acceptanceModels = acceptanceAgent?.supportedModels ?? [];
            const acceptanceThoughtLevel = findAcpThoughtLevel(acceptanceAgent?.configOptions);
            if (acceptanceModels.length === 0 && !acceptanceThoughtLevel) return null;
            return (
              <Field label={<HelpLabel label={t('workflowEditor.dynamicAcceptanceModel')} help={t('workflowEditor.dynamicAcceptanceModelHelp')} />} errors={errorsFor('agentStrategy.acceptanceModel')}>
                <AcpModelThoughtSelects
                  models={acceptanceModels}
                  modelValue={dynamicStrategy.acceptanceModel}
                  thoughtLevel={acceptanceThoughtLevel}
                  thoughtValue={acceptanceThoughtLevel ? dynamicStrategy.acceptanceConfigOptions?.[acceptanceThoughtLevel.id] : null}
                  compact
                  triggerClassName={cn('w-full max-w-none rounded-md', errorClass(errorsFor('agentStrategy.acceptanceModel')))}
                  onModelChange={(model) => updateAgentStrategy({ ...dynamicStrategy, acceptanceModel: model || undefined, acceptanceConfigOptions: {} })}
                  onThoughtChange={(optionId, value) => updateAgentStrategy({
                    ...dynamicStrategy,
                    acceptanceConfigOptions: updateAcpConfigOptionOverride(dynamicStrategy.acceptanceConfigOptions, optionId, value),
                  })}
                />
              </Field>
            );
          })()}
          {(() => {
            const dynamicStrategy = node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl;
            const bootstrapAgent = agents.find((agent) => agent.agentType === dynamicStrategy.bootstrapProvider);
            const bootstrapModes = bootstrapAgent?.supportedModes ?? [];
            if (bootstrapModes.length === 0) return null;
            return (
              <Field label={<HelpLabel label={t('workflowEditor.dynamicControlPermission')} help={t('workflowEditor.dynamicControlPermissionHelp')} />} errors={errorsFor('agentStrategy.permissionMode')}>
                <AcpSingleConfigMenu
                  label={t('acp.permissionMode')}
                  value={dynamicStrategy.permissionMode}
                  options={bootstrapModes}
                  unspecifiedLabel={t('workflowEditor.permissionModeUnspecified')}
                  compact
                  triggerClassName={cn('w-full rounded-md', errorClass(errorsFor('agentStrategy.permissionMode')))}
                  onValueChange={(permissionMode) => updateAgentStrategy({ ...dynamicStrategy, permissionMode })}
                />
              </Field>
            );
          })()}
          <Field label={<HelpLabel label={t('workflowEditor.dynamicAvailableAgents')} help={t('workflowEditor.dynamicAvailableAgentsHelp')} />} required errors={errorsFor('agentStrategy.availableAgents')}>
            <AgentMultiSelect
              agents={agents}
              selectedAgents={node.agentStrategy.availableAgents ?? []}
              invalid={errorsFor('agentStrategy.availableAgents').length > 0}
              onChange={(availableAgents) => updateAgentStrategy({ ...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl), availableAgents })}
              t={t}
            />
          </Field>
          {(node.agentStrategy.availableAgents ?? []).map((agentRef, idx) => {
            const agentObj = agents.find((a) => a.agentType === agentRef.provider);
            const agentModels = agentObj?.supportedModels ?? [];
            const agentModes = agentObj?.supportedModes ?? [];
            const thoughtLevel = findAcpThoughtLevel(agentObj?.configOptions);
            if (agentModels.length === 0 && !thoughtLevel && agentModes.length === 0) return null;
            return (
              <Field key={agentRef.provider} label={`${t('workflowEditor.model')} — ${agentObj!.displayName}`} errors={errorsFor(`agentStrategy.availableAgents.${idx}.model`)}>
                <div className="flex flex-wrap gap-2">
                  <AcpModelThoughtSelects
                    models={agentModels}
                    modelValue={agentRef.model}
                    thoughtLevel={thoughtLevel}
                    thoughtValue={thoughtLevel ? agentRef.configOptions?.[thoughtLevel.id] : null}
                    compact
                    triggerClassName={cn('min-w-[12rem] flex-1 rounded-md', errorClass(errorsFor(`agentStrategy.availableAgents.${idx}.model`)))}
                    onModelChange={(model) => {
                      const next = [...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl).availableAgents];
                      next[idx] = { ...next[idx], model: model || undefined };
                      updateAgentStrategy({ ...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl), availableAgents: next });
                    }}
                    onThoughtChange={(optionId, value) => {
                      const next = [...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl).availableAgents];
                      next[idx] = {
                        ...next[idx],
                        configOptions: updateAcpConfigOptionOverride(next[idx].configOptions, optionId, value),
                      };
                      updateAgentStrategy({ ...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl), availableAgents: next });
                    }}
                  />
                  {agentModes.length > 0 ? (
                    <AcpSingleConfigMenu
                      label={t('acp.permissionMode')}
                      value={agentRef.permissionMode}
                      options={agentModes}
                      unspecifiedLabel={t('workflowEditor.permissionModeUnspecified')}
                      compact
                      triggerClassName="min-w-[12rem] flex-1 rounded-md"
                      onValueChange={(permissionMode) => {
                        const next = [...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl).availableAgents];
                        next[idx] = { ...next[idx], permissionMode };
                        updateAgentStrategy({ ...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl), availableAgents: next });
                      }}
                    />
                  ) : null}
                </div>
              </Field>
            );
          })}
          <Field label={<HelpLabel label={t('workflowEditor.dynamicAgentRoutingPrompt')} help={t('workflowEditor.dynamicAgentRoutingPromptHelp')} />} errors={errorsFor('agentStrategy.routingPrompt')}>
            <Textarea
              className={errorClass(errorsFor('agentStrategy.routingPrompt'))}
              value={node.agentStrategy.routingPrompt}
              placeholder={t('workflowEditor.dynamicAgentRoutingPromptPlaceholder')}
              onChange={(event) => updateAgentStrategy({ ...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl), routingPrompt: event.target.value })}
            />
          </Field>
        </>
      )}
      <Field label={<HelpLabel label={t('workflowEditor.allowedWorkflows')} help={t('workflowEditor.allowedWorkflowsHelp')} />} errors={errorsFor('allowedWorkflows')}>
        <AllowedWorkflowMultiSelect
          templates={templates}
          selectedWorkflowIds={(node.allowedWorkflows ?? []).map((item) => item.workflowId)}
          allowNestedDynamic={false}
          invalid={errorsFor('allowedWorkflows').length > 0}
          onChange={(workflowIds) => updateDynamic({ allowedWorkflows: workflowIds.map((workflowId) => ({ workflowId })) })}
          t={t}
        />
      </Field>
      <Field label={<HelpLabel label={t('workflowEditor.allowedProfiles')} help={t('workflowEditor.allowedProfilesHelp')} />} errors={errorsFor('allowedProfiles')}>
        <ProfileMultiSelect
          profiles={profiles}
          selectedProfileIds={node.allowedProfiles ?? []}
          invalid={errorsFor('allowedProfiles').length > 0}
          onChange={(profileIds) => updateDynamic({ allowedProfiles: profileIds })}
          t={t}
        />
      </Field>
      <Field label={<HelpLabel label={t('workflowEditor.globalGoal')} help={t('workflowEditor.globalGoalHelp')} />} errors={errorsFor('globalGoal')}>
        <Textarea
          className={errorClass(errorsFor('globalGoal'))}
          value={node.globalGoal ?? ''}
          placeholder={t('workflowEditor.globalGoalPlaceholder')}
          onChange={(event) => updateDynamic({ globalGoal: event.target.value || null } as Partial<WorkflowAiDynamicNodeDsl>)}
        />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        {dynamicControlFields(t).map((field) => (
          <Field key={field.key} label={<HelpLabel label={field.label} help={field.help} />} required errors={errorsFor(`control.${field.key}`)}>
            <Input className={errorClass(errorsFor(`control.${field.key}`))} type="number" min={1} step={1} value={String(control[field.key])} onChange={(event) => updateControl({ [field.key]: parseLimit(event.target.value) } as Partial<DynamicControlDsl>)} />
          </Field>
        ))}
      </div>
    </InspectorCollapsible>
  );
}

function WorkflowEditorSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Collapsible className="rounded-lg border bg-muted/10">
      <CollapsibleTrigger className="flex w-full items-center justify-between gap-3 px-3 py-2.5 text-left text-sm font-medium">
        <span>{title}</span>
        <ChevronDown className="size-4 text-muted-foreground transition-transform data-[state=open]:rotate-180" />
      </CollapsibleTrigger>
      <CollapsibleContent className="space-y-3 border-t px-3 py-3">
        {children}
      </CollapsibleContent>
    </Collapsible>
  );
}

type MultiSelectPopoverProps<T> = {
  items: T[];
  getItemId: (item: T) => string;
  filterFn: (item: T, search: string) => boolean;
  isSelected: (id: string) => boolean;
  isItemDisabled?: (item: T, isSelected: boolean) => boolean;
  onToggle: (id: string) => void;
  onRemove: (id: string) => void;
  renderBadge: (id: string) => ReactNode;
  renderItem: (item: T, isSelected: boolean) => ReactNode;
  placeholder: string;
  emptyMessage: string;
  triggerEmptyLabel: string;
  showTriggerEmpty: boolean;
  invalid: boolean;
  resetSearchOnClose?: boolean;
};

function MultiSelectPopover<T>({ items, getItemId, filterFn, isSelected, isItemDisabled, onToggle, onRemove, renderBadge, renderItem, placeholder, emptyMessage, triggerEmptyLabel, showTriggerEmpty, invalid, resetSearchOnClose }: MultiSelectPopoverProps<T>) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const selectedIds = useMemo(() => new Set(items.filter((item) => isSelected(getItemId(item))).map(getItemId)), [items, isSelected, getItemId]);
  const filteredItems = useMemo(
    () => (search.trim() ? items.filter((item) => filterFn(item, search)) : items),
    [items, search, filterFn],
  );

  return (
    <Popover open={open} onOpenChange={(nextOpen) => {
      setOpen(nextOpen);
      if (!nextOpen && resetSearchOnClose) setSearch('');
    }} modal>
      <PopoverTrigger asChild>
        <Button variant="outline" role="combobox" className={cn('h-auto min-h-9 w-full justify-between px-2 py-1.5 font-normal', invalid && 'border-destructive')}>
          <span className="flex min-w-0 flex-1 flex-wrap gap-1">
            {[...selectedIds].map((id) => (
              <Badge key={id} variant="secondary" className="max-w-full gap-1">
                {renderBadge(id)}
                <span role="button" tabIndex={0} className="rounded-full hover:text-destructive" onClick={(event) => { event.preventDefault(); event.stopPropagation(); onRemove(id); }} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') onRemove(id); }}>
                  <X className="size-3" />
                </span>
              </Badge>
            ))}
            {showTriggerEmpty && selectedIds.size === 0 ? <span className="px-1 text-muted-foreground">{triggerEmptyLabel}</span> : null}
          </span>
          <ChevronsUpDown className="ml-2 size-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0" align="start">
        <Command shouldFilter={false}>
          <CommandInput value={search} onValueChange={setSearch} placeholder={placeholder} />
          <CommandList>
            {filteredItems.length === 0 ? <CommandEmpty>{emptyMessage}</CommandEmpty> : null}
            <CommandGroup>
              {filteredItems.map((item) => {
                const id = getItemId(item);
                const selected = isSelected(id);
                return (
                  <CommandItem key={id} value={id} disabled={isItemDisabled?.(item, selected)} onSelect={() => onToggle(id)} className="items-start py-2">
                    <Check className={cn('mt-0.5 size-4', selected ? 'opacity-100' : 'opacity-0')} />
                    {renderItem(item, selected)}
                  </CommandItem>
                );
              })}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function AgentMultiSelect({ agents, selectedAgents, invalid, onChange, t }: { agents: ManagedAgentVm[]; selectedAgents: DynamicAgentRefDsl[]; invalid: boolean; onChange: (agents: DynamicAgentRefDsl[]) => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const selectedMap = useMemo(
    () => new Map(selectedAgents.map((a) => [a.provider, a])),
    [selectedAgents],
  );
  const toggleAgent = useCallback(
    (agentType: string) => {
      const next = new Map(selectedMap);
      if (next.has(agentType)) {
        next.delete(agentType);
      } else {
        const agent = agents.find((item) => item.agentType === agentType);
        if (!agent || !isWorkflowAgentDoctorReady(agent)) return;
        next.set(agentType, { provider: agentType });
      }
      onChange(Array.from(next.values()));
    },
    [agents, selectedMap, onChange],
  );
  const getItemId = useCallback((a: ManagedAgentVm) => a.agentType, []);
  const filterFn = useCallback(
    (a: ManagedAgentVm, s: string) =>
      a.agentType.toLowerCase().includes(s.toLowerCase()) ||
      a.displayName.toLowerCase().includes(s.toLowerCase()),
    [],
  );
  const isSelected = useCallback((id: string) => selectedMap.has(id), [selectedMap]);
  const onRemove = useCallback(
    (id: string) => {
      const next = new Map(selectedMap);
      next.delete(id);
      onChange(Array.from(next.values()));
    },
    [selectedMap, onChange],
  );
  const renderBadge = useCallback(
    (id: string) => {
      const agent = agents.find((a) => a.agentType === id);
      return (
        <>
          <span className="max-w-40 truncate">{agent?.displayName ?? id}</span>
          <span className="font-mono text-[10px] text-muted-foreground">{id}</span>
        </>
      );
    },
    [agents],
  );
  const renderItem = useCallback(
    (agent: ManagedAgentVm, _selected: boolean) => {
      const reason = isWorkflowAgentDoctorReady(agent)
        ? null
        : (agent.diagnostic?.reason ?? t('workflowEditor.agentDoctorUnavailable'));
      return (
      <span className={cn('flex min-w-0 flex-col', reason && 'opacity-60')}>
        <span>{agent.displayName}</span>
        <span className="font-mono text-[11px] text-muted-foreground">{agent.agentType}</span>
        {reason ? <span className="max-w-[22rem] truncate text-[11px] text-destructive">{reason}</span> : null}
      </span>
      );
    },
    [t],
  );

  return (
    <MultiSelectPopover
      items={agents}
      getItemId={getItemId}
      filterFn={filterFn}
      isSelected={isSelected}
      isItemDisabled={(agent, selected) => !selected && !isWorkflowAgentDoctorReady(agent)}
      onToggle={toggleAgent}
      onRemove={onRemove}
      renderBadge={renderBadge}
      renderItem={renderItem}
      placeholder={t('workflowEditor.selectAgent')}
      emptyMessage={t('workflowEditor.noDoctorReadyAgents')}
      triggerEmptyLabel={t('workflowEditor.dynamicAvailableAgents')}
      showTriggerEmpty
      invalid={invalid}
    />
  );
}

function ProfileMultiSelect({ profiles, selectedProfileIds, invalid, onChange, t }: { profiles: ProfileVm[]; selectedProfileIds: string[]; invalid: boolean; onChange: (profileIds: string[]) => void; t: (key: string) => string }) {
  const selected = useMemo(() => new Set(selectedProfileIds), [selectedProfileIds]);
  const profileById = useMemo(() => new Map(profiles.map((profile) => [profile.id, profile] as const)), [profiles]);
  const selectedProfiles = useMemo(
    () => selectedProfileIds.map((pid) => profileById.get(pid)).filter((p): p is ProfileVm => Boolean(p)),
    [selectedProfileIds, profileById],
  );
  const invalidProfileIds = useMemo(
    () => selectedProfileIds.filter((pid) => !profileById.has(pid)),
    [selectedProfileIds, profileById],
  );
  const toggleProfile = (profileId: string) => {
    onChange(
      selected.has(profileId)
        ? selectedProfileIds.filter((item) => item !== profileId)
        : [...selectedProfileIds, profileId],
    );
  };

  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const normalizedSearch = search.trim().toLowerCase();
  const filteredProfiles = normalizedSearch
    ? profiles.filter((profile) => profileSearchText(profile).includes(normalizedSearch))
    : profiles;

  return (
    <Popover open={open} onOpenChange={(nextOpen) => {
      setOpen(nextOpen);
      if (!nextOpen) setSearch('');
    }} modal>
      <PopoverTrigger asChild>
        <Button variant="outline" role="combobox" aria-expanded={open} className={cn('h-auto min-h-9 w-full justify-between px-2 py-1.5 font-normal', invalid && 'border-destructive text-destructive focus-visible:ring-destructive')}>
          <span className="flex min-w-0 flex-1 flex-wrap gap-1">
            {selectedProfiles.map((profile) => (
              <Badge key={profile.id} variant="secondary" className="max-w-full gap-1">
                <span className="max-w-40 truncate">{profile.name}</span>
                <span className="font-mono text-[10px] text-muted-foreground">{profile.id}</span>
                <span role="button" tabIndex={0} className="rounded-full hover:text-destructive" onClick={(event) => { event.preventDefault(); event.stopPropagation(); onChange(selectedProfileIds.filter((item) => item !== profile.id)); }} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') onChange(selectedProfileIds.filter((item) => item !== profile.id)); }}>
                  <X className="size-3" />
                </span>
              </Badge>
            ))}
            {invalidProfileIds.map((profileId) => (
              <Badge key={profileId} variant="destructive" className="max-w-full gap-1">
                <span className="max-w-44 truncate font-mono text-[10px]">{profileId}</span>
                <span role="button" tabIndex={0} className="rounded-full" onClick={(event) => { event.preventDefault(); event.stopPropagation(); onChange(selectedProfileIds.filter((item) => item !== profileId)); }} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') onChange(selectedProfileIds.filter((item) => item !== profileId)); }}>
                  <X className="size-3" />
                </span>
              </Badge>
            ))}
            {selectedProfiles.length === 0 && invalidProfileIds.length === 0 ? <span className="px-1 text-muted-foreground">{t('workflowEditor.selectAllowedProfiles')}</span> : null}
          </span>
          <ChevronsUpDown className="ml-2 size-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0" align="start">
        <Command shouldFilter={false}>
          <CommandInput value={search} onValueChange={setSearch} placeholder={t('workflowEditor.searchProfiles')} />
          <CommandList>
            {filteredProfiles.length === 0 ? <CommandEmpty>{t('workflowEditor.noProfiles')}</CommandEmpty> : null}
            <CommandGroup>
              {filteredProfiles.map((profile) => (
                <CommandItem key={`${profile.scope}:${profile.id}`} value={profile.id} onSelect={() => toggleProfile(profile.id)} className="items-start py-2">
                  <Check className={cn('mt-0.5 size-4', selected.has(profile.id) ? 'opacity-100' : 'opacity-0')} />
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center justify-between gap-2 font-medium"><span className="truncate">{profile.name}</span><span className="shrink-0 text-[11px] text-muted-foreground">{profileScopeText(t, profile.scope)}</span></span>
                    <span className="mt-1 block truncate font-mono text-[11px] text-muted-foreground">{profile.id}</span>
                    <TooltipProvider>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <span className="mt-1 block truncate text-xs text-muted-foreground">{profile.summary}</span>
                        </TooltipTrigger>
                        <TooltipContent className="max-w-80 whitespace-pre-wrap break-words text-xs" sideOffset={6}>{profile.summary}</TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                    <span className="mt-1 block text-[11px] text-muted-foreground">{formatLocalDateTime(profile.createdAt)} / {formatLocalDateTime(profile.updatedAt)}</span>
                  </span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function AllowedWorkflowMultiSelect({ templates, selectedWorkflowIds, allowNestedDynamic, invalid, onChange, t }: { templates: WorkflowTemplate[]; selectedWorkflowIds: string[]; allowNestedDynamic: boolean; invalid: boolean; onChange: (workflowIds: string[]) => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const [open, setOpen] = useState(false);
  const selected = useMemo(() => new Set(selectedWorkflowIds), [selectedWorkflowIds]);
  const workflowIdCounts = useMemo(() => workflowIdCountMap(templates), [templates]);
  const uniqueSelectableTemplateByWorkflowId = useMemo(
    () =>
      new Map(
        templates
          .filter((template) => workflowDisabledReason(template, workflowIdCounts, allowNestedDynamic, t) === null)
          .map((template) => [template.workflow.id.trim(), template] as const),
      ),
    [templates, workflowIdCounts, allowNestedDynamic, t],
  );
  const selectedTemplates = useMemo(
    () =>
      selectedWorkflowIds
        .map((workflowId) => uniqueSelectableTemplateByWorkflowId.get(workflowId))
        .filter((template): template is WorkflowTemplate => Boolean(template)),
    [selectedWorkflowIds, uniqueSelectableTemplateByWorkflowId],
  );
  const invalidWorkflowIds = useMemo(
    () => selectedWorkflowIds.filter((workflowId) => !uniqueSelectableTemplateByWorkflowId.has(workflowId)),
    [selectedWorkflowIds, uniqueSelectableTemplateByWorkflowId],
  );
  const workflowOptions = useMemo(
    () =>
      templates.map((template) => ({
        template,
        reason: workflowDisabledReason(template, workflowIdCounts, allowNestedDynamic, t),
      })),
    [templates, workflowIdCounts, allowNestedDynamic, t],
  );
  const selectableOptions = useMemo(
    () => workflowOptions.filter((option) => option.reason === null),
    [workflowOptions],
  );
  const disabledOptions = useMemo(
    () => workflowOptions.filter((option) => option.reason !== null),
    [workflowOptions],
  );
  const toggleWorkflow = (workflowId: string) => {
    const next = selected.has(workflowId)
      ? selectedWorkflowIds.filter((item) => item !== workflowId)
      : [...selectedWorkflowIds, workflowId];
    onChange(next);
  };
  const removeWorkflow = (workflowId: string) => onChange(selectedWorkflowIds.filter((item) => item !== workflowId));

  return (
    <Popover open={open} onOpenChange={setOpen} modal>
      <PopoverTrigger asChild>
        <Button variant="outline" role="combobox" aria-expanded={open} className={cn('h-auto min-h-9 w-full justify-between px-2 py-1.5 font-normal', invalid && 'border-destructive text-destructive focus-visible:ring-destructive')}>
          <span className="flex min-w-0 flex-1 flex-wrap gap-1">
            {selectedTemplates.map((template) => (
              <Badge key={template.workflow.id} variant="secondary" className="max-w-full gap-1">
                <span className="max-w-40 truncate">{workflowTemplateDisplayName(template, t)}</span>
                <span className="font-mono text-[10px] text-muted-foreground">{template.workflow.id}</span>
                <span role="button" tabIndex={0} className="rounded-full hover:text-destructive" onClick={(event) => { event.preventDefault(); event.stopPropagation(); removeWorkflow(template.workflow.id); }} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') removeWorkflow(template.workflow.id); }}>
                  <X className="size-3" />
                </span>
              </Badge>
            ))}
            {invalidWorkflowIds.map((workflowId) => (
              <Badge key={workflowId} variant="destructive" className="max-w-full gap-1">
                <span className="max-w-44 truncate font-mono text-[10px]">{workflowId}</span>
                <span role="button" tabIndex={0} className="rounded-full" onClick={(event) => { event.preventDefault(); event.stopPropagation(); removeWorkflow(workflowId); }} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') removeWorkflow(workflowId); }}>
                  <X className="size-3" />
                </span>
              </Badge>
            ))}
            {selectedTemplates.length === 0 && invalidWorkflowIds.length === 0 ? <span className="px-1 text-muted-foreground">{t('workflowEditor.selectAllowedWorkflows')}</span> : null}
          </span>
          <ChevronsUpDown className="ml-2 size-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0" align="start">
        <Command filter={(itemValue, search) => workflowCommandScore(itemValue, search)}>
          <CommandInput placeholder={t('workflowEditor.searchWorkflows')} />
          <CommandList>
            <CommandEmpty>{t('workflowEditor.noWorkflowTemplates')}</CommandEmpty>
            <CommandGroup heading={t('workflowEditor.selectableWorkflows')}>
              {selectableOptions.map(({ template }) => {
                const workflowId = template.workflow.id;
                return (
                  <CommandItem key={workflowId} value={workflowTemplateSearchText(template, t)} onSelect={() => toggleWorkflow(workflowId)} className="items-start py-2">
                    <Check className={cn('mt-0.5 size-4', selected.has(workflowId) ? 'opacity-100' : 'opacity-0')} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">{workflowTemplateDisplayName(template, t)}</span>
                      <span className="mt-1 block truncate font-mono text-[11px] text-muted-foreground">{workflowId}</span>
                    </span>
                  </CommandItem>
                );
              })}
            </CommandGroup>
            {disabledOptions.length > 0 ? (
              <CommandGroup heading={t('workflowEditor.unselectableWorkflows')}>
                {disabledOptions.map(({ template, reason }, index) => {
                  const workflowId = template.workflow.id.trim();
                  return (
                    <CommandItem key={`${template.id}:${workflowId}:${index}`} value={workflowTemplateSearchText(template, t)} disabled className="items-start py-2 opacity-60">
                      <span className="mt-1 size-4 shrink-0 rounded-full border border-muted-foreground/40" />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-medium">{workflowTemplateDisplayName(template, t)}</span>
                        <span className="mt-1 block truncate font-mono text-[11px] text-muted-foreground">{workflowId || t('workflowEditor.emptyWorkflowId')}</span>
                        <span className="mt-1 block text-xs text-destructive">{reason}</span>
                      </span>
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            ) : null}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

function dynamicControlFields(t: (key: string) => string): Array<{ key: Exclude<keyof DynamicControlDsl, 'allowNestedDynamic'>; label: string; help: string }> {
  return [
    { key: 'maxDynamicNodes', label: t('workflowEditor.maxDynamicNodes'), help: t('workflowEditor.maxDynamicNodesHelp') },
    { key: 'maxFanout', label: t('workflowEditor.maxFanout'), help: t('workflowEditor.maxFanoutHelp') },
    { key: 'maxDepth', label: t('workflowEditor.maxDepth'), help: t('workflowEditor.maxDepthHelp') },
    { key: 'maxParallel', label: t('workflowEditor.maxParallel'), help: t('workflowEditor.maxParallelHelp') },
    { key: 'maxGroupDepth', label: t('workflowEditor.maxGroupDepth'), help: t('workflowEditor.maxGroupDepthHelp') },
    { key: 'maxWorkflowInvocations', label: t('workflowEditor.maxWorkflowInvocations'), help: t('workflowEditor.maxWorkflowInvocationsHelp') },
  ];
}

function workflowContainsAiDynamic(workflow: WorkflowDsl) {
  return workflow.nodes.some((item) => item.type === 'ai-dynamic');
}

function ProfileLabel({ t, onOpenProfileManagement }: { t: (key: string) => string; onOpenProfileManagement?: () => void }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span>{t('workflowEditor.profile')}</span>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button type="button" variant="ghost" size="icon-xs" className="rounded-full text-muted-foreground hover:text-foreground" onClick={(event) => event.preventDefault()} aria-label={t('workflowEditor.profileHelp')}>
              <CircleHelp className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent className="max-w-80 whitespace-pre-wrap break-words text-[12px] leading-relaxed" side="bottom" sideOffset={8}>{t('workflowEditor.profileHelp')}</TooltipContent>
        </Tooltip>
      </TooltipProvider>
      {onOpenProfileManagement ? <Button type="button" variant="link" size="xs" className="h-auto px-0" onClick={(event) => { event.preventDefault(); onOpenProfileManagement(); }}>{t('workflowEditor.manageProfiles')}</Button> : null}
    </span>
  );
}

function ProfilePicker({ profiles, value, invalid = false, onChange, t }: { profiles: ProfileVm[]; value: string | null; invalid?: boolean; onChange: (profile: string | null) => void; t: (key: string) => string }) {
  const [open, setOpen] = useState(false);
  const selected = profiles.find((profile) => profile.id === value) ?? null;

  const selectProfile = (profileId: string | null) => {
    onChange(profileId);
    setOpen(false);
  };

  return (
    <div className="flex items-center gap-1.5">
      <Popover open={open} onOpenChange={setOpen} modal>
        <PopoverTrigger asChild>
          <Button variant="outline" role="combobox" aria-expanded={open} className={cn('min-w-0 flex-1 justify-between px-3 font-normal', invalid && 'border-destructive text-destructive focus-visible:ring-destructive')}>
            <span className={cn('truncate', !selected && 'text-muted-foreground')}>{selected?.name ?? t('workflowEditor.selectProfile')}</span>
            <ChevronsUpDown className="size-4 opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-[var(--radix-popover-trigger-width)] p-0" align="start">
          <Command filter={(itemValue, search) => profileCommandScore(itemValue, search)}>
            <CommandInput placeholder={t('workflowEditor.selectProfile')} />
            <CommandList>
              <CommandEmpty>{t('workflowEditor.noProfiles')}</CommandEmpty>
              <CommandGroup>
                {value ? <CommandItem value="__clear_profile__" onSelect={() => selectProfile(null)}>{t('workflowEditor.clearProfile')}</CommandItem> : null}
                {profiles.map((profile) => (
                  <CommandItem key={`${profile.scope}:${profile.id}`} value={profileSearchText(profile)} onSelect={() => selectProfile(profile.id)} className="items-start py-2">
                    <Check className={cn('mt-0.5 size-4', value === profile.id ? 'opacity-100' : 'opacity-0')} />
                    <span className="min-w-0 flex-1">
                      <span className="flex items-center justify-between gap-2 font-medium"><span className="truncate">{profile.name}</span><span className="shrink-0 text-[11px] text-muted-foreground">{profileScopeText(t, profile.scope)}</span></span>
                      <span className="mt-1 block truncate font-mono text-[11px] text-muted-foreground">{profile.id}</span>
                      <TooltipProvider>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span className="mt-1 block truncate text-xs text-muted-foreground">{profile.summary}</span>
                          </TooltipTrigger>
                          <TooltipContent className="max-w-80 whitespace-pre-wrap break-words text-xs" sideOffset={6}>{profile.summary}</TooltipContent>
                        </Tooltip>
                      </TooltipProvider>
                      <span className="mt-1 block text-[11px] text-muted-foreground">{formatLocalDateTime(profile.createdAt)} / {formatLocalDateTime(profile.updatedAt)}</span>
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
      {selected ? <ProfileSummaryTooltip profile={selected} /> : null}
    </div>
  );
}

function ProfileSummaryTooltip({ profile }: { profile: ProfileVm }) {
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button type="button" variant="ghost" size="icon-sm" aria-label={profile.name}>
            <Info className="size-4" />
          </Button>
        </TooltipTrigger>
        <TooltipContent align="end" side="bottom" sideOffset={8} className="max-w-80 space-y-1 whitespace-pre-wrap break-words p-3 text-[12px] leading-relaxed">
          <p className="font-semibold text-foreground">{profile.name}</p>
          <p className="font-mono text-[11px] text-muted-foreground">{profile.id}</p>
          <p className="whitespace-pre-wrap break-words">{profile.summary}</p>
          <p className="text-muted-foreground">{formatLocalDateTime(profile.createdAt)} / {formatLocalDateTime(profile.updatedAt)}</p>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function profileScopeText(t: (key: string) => string, scope: ProfileVm['scope']) {
  switch (scope) {
    case 'built-in':
      return t('contextManagement.builtInScope');
    case 'user':
    default:
      return t('contextManagement.userScope');
  }
}

function profileSearchText(profile: ProfileVm) {
  return [profile.id, profile.name, profile.scope].join('\n').toLowerCase();
}

function profileCommandScore(itemValue: string, search: string) {
  const normalizedSearch = search.trim().toLowerCase();
  if (!normalizedSearch) return 1;
  return itemValue.toLowerCase().includes(normalizedSearch) ? 1 : 0;
}

function workflowTemplateSearchText(
  template: WorkflowTemplate,
  t: (key: string, options?: Record<string, unknown>) => string,
) {
  return [workflowTemplateDisplayName(template, t), template.name, template.workflow.id].join('\n').toLowerCase();
}

function workflowIdCountMap(templates: WorkflowTemplate[]) {
  const counts = new Map<string, number>();
  templates.forEach((template) => {
    const workflowId = template.workflow.id.trim();
    if (!workflowId) return;
    counts.set(workflowId, (counts.get(workflowId) ?? 0) + 1);
  });
  return counts;
}

function workflowDisabledReason(template: WorkflowTemplate, workflowIdCounts: Map<string, number>, allowNestedDynamic: boolean, t: (key: string, options?: Record<string, unknown>) => string) {
  const workflowId = template.workflow.id.trim();
  if (!workflowId) return t('workflowEditor.unselectableWorkflowEmptyId');
  if ((workflowIdCounts.get(workflowId) ?? 0) > 1) return t('workflowEditor.unselectableWorkflowDuplicateId', { workflow: workflowId });
  if (!allowNestedDynamic && workflowContainsAiDynamic(template.workflow)) return t('workflowEditor.unselectableWorkflowNestedDynamic');
  return null;
}

function workflowCommandScore(itemValue: string, search: string) {
  const normalizedSearch = search.trim().toLowerCase();
  if (!normalizedSearch) return 1;
  return itemValue.toLowerCase().includes(normalizedSearch) ? 1 : 0;
}

function InspectorCollapsible({ title, meta, children }: { title: string; meta?: ReactNode; children: ReactNode }) {
  return (
    <Collapsible defaultOpen className="overflow-hidden rounded-lg bg-muted/20">
      <div className="flex items-center gap-1">
        <CollapsibleTrigger className="flex min-w-0 flex-1 items-center justify-between gap-2 px-3 py-3 text-left hover:bg-muted/40 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset">
          <span className="flex min-w-0 items-center gap-2"><strong className="truncate text-sm">{title}</strong>{meta}</span>
          <ChevronDown className="size-4 shrink-0 transition-transform [[data-state=open]>&]:rotate-180" />
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent className="space-y-3 border-t border-border/50 p-3">{children}</CollapsibleContent>
    </Collapsible>
  );
}

function EdgeInspector({ edge, index, workflow, fieldErrors, onUpdate, t }: { edge: WorkflowEdgeDsl; index: number; workflow: WorkflowDsl; fieldErrors: Record<string, string[]>; onUpdate: (index: number, patch: Partial<WorkflowEdgeDsl>) => void; t: (key: string) => string }) {
  const errorsFor = (field: string) => fieldErrors[`edge:${index}:${field}`] ?? [];
  const targetOptions = edge.on === 'success' ? [END_NODE] : [END_NODE, NEW_ROUND_NODE];
  const newRoundEntryOptions = [ENTRY_NODE, ...workflow.nodes.map((node) => node.id)];
  const sourceSupportsFailureOutcome = nodeSupportsFailureOutcome(workflow.nodes.find((node) => node.id === edge.from));
  const outcomeOptions: EdgeOutcome[] = sourceSupportsFailureOutcome || edge.on === 'failure' ? ['success', 'failure'] : ['success'];
  return (
    <InspectorCollapsible title={t('workflowEditor.edgeConfig')}>
      <Field label={t('workflowEditor.edgeOutcome')} required errors={errorsFor('on')}>
        <Select value={edge.on} onValueChange={(on) => onUpdate(index, { on: on as EdgeOutcome })}>
          <SelectTrigger className={errorClass(errorsFor('on'))}><SelectValue /></SelectTrigger>
          <SelectContent>{outcomeOptions.map((value) => <SelectItem value={value} key={value} disabled={value === 'failure' && !sourceSupportsFailureOutcome}>{value}</SelectItem>)}</SelectContent>
        </Select>
      </Field>
      <Field label={t('workflowEditor.edgeTarget')} required errors={errorsFor('to')}>
        <Select value={edge.to} onValueChange={(to) => onUpdate(index, { to })}>
          <SelectTrigger className={errorClass(errorsFor('to'))}><SelectValue /></SelectTrigger>
          <SelectContent>
            {workflow.nodes.map((node) => <SelectItem value={node.id} key={node.id}>{node.id}</SelectItem>)}
            {targetOptions.map((target) => <SelectItem value={target} key={target}>{target}</SelectItem>)}
          </SelectContent>
        </Select>
      </Field>
      {edge.to === NEW_ROUND_NODE ? (
        <Field label={<HelpLabel label={t('workflowEditor.newRoundEntry')} help={t('workflowEditor.newRoundEntryHelp')} />} required errors={errorsFor('new_round_entry')}>
          <Select value={edge.new_round_entry?.trim() || undefined} onValueChange={(new_round_entry) => onUpdate(index, { new_round_entry })}>
            <SelectTrigger className={errorClass(errorsFor('new_round_entry'))}><SelectValue placeholder={t('workflowEditor.selectNewRoundEntry')} /></SelectTrigger>
            <SelectContent>
              {newRoundEntryOptions.map((target) => (
                <SelectItem value={target} key={target}>
                  {target === ENTRY_NODE ? `${ENTRY_NODE} · ${workflow.entry || '-'}` : target}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
      ) : null}
      <Field label={t('workflowEditor.sessionMode')} errors={errorsFor('session')}>
        <Select value={edge.session ?? 'new'} onValueChange={(session) => onUpdate(index, { session: session as SessionMode })}>
          <SelectTrigger className={errorClass(errorsFor('session'))}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="new">new</SelectItem>
            <SelectItem value="continue">continue</SelectItem>
          </SelectContent>
        </Select>
      </Field>
    </InspectorCollapsible>
  );
}

function Field({ label, children, errors = [], required = false }: { label: React.ReactNode; children: React.ReactNode; errors?: string[]; required?: boolean }) {
  return (
    <div className="grid gap-1.5 text-sm">
      <div className={cn('flex items-center gap-1.5 text-xs font-medium text-muted-foreground', errors.length > 0 && 'text-destructive')}>
        <span className="min-w-0">{label}</span>
        {required ? <span className="text-destructive">*</span> : null}
      </div>
      {children}
      {errors.map((error) => <span key={error} className="text-xs text-destructive">{error}</span>)}
    </div>
  );
}

function errorClass(errors: string[]) {
  return errors.length > 0 ? 'border-destructive focus-visible:ring-destructive' : undefined;
}

function HelpLabel({ label, help }: { label: string; help: string }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span>{label}</span>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              className="rounded-full text-muted-foreground hover:text-foreground"
              aria-label={help}
              onClick={(event) => event.preventDefault()}
            >
              <CircleHelp className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent
            className="max-w-80 whitespace-pre-wrap break-words text-[12px] leading-relaxed"
            side="top"
            sideOffset={10}
          >
            {help}
          </TooltipContent>
        </Tooltip>
      </TooltipProvider>
    </span>
  );
}

function WorkflowRoutedEdge({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition, markerEnd, style, label, data }: EdgeProps<Edge<WorkflowEdgeData>>) {
  const route = data?.route;
  const [smoothPath, smoothLabelX, smoothLabelY] = getSmoothStepPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition });
  const path = route?.path ?? smoothPath;
  const labelX = route?.labelX ?? smoothLabelX;
  const labelY = route?.labelY ?? smoothLabelY;
  return (
    <>
      <BaseEdge data-theme-role="workflow-edge" path={path} markerEnd={markerEnd} style={style} className="workflow-edge-flow" />
      {label ? (
        <EdgeLabelRenderer>
          <span
            data-theme-role="workflow-edge"
            className="workflow-edge-label pointer-events-none absolute z-20 rounded-full border bg-background px-2 py-0.5 text-[11px] font-semibold shadow-sm"
            style={{ color: style?.stroke, transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }}
          >
            {label}
          </span>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
}

export function deriveWorkflowEntryCandidateIds(workflow: Pick<WorkflowDsl, 'nodes' | 'edges'>): string[] {
  const nodeIds = new Set(workflow.nodes.map((node) => node.id).filter(Boolean));
  const nodeOrder = workflowSuccessTopologyOrder({ ...workflow, entry: '' });
  const incomingNodeIds = new Set<string>();
  workflow.edges.forEach((edge) => {
    if (!nodeIds.has(edge.from) || !nodeIds.has(edge.to)) return;
    if (edge.on !== 'success' && isBackwardEdge(edge.from, edge.to, nodeOrder)) return;
    incomingNodeIds.add(edge.to);
  });
  return workflow.nodes
    .map((node) => node.id)
    .filter((id) => Boolean(id) && !incomingNodeIds.has(id));
}

export function authoringWorkflowGraphSignature(workflow: Pick<WorkflowDsl, 'entry' | 'nodes' | 'edges'>): string {
  return JSON.stringify({
    entry: workflow.entry,
    nodes: workflow.nodes.map((node) => [
      node.id,
      node.type,
      'provider' in node ? node.provider ?? null : null,
    ]),
    edges: workflow.edges.map((edge) => [edge.from, edge.to, edge.on]),
  });
}

function stringSetSignature(values: Set<string>): string {
  return Array.from(values).sort().join('\u0000');
}

function normalizeWorkflowEntryFromTopology(workflow: WorkflowDsl): WorkflowDsl {
  const entryCandidateIds = deriveWorkflowEntryCandidateIds(workflow);
  const entry = entryCandidateIds.length === 1 ? entryCandidateIds[0] : '';
  return workflow.entry === entry ? workflow : { ...workflow, entry };
}

export type AuthoringGraphLayout = {
  items: Array<{ id: string; terminal: boolean }>;
  layoutPositions: Map<string, { x: number; y: number }>;
  branchRouteByEdgeIndex: Map<number, WorkflowGraphBranchRoute>;
  entryCandidateIds: Set<string>;
};

export function createAuthoringGraphLayout(workflow: WorkflowDsl, visibleTerminalIds: ReadonlySet<string> = new Set()): AuthoringGraphLayout {
  const collectedNodes = collectAuthoringNodes(workflow);
  const collectedIds = new Set(collectedNodes.map((node) => node.id));
  const items = [
    ...collectedNodes,
    ...Array.from(visibleTerminalIds).filter((id) => !collectedIds.has(id)).map((id) => ({ id, terminal: true })),
  ];
  const nodeIds = new Set(items.map((node) => node.id));
  const entryCandidateIds = new Set(deriveWorkflowEntryCandidateIds(workflow));
  const nodeOrder = workflowSuccessTopologyOrder(workflow);
  const nodeSpecs = items.map((node) => ({ id: node.id, width: node.terminal ? TERMINAL_NODE_WIDTH : NODE_WIDTH, height: node.terminal ? TERMINAL_NODE_HEIGHT : NODE_HEIGHT }));
  const layoutPositions = layoutSuccessPath(
    nodeSpecs,
    workflow.edges.map((e) => ({ from: e.from, to: e.to, on: e.on })),
    nodeIds,
    nodeOrder,
  );
  const nodeById = new Map(workflow.nodes.map((node) => [node.id, node]));
  const branchRouteByEdgeIndex = routeWorkflowBranchEdges(
    nodeSpecs,
    layoutPositions,
    workflow.edges.map((edge, index) => {
      const sourceNode = nodeById.get(edge.from);
      const sourceYOffset = nodeSupportsFailureOutcome(sourceNode)
        ? NODE_HEIGHT * (WORKFLOW_NODE_SPLIT_OUTCOME_RATIO[edge.on === 'failure' ? 'failure' : 'success'] - 0.5)
        : 0;
      return {
        index,
        sourceId: edge.from,
        targetId: edge.to,
        sourceYOffset,
        branch: edge.on !== 'success' || isBackwardEdge(edge.from, edge.to, nodeOrder),
      };
    }),
  );
  return { items, layoutPositions, branchRouteByEdgeIndex, entryCandidateIds };
}

export function authoringWorkflowTopologySignature(workflow: Pick<WorkflowDsl, 'entry' | 'nodes' | 'edges'>): string {
  return JSON.stringify({
    entry: workflow.entry,
    nodeIds: workflow.nodes.map((node) => node.id),
    edges: workflow.edges.map((edge) => [edge.from, edge.to, edge.on]),
  });
}

export function createAuthoringFlowProjection(
  workflow: WorkflowDsl,
  layout: AuthoringGraphLayout,
  selectedNodeId: string | null,
  selectedEdgeId: string | null,
  invalidNodeIds: ReadonlySet<string>,
  agentIconKeys: ReadonlyMap<string, string>,
  t: (key: string) => string,
  onQuickAdd: (nodeId: string, outcome: EdgeOutcome) => void = () => undefined,
  onDeleteNode: (nodeId: string) => void = () => undefined,
  selectedTerminalId: string | null = null,
): { nodes: Node<EditorNodeData>[]; edges: Edge[] } {
  const nodeById = new Map(workflow.nodes.map((node) => [node.id, node]));
  const nodes: Node<EditorNodeData>[] = layout.items.map((item) => {
    const pos = layout.layoutPositions.get(item.id) ?? { x: 0, y: 0 };
    const width = item.terminal ? TERMINAL_NODE_WIDTH : NODE_WIDTH;
    const height = item.terminal ? TERMINAL_NODE_HEIGHT : NODE_HEIGHT;
    const node = nodeById.get(item.id);
    const invalid = !item.terminal && invalidNodeIds.has(item.id);
    const provider = node && 'provider' in node ? node.provider : undefined;
    const iconKey = provider ? agentIconKeys.get(provider) : undefined;
    const supportsFailureOutcome = nodeSupportsFailureOutcome(node);
    const selected = item.terminal ? item.id === selectedTerminalId : item.id === selectedNodeId;
    return {
      id: item.id,
      type: 'editorCanvas',
      position: topLeft(pos.x, pos.y, width, height),
      sourcePosition: SOURCE_POS,
      targetPosition: TARGET_POS,
      data: {
        label: workflowNodeLabel(item.id, item.terminal, node?.type, t),
        kind: item.terminal ? 'terminal' : node?.type ?? 'node',
        terminal: item.terminal,
        iconKey,
        entryCandidate: !item.terminal && layout.entryCandidateIds.has(item.id),
        entryLabel: t('workflowEditor.entryBadge'),
        selected,
        supportsFailureOutcome,
        successLabel: t('workflowEditor.edgeLabels.success'),
        failureLabel: t('workflowEditor.edgeLabels.failure'),
        quickAddLabel: t('workflowEditor.quickAddSuccessor'),
        deleteLabel: t('workflowEditor.deleteNode'),
        onQuickAdd: item.terminal ? undefined : (outcome) => onQuickAdd(item.id, outcome),
        onDelete: item.terminal ? undefined : () => onDeleteNode(item.id),
      },
      className: cn(selected && 'workflow-node-selected', item.terminal && 'workflow-terminal-node', invalid && 'ring-1 ring-destructive'),
      selected,
      draggable: false,
      selectable: true,
      connectable: true,
      style: { width, height },
    };
  });

  const edges: Edge<WorkflowEdgeData>[] = workflow.edges.map((edge, index) => {
    const id = edgeId(edge, index);
    const branchRoute = layout.branchRouteByEdgeIndex.get(index);
    const color = authoringEdgeColor(edge.on);
    const sourceNode = nodeById.get(edge.from);
    const sourceHandle = edge.on === 'failure' && !nodeSupportsFailureOutcome(sourceNode) ? 'success' : edge.on;
    return {
      id,
      source: edge.from,
      target: edge.to,
      sourceHandle,
      label: workflowEdgeLabel(edge.on, t),
      type: 'workflowRouted',
      animated: false,
      markerEnd: { type: MarkerType.ArrowClosed, width: 16, height: 16, color },
      style: { stroke: color, strokeWidth: edge.on === 'success' ? 2.2 : 2, strokeDasharray: '3 17' },
      className: cn('workflow-edge-flow', (edge.on !== 'success' || branchRoute !== undefined) && 'workflow-edge-branch', id === selectedEdgeId && 'workflow-edge-selected'),
      selected: id === selectedEdgeId,
      data: { outcome: edge.on, route: branchRoute },
      zIndex: 0,
    };
  });

  return { nodes, edges };
}

function edgeColor(edge: WorkflowEdgeDsl) {
  return authoringEdgeColor(edge.on);
}

function workflowNodeLabel(id: string, terminal: boolean, nodeType: WorkflowNodeDsl['type'] | undefined, t: (key: string) => string) {
  if (id === END_NODE) return t('workflowEditor.nodeLabels.end');
  if (id === NEW_ROUND_NODE) return t('workflowEditor.nodeLabels.newRound');
  if (nodeType === 'ai-dynamic' && /^ai-dynamic(?:-\d+)?$/.test(id)) return t('workflowEditor.nodeLabels.aiDynamic');
  return id;
}

function workflowEdgeLabel(outcome: WorkflowEdgeDsl['on'], t: (key: string) => string) {
  if (outcome === 'success') return t('workflowEditor.edgeLabels.success');
  if (outcome === 'failure') return t('workflowEditor.edgeLabels.failure');
  return outcome;
}

function edgeId(edge: WorkflowEdgeDsl, index: number) {
  return `${edge.from}:${edge.to}:${edge.on}:${index}`;
}

export function parseWorkflowJson(json?: string | null): WorkflowDsl | null {
  if (!json) return null;
  try {
    const value = JSON.parse(json) as WorkflowDsl;
    return value?.version && Array.isArray(value.nodes) ? value : null;
  } catch {
    return null;
  }
}

function uniqueNodeId(workflow: WorkflowDsl, base: string) {
  let candidate = base;
  let index = 1;
  while (workflow.nodes.some((node) => node.id === candidate)) {
    index += 1;
    candidate = `${base}-${index}`;
  }
  return candidate;
}

function sanitizeNodeId(value: string, workflow: WorkflowDsl, currentId?: string) {
  const sanitized = value.trim().replace(/[\\/:*?"<>|\x00-\x1F\x7F]/g, '-');
  if (!sanitized) return currentId ?? uniqueNodeId(workflow, 'node');
  if (sanitized === currentId) return sanitized;
  return workflow.nodes.some((node) => node.id === sanitized) ? uniqueNodeId(workflow, sanitized) : sanitized;
}

function defaultValidationPatch(nodeId: string): Partial<WorkflowWorkerNodeDsl> {
  const artifact = `${nodeId}-result`;
  return {
    output: { kind: 'json', artifact, schema: null },
    success_condition: { expression: '' },
  };
}

function defaultDynamicControl(): DynamicControlDsl {
  return {
    maxDynamicNodes: 20,
    maxFanout: 5,
    maxDepth: 6,
    maxParallel: 3,
    maxGroupDepth: 1,
    maxWorkflowInvocations: 10,
    allowNestedDynamic: false,
  };
}

function conditionExpression(condition?: WorkflowJsonConditionDsl | null) {
  if (!condition) return '';
  if ('expression' in condition) return condition.expression;
  return `$.${condition.path} == ${JSON.stringify(condition.equals)}`;
}

function formatSchema(schema: unknown) {
  if (!schema) return '';
  try {
    return JSON.stringify(normalizeOutputSchema(schema), null, 2);
  } catch {
    return '';
  }
}

function normalizeWorkflowSchemas(workflow: WorkflowDsl): WorkflowDsl {
  const rawControl = workflow.control as WorkflowControlDsl & Record<string, unknown>;
  const control: WorkflowControlDsl = {};
  if (rawControl?.max_attempts != null) control.max_attempts = normalizeControlLimit(rawControl.max_attempts);
  if (rawControl?.max_rounds != null) control.max_rounds = normalizeControlLimit(rawControl.max_rounds);
  return {
    ...workflow,
    control,
    edges: normalizeWorkflowEdges(workflow.edges ?? []),
    nodes: workflow.nodes.map((node) => {
      if (node.type === 'ai-dynamic') {
        const rawNode = node as WorkflowAiDynamicNodeDsl & {
          provider?: string | null;
          profile?: string | null;
          goal?: string | null;
          agentStrategy?: WorkflowAiDynamicNodeDsl['agentStrategy'];
          allowedProfiles?: string[];
          globalGoal?: string | null;
        };
        const normalizedStrategy = rawNode.agentStrategy ?? {
          mode: 'fixed',
          provider: rawNode.provider ?? '',
        };
        const agentStrategy = normalizedStrategy.mode === 'fixed'
          ? {
            ...normalizedStrategy,
            model: normalizedStrategy.model?.trim() ? normalizedStrategy.model : undefined,
            permissionMode: normalizedStrategy.permissionMode?.trim() ? normalizedStrategy.permissionMode : undefined,
          }
          : {
            ...normalizedStrategy,
            bootstrapModel: normalizedStrategy.bootstrapModel?.trim() ? normalizedStrategy.bootstrapModel : undefined,
            permissionMode: normalizedStrategy.permissionMode?.trim() ? normalizedStrategy.permissionMode : undefined,
            acceptanceModel: normalizedStrategy.acceptanceModel?.trim() ? normalizedStrategy.acceptanceModel : undefined,
            routingPrompt: normalizedStrategy.routingPrompt ?? '',
            availableAgents: (normalizedStrategy.availableAgents ?? []).map((agent) => ({
              ...agent,
              model: agent.model?.trim() ? agent.model : undefined,
              permissionMode: agent.permissionMode?.trim() ? agent.permissionMode : undefined,
            })),
          };
        return {
          ...node,
          agentStrategy,
          allowedProfiles: node.allowedProfiles ?? rawNode.allowedProfiles ?? [],
          globalGoal: node.globalGoal ?? rawNode.globalGoal ?? null,
          control: { ...defaultDynamicControl(), ...((node.control ?? {}) as Partial<DynamicControlDsl>), allowNestedDynamic: false },
          allowedWorkflows: node.allowedWorkflows ?? [],
        };
      }
      const normalizedNode = { ...(node as WorkflowWorkerNodeDsl & { primary_artifact?: unknown }) };
      delete normalizedNode.primary_artifact;
      if (!normalizedNode.output?.schema) return normalizedNode;
      return {
        ...normalizedNode,
        output: {
          ...normalizedNode.output,
          schema: normalizeOutputSchema(normalizedNode.output.schema),
        },
      };
    }),
  };
}

function normalizeWorkflowEdges(edges: WorkflowEdgeDsl[]): WorkflowEdgeDsl[] {
  return edges.map((edge) => {
    const normalized = { ...edge };
    const newRoundEntry = normalized.new_round_entry?.trim();
    if (normalized.to === NEW_ROUND_NODE) {
      if (newRoundEntry) normalized.new_round_entry = newRoundEntry;
      else delete normalized.new_round_entry;
    } else {
      delete normalized.new_round_entry;
    }
    return normalized;
  });
}

function newRoundEntryDraftsFromWorkflow(workflow: WorkflowDsl): Record<number, string> {
  return workflow.edges.reduce<Record<number, string>>((drafts, edge, index) => {
    const newRoundEntry = edge.new_round_entry?.trim();
    if (newRoundEntry) drafts[index] = newRoundEntry;
    return drafts;
  }, {});
}

function shiftNewRoundEntryDraftsAfterDelete(drafts: Record<number, string>, deletedIndex: number): Record<number, string> {
  const next: Record<number, string> = {};
  Object.entries(drafts).forEach(([key, value]) => {
    const index = Number(key);
    if (!Number.isFinite(index) || index === deletedIndex) return;
    next[index > deletedIndex ? index - 1 : index] = value;
  });
  return next;
}

function normalizeControlLimit(value: unknown): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.trunc(parsed) : 0;
}

function normalizeOutputSchema(schema: unknown): unknown {
  const simple = jsonSchemaToSimpleShape(schema);
  return simple ?? schema;
}

function jsonSchemaToSimpleShape(schema: unknown): unknown | null {
  if (!isRecord(schema)) return null;
  if (schema.type === 'object' && isRecord(schema.properties)) {
    const shape: Record<string, unknown> = {};
    Object.entries(schema.properties).forEach(([key, value]) => {
      shape[key] = jsonSchemaToSimpleShape(value) ?? simpleTypeFromJsonSchema(value);
    });
    return shape;
  }
  if (schema.type === 'array') {
    const itemShape = jsonSchemaToSimpleShape(schema.items) ?? simpleTypeFromJsonSchema(schema.items);
    return itemShape ? [itemShape] : ['String'];
  }
  return simpleTypeFromJsonSchema(schema);
}

function simpleTypeFromJsonSchema(schema: unknown): string | null {
  if (!isRecord(schema) || typeof schema.type !== 'string') return null;
  if (schema.type === 'string') return 'String';
  if (schema.type === 'boolean') return 'boolean';
  if (schema.type === 'number') return 'number';
  if (schema.type === 'integer') return 'integer';
  if (schema.type === 'object') return 'object';
  if (schema.type === 'array') return 'array';
  if (schema.type === 'null') return 'null';
  return null;
}

function cloneWorkflow(workflow: WorkflowDsl): WorkflowDsl {
  return JSON.parse(JSON.stringify(workflow)) as WorkflowDsl;
}

type PathSegment = { type: 'key'; key: string } | { type: 'index'; index: number };

export function validateWorkflowForSave(
  workflow: WorkflowDsl,
  profiles: ProfileVm[],
  agents: ManagedAgentVm[],
  t: (key: string, options?: Record<string, unknown>) => string,
  workflowTemplates: WorkflowTemplateStore | null = null,
  currentTemplateId: string | null = null,
  currentTemplateName: string | null = null,
  validateTemplateDuplicateId = true,
): WorkflowValidationResult {
  const sanitizedWorkflow = normalizeWorkflowSchemas(cloneWorkflow(workflow));
  const issues: WorkflowValidationIssue[] = [];
  const fieldErrors: Record<string, string[]> = {};
  const profileIds = new Set(profiles.map((profile) => profile.id));
  const agentById = new Map(agents.map((agent) => [agent.agentType, agent]));
  const agentIds = new Set(agentById.keys());
  const templates = workflowTemplates?.templates ?? [];
  const workflowIdCounts = workflowIdCountMap(templates);
  const duplicateWorkflowTemplates = workflow.id.trim()
    ? templates.filter((template) => template.workflow.id.trim() === workflow.id.trim())
    : [];
  const duplicateConflictTemplates = duplicateWorkflowTemplates.filter((template) => template.id !== currentTemplateId);
  const nodeIds = new Set(workflow.nodes.map((node) => node.id).filter(Boolean));
  const nodeById = new Map(workflow.nodes.map((node) => [node.id, node]));
  const entryCandidateIds = deriveWorkflowEntryCandidateIds(sanitizedWorkflow);
  sanitizedWorkflow.entry = entryCandidateIds.length === 1 ? entryCandidateIds[0] : '';
  const outgoingEdgeCounts = workflow.edges.reduce<Record<string, number>>((counts, edge) => {
    if (edge.from.trim()) {
      counts[edge.from] = (counts[edge.from] ?? 0) + 1;
    }
    return counts;
  }, {});
  const edgeOutcomeCounts = workflow.edges.reduce<Record<string, number>>((counts, edge) => {
    if (edge.from.trim() && ['success', 'failure'].includes(edge.on)) {
      const key = `${edge.from}\0${edge.on}`;
      counts[key] = (counts[key] ?? 0) + 1;
    }
    return counts;
  }, {});
  const reportedDuplicateEdgeOutcomes = new Set<string>();
  const nodeIdCounts = workflow.nodes.reduce<Record<string, number>>((counts, node) => {
    counts[node.id] = (counts[node.id] ?? 0) + 1;
    return counts;
  }, {});

  const addIssue = (message: string, fieldKey?: string, nodeId?: string, edgeIndex?: number, nodeIds?: string[]) => {
    issues.push({ message, fieldKey, nodeId, edgeIndex, nodeIds });
    if (fieldKey) fieldErrors[fieldKey] = [...(fieldErrors[fieldKey] ?? []), message];
  };
  const nodeField = (node: WorkflowNodeDsl, field: string) => `node:${node.id}:${field}`;
  const edgeField = (index: number, field: string) => `edge:${index}:${field}`;
  const controlField = (field: string) => `control:${field}`;
  if (!workflow.id.trim()) addIssue(t('workflowEditor.validationWorkflowIdRequired'));
  else if (validateTemplateDuplicateId && duplicateConflictTemplates.length > 0) {
    addIssue(
      t('errors.workflow.duplicate-id', {
        workflowName: currentTemplateName ?? duplicateWorkflowTemplates.find((template) => template.id === currentTemplateId)?.name ?? workflow.id.trim(),
        workflowId: workflow.id.trim(),
        conflicts: duplicateConflictTemplates.map((template) => template.name).join('、'),
      }),
    );
  }
  if (!workflow.nodes.length) addIssue(t('workflowEditor.validationNodesRequired'));
  else if (entryCandidateIds.length === 0) {
    addIssue(t('workflowEditor.validationEntryCandidateMissing'));
  } else if (entryCandidateIds.length > 1) {
    addIssue(t('workflowEditor.validationEntryCandidateMultiple', { entries: entryCandidateIds.join(', ') }), undefined, undefined, undefined, entryCandidateIds);
  }
  if (!workflow.edges.some((edge) => edge.to === END_NODE)) addIssue(t('workflowEditor.validationEndNodeRequired'));
  if (sanitizedWorkflow.control.max_attempts != null && sanitizedWorkflow.control.max_attempts <= 0) {
    addIssue(t('workflowEditor.validationMaxAttemptsPositive'), controlField('max_attempts'));
  }
  if (sanitizedWorkflow.control.max_rounds != null && sanitizedWorkflow.control.max_rounds <= 0) {
    addIssue(t('workflowEditor.validationMaxRoundsPositive'), controlField('max_rounds'));
  }

  workflow.nodes.forEach((node, nodeIndex) => {
    const nodeLabel = node.id || t('workflowEditor.unnamedNode');
    if (!node.id.trim()) addIssue(t('workflowEditor.validationNodeIdRequired', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    if ([END_NODE, ENTRY_NODE, NEW_ROUND_NODE].includes(node.id)) addIssue(t('workflowEditor.validationReservedNodeId', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    if ((nodeIdCounts[node.id] ?? 0) > 1) addIssue(t('workflowEditor.validationDuplicateNodeId', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    if ((outgoingEdgeCounts[node.id] ?? 0) === 0) {
      addIssue(t('workflowEditor.validationDanglingNode', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    }

    if (node.type === 'ai-dynamic') {
      validateAiDynamicNodeForSave(node, nodeLabel, workflowTemplates, profiles, agentIds, agentById, nodeField, addIssue, t);
      return;
    }
    if (!node.provider?.trim()) addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'provider'), node.id);
    else if (!agentIds.has(node.provider)) addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'provider'), node.id);
    else if (node.permission_mode?.trim()) {
      const supportedModeIds = new Set((agentById.get(node.provider)?.supportedModes ?? []).map((mode) => mode.id));
      if (supportedModeIds.size > 0 && !supportedModeIds.has(node.permission_mode)) {
        addIssue(t('workflowEditor.validationPermissionModeUnavailable', { node: nodeLabel }), nodeField(node, 'permission_mode'), node.id);
      }
    }

    const workerNode = node as WorkflowWorkerNodeDsl;
    if (!workerNode.profile?.trim()) {
      addIssue(t('workflowEditor.validationNodeProfileRequired', { node: nodeLabel }), nodeField(workerNode, 'profile'), workerNode.id);
    } else if (!profileIds.has(workerNode.profile)) {
      addIssue(t('workflowEditor.validationNodeProfileVisibilityChanged', { node: nodeLabel }), nodeField(workerNode, 'profile'), workerNode.id);
      const sanitized = sanitizedWorkflow.nodes[nodeIndex];
      if (sanitized && sanitized.type === 'worker') sanitized.profile = null;
    }
    const validationEnabled = Boolean(workerNode.output || workerNode.success_condition);
    if (validationEnabled && workerNode.manual_check) {
      addIssue(t('workflowEditor.validationResultModeExclusive', { node: nodeLabel }), nodeField(workerNode, 'success_condition'), workerNode.id);
    }
    if (validationEnabled) {
      if (!workerNode.output?.artifact?.trim()) addIssue(t('workflowEditor.validationOutputArtifactRequired', { node: nodeLabel }), nodeField(workerNode, 'output.artifact'), workerNode.id);
      if (!workerNode.success_condition) addIssue(t('workflowEditor.validationSuccessExpressionRequired', { node: nodeLabel }), nodeField(workerNode, 'success_condition'), workerNode.id);
      let path: PathSegment[] | null = null;
      if (workerNode.success_condition) {
        try {
          path = successConditionPath(workerNode.success_condition);
        } catch {
          addIssue(t('workflowEditor.saveErrorInvalidExpression', { node: nodeLabel }), nodeField(workerNode, 'success_condition'), workerNode.id);
        }
      }
      const schema = workerNode.output?.schema;
      if (schema && looksLikeJsonSchema(schema)) {
        addIssue(t('workflowEditor.saveErrorLegacySchema', { node: nodeLabel }), nodeField(node, 'output.schema'), node.id);
      }
      if (schema && path && !looksLikeJsonSchema(schema) && !schemaContainsPath(schema, path)) {
        addIssue(t('workflowEditor.saveErrorMissingPath', { node: nodeLabel }), nodeField(node, 'output.schema'), node.id);
      }
    }
  });

  workflow.edges.forEach((edge, index) => {
    if (!edge.from.trim()) addIssue(t('workflowEditor.validationEdgeSourceRequired', { index: index + 1 }), edgeField(index, 'from'), undefined, index);
    else if (!nodeIds.has(edge.from)) addIssue(t('workflowEditor.validationEdgeSourceMissing', { node: edge.from }), edgeField(index, 'from'), edge.from, index);
    if (!edge.to.trim()) addIssue(t('workflowEditor.validationEdgeTargetRequired', { index: index + 1 }), edgeField(index, 'to'), undefined, index);
    else if (![END_NODE, NEW_ROUND_NODE].includes(edge.to) && !nodeIds.has(edge.to)) addIssue(t('workflowEditor.validationEdgeTargetMissing', { node: edge.to }), edgeField(index, 'to'), edge.to, index);
    if (!['success', 'failure'].includes(edge.on)) addIssue(t('workflowEditor.validationEdgeOutcomeRequired', { index: index + 1 }), edgeField(index, 'on'), undefined, index);
    else if (edge.on === 'failure' && !nodeSupportsFailureOutcome(nodeById.get(edge.from))) {
      addIssue(t('workflowEditor.validationFailureOutcomeRequiresOutputValidation', { node: edge.from }), edgeField(index, 'on'), edge.from, index);
    }
    else if (edge.on === 'success' && edge.to === NEW_ROUND_NODE) {
      addIssue(t('workflowEditor.validationSuccessNewRoundTarget', { node: edge.from }), edgeField(index, 'to'), edge.from, index);
    } else if (edge.from.trim()) {
      const edgeOutcomeKey = `${edge.from}\0${edge.on}`;
      const edgeOutcomeCount = edgeOutcomeCounts[edgeOutcomeKey] ?? 0;
      if (edgeOutcomeCount > 1 && !reportedDuplicateEdgeOutcomes.has(edgeOutcomeKey)) {
        addIssue(t('workflowEditor.validationDuplicateEdgeOutcome', { node: edge.from, outcome: edge.on, num: edgeOutcomeCount }), edgeField(index, 'on'), edge.from, index);
        reportedDuplicateEdgeOutcomes.add(edgeOutcomeKey);
      }
    }
    if (edge.to === NEW_ROUND_NODE) {
      const newRoundEntry = edge.new_round_entry?.trim();
      if (!newRoundEntry) {
        addIssue(t('workflowEditor.validationNewRoundEntryRequired', { node: edge.from }), edgeField(index, 'new_round_entry'), edge.from, index);
      } else if (newRoundEntry !== ENTRY_NODE && !nodeIds.has(newRoundEntry)) {
        addIssue(t('workflowEditor.validationNewRoundEntryMissing', { node: edge.from, entry: newRoundEntry }), edgeField(index, 'new_round_entry'), edge.from, index);
      }
    }
    if ([END_NODE, NEW_ROUND_NODE].includes(edge.from)) addIssue(t('workflowEditor.validationTerminalEdgeSource', { node: edge.from }), edgeField(index, 'from'), undefined, index);
    if (edge.session === 'continue' && [END_NODE, NEW_ROUND_NODE].includes(edge.to)) addIssue(t('workflowEditor.validationContinueTerminalTarget', { index: index + 1 }), edgeField(index, 'session'), undefined, index);
  });

  return { valid: issues.length === 0, issues, fieldErrors, sanitizedWorkflow };
}

function validateAiDynamicNodeForSave(
  node: WorkflowAiDynamicNodeDsl,
  nodeLabel: string,
  workflowTemplates: WorkflowTemplateStore | null | undefined,
  profiles: ProfileVm[],
  agentIds: Set<string>,
  agentById: Map<string, ManagedAgentVm>,
  nodeField: (node: WorkflowNodeDsl, field: string) => string,
  addIssue: (message: string, fieldKey?: string, nodeId?: string, edgeIndex?: number) => void,
  t: (key: string, options?: Record<string, unknown>) => string,
) {
  const control = { ...defaultDynamicControl(), ...(node.control ?? {}) };
  const validatePermissionMode = (provider: string | undefined, permissionMode: string | null | undefined, field: string) => {
    const mode = permissionMode?.trim();
    if (!provider || !mode) return;
    const supportedModeIds = new Set((agentById.get(provider)?.supportedModes ?? []).map((option) => option.id));
    if (supportedModeIds.size > 0 && !supportedModeIds.has(mode)) {
      addIssue(t('workflowEditor.validationPermissionModeUnavailable', { node: nodeLabel }), nodeField(node, field), node.id);
    }
  };
  if (node.agentStrategy.mode === 'fixed') {
    const provider = node.agentStrategy.provider?.trim();
    if (!provider) {
      addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.provider'), node.id);
    } else if (!agentIds.has(provider)) {
      addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'agentStrategy.provider'), node.id);
    }
    validatePermissionMode(provider, node.agentStrategy.permissionMode, 'agentStrategy.permissionMode');
  } else {
    const bootstrapProvider = node.agentStrategy.bootstrapProvider?.trim();
    if (!bootstrapProvider) {
      addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.bootstrapProvider'), node.id);
    } else if (!agentIds.has(bootstrapProvider)) {
      addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'agentStrategy.bootstrapProvider'), node.id);
    }
    validatePermissionMode(bootstrapProvider, node.agentStrategy.permissionMode, 'agentStrategy.permissionMode');
    if ((node.agentStrategy.availableAgents ?? []).length === 0) {
      addIssue(t('workflowEditor.validationDynamicAvailableAgentsRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.availableAgents'), node.id);
    }
    const seenDynamicAgents = new Set<string>();
    (node.agentStrategy.availableAgents ?? []).forEach((agentRef, index) => {
      const provider = agentRef.provider?.trim();
      if (!provider) {
        addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, `agentStrategy.availableAgents.${index}.provider`), node.id);
        return;
      }
      if (seenDynamicAgents.has(provider)) {
        addIssue(t('workflowEditor.validationDynamicAgentDuplicated', { node: nodeLabel, agent: provider }), nodeField(node, 'agentStrategy.availableAgents'), node.id);
        return;
      }
      seenDynamicAgents.add(provider);
      if (!agentIds.has(provider)) {
        addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, `agentStrategy.availableAgents.${index}.provider`), node.id);
      }
      validatePermissionMode(provider, agentRef.permissionMode, `agentStrategy.availableAgents.${index}.permissionMode`);
    });
  }
  const knownProfileIds = new Set(profiles.map((profile) => profile.id));
  const seenProfiles = new Set<string>();
  (node.allowedProfiles ?? []).forEach((profileId) => {
    const value = profileId?.trim();
    if (!value) {
      addIssue(t('workflowEditor.validationAllowedProfileRequired', { node: nodeLabel }), nodeField(node, 'allowedProfiles'), node.id);
      return;
    }
    if (seenProfiles.has(value)) {
      addIssue(t('workflowEditor.validationAllowedProfileDuplicated', { node: nodeLabel, profile: value }), nodeField(node, 'allowedProfiles'), node.id);
      return;
    }
    seenProfiles.add(value);
    if (!knownProfileIds.has(value)) {
      addIssue(t('workflowEditor.validationAllowedProfileMissing', { node: nodeLabel, profile: value }), nodeField(node, 'allowedProfiles'), node.id);
    }
  });
  if (node.globalGoal !== undefined && node.globalGoal !== null && !node.globalGoal.trim()) {
    addIssue(t('workflowEditor.validationGlobalGoalBlank', { node: nodeLabel }), nodeField(node, 'globalGoal'), node.id);
  }
  dynamicControlFields(t).forEach((field) => {
    if ((control[field.key] ?? 0) <= 0) {
      addIssue(t('workflowEditor.validationDynamicLimitPositive', { node: nodeLabel, field: field.label }), nodeField(node, `control.${field.key}`), node.id);
    }
  });
  const templates = workflowTemplates?.templates ?? [];
  const workflowIdCounts = workflowIdCountMap(templates);
  const templateById = new Map(
    templates
      .filter((template) => workflowIdCounts.get(template.workflow.id.trim()) === 1)
      .map((template) => [template.workflow.id.trim(), template] as const),
  );
  const seen = new Set<string>();
  (node.allowedWorkflows ?? []).forEach((allowed) => {
    const workflowId = allowed.workflowId?.trim();
    if (!workflowId) {
      addIssue(t('workflowEditor.validationAllowedWorkflowRequired', { node: nodeLabel }), nodeField(node, 'allowedWorkflows'), node.id);
      return;
    }
    if (seen.has(workflowId)) {
      addIssue(t('workflowEditor.validationAllowedWorkflowDuplicated', { node: nodeLabel, workflow: workflowId }), nodeField(node, 'allowedWorkflows'), node.id);
      return;
    }
    seen.add(workflowId);
    const template = templateById.get(workflowId);
    if (!template) {
      const duplicated = (workflowIdCounts.get(workflowId) ?? 0) > 1;
      addIssue(t(duplicated ? 'workflowEditor.validationAllowedWorkflowIdNotUnique' : 'workflowEditor.validationAllowedWorkflowMissing', { node: nodeLabel, workflow: workflowId }), nodeField(node, 'allowedWorkflows'), node.id);
      return;
    }
    if (!control.allowNestedDynamic && workflowContainsAiDynamic(template.workflow)) {
      addIssue(t('workflowEditor.validationAllowedWorkflowNestedDynamic', { node: nodeLabel, workflow: workflowId }), nodeField(node, 'allowedWorkflows'), node.id);
    }
  });
}

function successConditionPath(condition: WorkflowJsonConditionDsl) {
  if ('expression' in condition) return parseExpressionPath(condition.expression ?? '');
  return parseJsonPath(condition.path ?? '');
}

function parseExpressionPath(expression: string) {
  const operators = ['>=', '<=', '!=', '==', '>', '<'];
  const operator = operators.find((item) => expression.includes(item));
  if (!operator) throw new Error('unsupported expression');
  const [left] = expression.split(operator);
  if (!left.trim().startsWith('$')) throw new Error('left side must start with $');
  return parseJsonPath(left.trim());
}

function parseJsonPath(path: string): PathSegment[] {
  let value = path.trim();
  if (value.startsWith('$.')) value = value.slice(2);
  else if (value === '$') throw new Error('root path is not supported');
  else if (value.startsWith('$')) value = value.slice(1);
  if (!value) throw new Error('empty path');

  const segments: PathSegment[] = [];
  let key = '';
  for (let index = 0; index < value.length;) {
    const char = value[index];
    if (char === '.') {
      if (!key) {
        if (segments.at(-1)?.type !== 'index') throw new Error('empty segment');
      } else {
        segments.push({ type: 'key', key });
        key = '';
      }
      index += 1;
      continue;
    }
    if (char === '[') {
      if (key) {
        segments.push({ type: 'key', key });
        key = '';
      }
      const closeIndex = value.indexOf(']', index + 1);
      if (closeIndex < 0) throw new Error('unclosed index');
      const rawIndex = value.slice(index + 1, closeIndex);
      if (!/^\d+$/.test(rawIndex)) throw new Error('invalid index');
      segments.push({ type: 'index', index: Number(rawIndex) });
      index = closeIndex + 1;
      if (index < value.length && value[index] !== '.' && value[index] !== '[') throw new Error('invalid separator');
      continue;
    }
    key += char;
    index += 1;
  }
  if (key) segments.push({ type: 'key', key });
  if (!segments.length) throw new Error('empty path');
  return segments;
}

function looksLikeJsonSchema(schema: unknown) {
  if (!isRecord(schema)) return false;
  return ['type', 'properties', 'required', 'additionalProperties', 'items'].some((key) => key in schema);
}

function schemaContainsPath(schema: unknown, path: PathSegment[]) {
  let cursor = schema;
  for (const segment of path) {
    if (segment.type === 'key') {
      if (!isRecord(cursor) || !(segment.key in cursor)) return false;
      cursor = cursor[segment.key];
      continue;
    }
    if (!Array.isArray(cursor)) return false;
    cursor = cursor[segment.index] ?? cursor[0];
    if (cursor === undefined) return false;
  }
  return true;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
