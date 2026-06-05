import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Check, ChevronDown, ChevronsUpDown, CircleHelp, Info, Plus, Sparkles, Trash2, X } from 'lucide-react';
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
  type Connection,
  type Edge,
  type EdgeProps,
  type Node,
  type ReactFlowInstance,
} from '@xyflow/react';
import { useTranslation } from 'react-i18next';
import type { AgentRegistryVm, DynamicControlDsl, ManagedAgentVm, ProfileVm, WorkflowAiDynamicDynamicAgentStrategyDsl, WorkflowAiDynamicFixedAgentStrategyDsl, WorkflowAiDynamicNodeDsl, WorkflowControlDsl, WorkflowDsl, WorkflowEdgeDsl, WorkflowJsonConditionDsl, WorkflowNodeDsl, WorkflowOutputContractDsl, WorkflowTemplate, WorkflowTemplateStore, WorkflowWorkerNodeDsl } from '../types';
import {
  END_NODE,
  NEW_ROUND_NODE,
  NODE_WIDTH,
  NODE_HEIGHT,
  TERMINAL_NODE_WIDTH,
  TERMINAL_NODE_HEIGHT,
  collectAuthoringNodes,
  workflowNodeOrder,
  computeBackwardLanes,
  authoringEdgeColor,
  layoutSuccessPath,
  topLeft,
  SOURCE_POS,
  TARGET_POS,
} from './workflowGraph';
import { AppCard } from '@/components/AppCard';
import { CodeBlock, EmptyState } from '@/components/PageScaffold';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { displayAppError } from '../i18n';
import { cn } from '@/lib/utils';
import { formatLocalDateTime } from '@/lib/datetime';

function providerToIconKey(provider: string): string | undefined {
  const mapping: Record<string, string> = { 'claude-acp': 'claude', 'codex-acp': 'codex', cursor: 'cursor', gemini: 'gemini', opencode: 'opencode' };
  return mapping[provider];
}

function workflowIconClass(iconKey: string) {
  const scale: Record<string, string> = {
    codex: 'scale-125',
    gemini: 'scale-110',
    opencode: 'scale-110',
  };
  return cn('size-4 object-contain', scale[iconKey]);
}

const DEFAULT_PERMISSION_MODE = '__default_permission_mode__';

type EditorTab = 'canvas' | 'json';
type EdgeOutcome = 'success' | 'failure';
type SessionMode = 'new' | 'continue';
type EditorNodeData = { label: string; kind: string; detail: string; terminal?: boolean; iconKey?: string };
type WorkflowEdgeData = { outcome: WorkflowEdgeDsl['on']; lane?: number };
export type WorkflowValidationIssue = { message: string; fieldKey?: string; nodeId?: string; edgeIndex?: number };
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

function EditorCanvasNode({ data }: { data: EditorNodeData }) {
  if (data.terminal) {
    return (
      <div className="flex size-full items-center justify-center rounded-full border border-dashed border-border/80 bg-muted/20 text-xs tracking-wide text-muted-foreground">
        <Handle type="target" position={Position.Left} className="!size-2 !border-2 !border-card !bg-muted-foreground" />
        {data.label}
      </div>
    );
  }
  return (
    <div className="flex size-full flex-col items-center justify-center gap-1 rounded-[14px] border border-border bg-card px-3 py-2">
      <Handle type="target" position={Position.Left} className="!size-2 !border-2 !border-card !bg-muted-foreground" />
      <Handle type="source" position={Position.Right} className="!size-2 !border-2 !border-card !bg-muted-foreground" />
      <div className="flex items-center gap-1.5">
        {data.iconKey ? (
          <span className="grid size-5 shrink-0 place-items-center rounded-md border border-border/60 bg-muted/30 shadow-sm">
            <img src={`/agent-icons/${data.iconKey}.svg`} alt="" className={workflowIconClass(data.iconKey)} />
          </span>
        ) : null}
        <span className="text-[13px] font-medium text-foreground">{data.label}</span>
      </div>
      <span className="truncate font-mono text-[10px] uppercase tracking-[0.14em] text-muted-foreground">{data.kind}</span>
    </div>
  );
}

interface WorkflowEditorProps {
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
}

export function WorkflowEditor({ value, agentRegistry, profiles = [], onOpenProfileManagement, onSave, onChange, onApplyDefaultTemplate, defaultWorkflow, workflowTemplates, currentTemplateId = null, currentTemplateName = null, validateTemplateDuplicateId = true, allowAiDynamic = false, saving, showSaveAction = true, validationRequestId = 0 }: WorkflowEditorProps) {
  const { t } = useTranslation();
  const initialWorkflow = useMemo(() => normalizeWorkflowSchemas(value), [value]);
  const [workflow, setWorkflow] = useState<WorkflowDsl>(initialWorkflow);
  const [tab, setTab] = useState<EditorTab>('canvas');
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(initialWorkflow.nodes[0]?.id ?? null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [flowInstance, setFlowInstance] = useState<ReactFlowInstance<Node<EditorNodeData>, Edge> | null>(null);
  const [pendingFocusNodeId, setPendingFocusNodeId] = useState<string | null>(null);
  const [visibleTerminalIds, setVisibleTerminalIds] = useState<Set<string>>(new Set());
  const [terminalMenu, setTerminalMenu] = useState<TerminalMenu | null>(null);
  const [validationDialogOpen, setValidationDialogOpen] = useState(false);
  const [pendingValidation, setPendingValidation] = useState<WorkflowValidationResult | null>(null);
  const [fieldErrors, setFieldErrors] = useState<Record<string, string[]>>({});
  const [invalidNodeIds, setInvalidNodeIds] = useState<Set<string>>(new Set());
  const [jsonDraft, setJsonDraft] = useState(() => JSON.stringify(initialWorkflow, null, 2));
  const [jsonError, setJsonError] = useState<string | null>(null);
  const handledValidationRequestIdRef = useRef(0);
  const agents = useMemo(() => agentRegistry?.agents.filter((agent) => agent.supported && agent.diagnostic?.available === true) ?? [], [agentRegistry]);
  const selectedNode = selectedNodeId ? workflow.nodes.find((node) => node.id === selectedNodeId) ?? null : null;
  const selectedEdgeIndex = selectedEdgeId ? Number(selectedEdgeId.split(':').at(-1)) : -1;
  const selectedEdge = selectedEdgeIndex >= 0 ? workflow.edges[selectedEdgeIndex] ?? null : null;
  const canSave = workflow.nodes.length > 0 && workflow.entry.trim() !== '' && agents.length > 0;
  const { nodes, edges } = useMemo(() => workflowToFlow(workflow, selectedNodeId, selectedEdgeId, invalidNodeIds, visibleTerminalIds, t), [invalidNodeIds, selectedEdgeId, selectedNodeId, t, visibleTerminalIds, workflow]);

  useEffect(() => {
    if (JSON.stringify(workflow) === JSON.stringify(initialWorkflow)) return;
    setWorkflow(initialWorkflow);
    setJsonDraft(JSON.stringify(initialWorkflow, null, 2));
    setJsonError(null);
    setSelectedNodeId(initialWorkflow.nodes[0]?.id ?? null);
    setSelectedEdgeId(null);
    setVisibleTerminalIds(new Set());
    setTerminalMenu(null);
  }, [initialWorkflow]);

  useEffect(() => {
    if (validationRequestId <= 0 || handledValidationRequestIdRef.current === validationRequestId) return;
    handledValidationRequestIdRef.current = validationRequestId;
    const validation = validateWorkflowForSave(workflow, profiles, agents, t, workflowTemplates ?? null, currentTemplateId, currentTemplateName, validateTemplateDuplicateId);
    if (validation.valid) return;
    setPendingValidation(validation);
    setValidationDialogOpen(true);
  }, [agents, profiles, t, validationRequestId, workflow, workflowTemplates]);

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

  const syncWorkflow = (next: WorkflowDsl) => {
    setFieldErrors({});
    setInvalidNodeIds(new Set());
    setJsonError(null);
    setWorkflow(next);
    setJsonDraft(JSON.stringify(next, null, 2));
    onChange?.(next);
  };

  const closeValidationDialog = (open: boolean) => {
    setValidationDialogOpen(open);
    if (open || !pendingValidation) return;
    setFieldErrors(pendingValidation.fieldErrors);
    setInvalidNodeIds(new Set(pendingValidation.issues.map((issue) => issue.nodeId).filter(Boolean) as string[]));
    setWorkflow(pendingValidation.sanitizedWorkflow);
    setJsonDraft(JSON.stringify(pendingValidation.sanitizedWorkflow, null, 2));
    onChange?.(pendingValidation.sanitizedWorkflow);
    const firstIssue = pendingValidation.issues.find((issue) => issue.nodeId || issue.edgeIndex !== undefined);
    if (firstIssue?.nodeId) {
      setSelectedNodeId(firstIssue.nodeId);
      setSelectedEdgeId(null);
      setPendingFocusNodeId(firstIssue.nodeId);
    } else if (firstIssue?.edgeIndex !== undefined) {
      const edge = pendingValidation.sanitizedWorkflow.edges[firstIssue.edgeIndex];
      if (edge) {
        setSelectedNodeId(null);
        setSelectedEdgeId(edgeId(edge, firstIssue.edgeIndex));
      }
    }
    setPendingValidation(null);
  };

  const handleConnect = (connection: Connection) => {
    if (!connection.source || !connection.target) return;
    if (connection.source === END_NODE || connection.source === NEW_ROUND_NODE) return;
    const edge: WorkflowEdgeDsl = {
      from: connection.source,
      to: connection.target,
      on: connection.target === NEW_ROUND_NODE ? 'failure' : 'success',
    };
    const next = { ...workflow, edges: [...workflow.edges, edge] };
    syncWorkflow(next);
    setSelectedEdgeId(edgeId(edge, next.edges.length - 1));
    setSelectedNodeId(null);
    setTerminalMenu(null);
  };

  const showTerminalTarget = (terminalId: string) => {
    setVisibleTerminalIds((current) => new Set(current).add(terminalId));
    setTerminalMenu(null);
  };

  const applyDefaultTemplate = () => {
    if (!defaultWorkflow) return;
    const next = normalizeWorkflowSchemas(cloneWorkflow(defaultWorkflow));
    syncWorkflow(next);
    onApplyDefaultTemplate?.(next);
    setSelectedNodeId(next.nodes[0]?.id ?? null);
    setSelectedEdgeId(null);
  };

  const handleSave = async () => {
    let workflowToSave = workflow;
    if (tab === 'json') {
      const parsed = parseWorkflowJson(jsonDraft);
      if (!parsed) {
        setJsonError(t('workflowEditor.outputSchemaInvalid'));
        return;
      }
      workflowToSave = normalizeWorkflowSchemas(parsed);
      setWorkflow(workflowToSave);
      onChange?.(workflowToSave);
    }
    const validation = validateWorkflowForSave(workflowToSave, profiles, agents, t, workflowTemplates ?? null, currentTemplateId, currentTemplateName, validateTemplateDuplicateId);
    if (!validation.valid) {
      setPendingValidation(validation);
      setValidationDialogOpen(true);
      return;
    }
    setFieldErrors({});
    setInvalidNodeIds(new Set());
    try {
      await onSave(validation.sanitizedWorkflow);
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
    setPendingFocusNodeId(id);
  };

  const deleteSelectedNode = () => {
    if (!selectedNodeId) return;
    const nodes = workflow.nodes.filter((node) => node.id !== selectedNodeId);
    const next = {
      ...workflow,
      entry: workflow.entry === selectedNodeId ? nodes[0]?.id ?? '' : workflow.entry,
      nodes,
      edges: workflow.edges.filter((edge) => edge.from !== selectedNodeId && edge.to !== selectedNodeId),
    };
    syncWorkflow(next);
    setSelectedNodeId(next.nodes[0]?.id ?? null);
  };

  const updateNode = (nodeId: string, patch: Partial<WorkflowNodeDsl>) => {
    const nextId = patch.id && patch.id !== nodeId ? sanitizeNodeId(patch.id, workflow, nodeId) : null;
    const next = {
      ...workflow,
      entry: nextId && workflow.entry === nodeId ? nextId : workflow.entry,
      nodes: workflow.nodes.map((node) => node.id === nodeId ? { ...node, ...patch, id: nextId ?? node.id } as WorkflowNodeDsl : node),
      edges: nextId ? workflow.edges.map((edge) => ({ ...edge, from: edge.from === nodeId ? nextId : edge.from, to: edge.to === nodeId ? nextId : edge.to })) : workflow.edges,
    };
    syncWorkflow(next);
    if (nextId) setSelectedNodeId(nextId);
  };

  const updateEdge = (index: number, patch: Partial<WorkflowEdgeDsl>) => {
    const currentEdge = workflow.edges[index];
    if (!currentEdge) return;
    const updatedEdge = { ...currentEdge, ...patch };
    if (updatedEdge.on === 'success' && updatedEdge.to === NEW_ROUND_NODE) updatedEdge.to = END_NODE;
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

  const deleteSelectedEdge = () => {
    if (selectedEdgeIndex < 0) return;
    const next = { ...workflow, edges: workflow.edges.filter((_, index) => index !== selectedEdgeIndex) };
    syncWorkflow(next);
    setSelectedEdgeId(null);
  };

  return (
    <>
      <Dialog open={validationDialogOpen} onOpenChange={closeValidationDialog}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('workflowEditor.validationDialogTitle')}</DialogTitle>
            <DialogDescription>{t('workflowEditor.validationDialogDescription')}</DialogDescription>
          </DialogHeader>
          <div className="max-h-80 space-y-2 overflow-auto rounded-lg border bg-muted/20 p-3 text-sm">
            {pendingValidation?.issues.map((issue, index) => (
              <div key={`${issue.message}:${index}`} className="rounded-md bg-background/70 px-3 py-2 text-destructive">
                {issue.message}
              </div>
            ))}
          </div>
          <DialogFooter>
            <Button onClick={() => closeValidationDialog(false)}>{t('workflowEditor.validationDialogClose')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      <div className="grid min-h-[620px] gap-3 lg:grid-cols-[minmax(0,1fr)_340px]">
      <AppCard className="min-h-0 gap-0 overflow-hidden py-0">
        <CardHeader className="flex flex-row items-center justify-between border-b px-4 py-3">
          <div className="min-w-0">
            <CardTitle>{t('workflowEditor.title')}</CardTitle>
            <p className="mt-1 text-xs text-muted-foreground">{t('workflowEditor.subtitle')}</p>
          </div>
          <div className="flex items-center gap-2">
            <Tabs value={tab} onValueChange={(value) => setTab(value as EditorTab)}>
              <TabsList>
                <TabsTrigger value="canvas">{t('workflowEditor.canvas')}</TabsTrigger>
                <TabsTrigger value="json">JSON</TabsTrigger>
              </TabsList>
            </Tabs>
            {defaultWorkflow ? <Button variant="outline" size="sm" onClick={applyDefaultTemplate}>{t('workflowEditor.defaultTemplate')}</Button> : null}
            {showSaveAction ? <Button size="sm" disabled={!canSave || saving} onClick={() => void handleSave()}>{t('workflowEditor.saveWorkflow')}</Button> : null}
          </div>
        </CardHeader>
        <CardContent className="min-h-0 flex-1 p-0">
          {tab === 'canvas' ? (
            <div className="relative h-[560px] min-h-0">
              <div className="absolute left-3 top-3 z-10 flex items-center gap-1 rounded-full border border-border/70 bg-background/75 p-1 shadow-sm shadow-background/20 backdrop-blur-md">
                <Button size="sm" variant="ghost" className="h-8 rounded-full px-3 text-xs font-medium hover:bg-muted/80" onClick={addWorkerNode}>
                  <Plus className="size-3.5" />
                  {t('workflowEditor.addWorkerNode')}
                </Button>
                {allowAiDynamic ? (
                  <span className="inline-flex items-center">
                    <Button size="sm" variant="ghost" className="h-8 rounded-full px-3 text-xs font-medium hover:bg-muted/80" onClick={addAiDynamicNode}>
                      <Sparkles className="size-3.5" />
                      {t('workflowEditor.addAiDynamicNode')}
                    </Button>
                    <TooltipProvider>
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button type="button" variant="ghost" size="icon-xs" className="ml-0.5 h-7 w-7 rounded-full" aria-label={t('workflowEditor.aiDynamicHelp')}>
                            <CircleHelp className="size-3.5" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent className="max-w-72 whitespace-pre-wrap break-words text-[12px] leading-relaxed" side="bottom" sideOffset={8}>
                          {t('workflowEditor.aiDynamicHelp')}
                        </TooltipContent>
                      </Tooltip>
                    </TooltipProvider>
                  </span>
                ) : null}
                <Button size="sm" variant="ghost" className="h-8 rounded-full px-2.5 text-xs font-medium text-muted-foreground hover:bg-destructive/10 hover:text-destructive disabled:hover:bg-transparent" disabled={!selectedNodeId} onClick={deleteSelectedNode}>
                  <Trash2 className="size-3.5" />
                  {t('workflowEditor.deleteNode')}
                </Button>
              </div>
              {terminalMenu ? (
                <div className="absolute z-20 w-44 overflow-hidden rounded-xl border bg-popover p-1 text-sm text-popover-foreground shadow-lg" style={{ left: terminalMenu.x, top: terminalMenu.y }}>
                  <button type="button" className="flex w-full items-center rounded-md px-3 py-2 text-left hover:bg-accent hover:text-accent-foreground" onClick={() => showTerminalTarget(END_NODE)}>{t('workflowEditor.addEndTarget')}</button>
                  <button type="button" className="flex w-full items-center rounded-md px-3 py-2 text-left hover:bg-accent hover:text-accent-foreground" onClick={() => showTerminalTarget(NEW_ROUND_NODE)}>{t('workflowEditor.addNewRoundTarget')}</button>
                </div>
              ) : null}
              <ReactFlow
                nodes={nodes}
                edges={edges}
                onConnect={handleConnect}
                onPaneClick={() => setTerminalMenu(null)}
                onPaneContextMenu={(event) => {
                  event.preventDefault();
                  const target = event.currentTarget as Element | null;
                  if (!target) return;
                  const bounds = target.getBoundingClientRect();
                  setTerminalMenu({ x: event.clientX - bounds.left, y: event.clientY - bounds.top });
                }}
                onInit={(instance) => setFlowInstance(instance)}
                onNodeClick={(_, node) => {
                  if (node.data.terminal) {
                    setSelectedNodeId(null);
                  } else {
                    setSelectedNodeId(node.id);
                  }
                  setSelectedEdgeId(null);
                }}
                onEdgeClick={(_, edge) => { setSelectedEdgeId(edge.id); setSelectedNodeId(null); }}
                nodesDraggable={false}
                nodesConnectable
                elementsSelectable={false}
                nodesFocusable={false}
                edgesFocusable={false}
                fitView
                proOptions={{ hideAttribution: true }}
                edgeTypes={edgeTypes}
                nodeTypes={editorNodeTypes}
                className="workflow-graph bg-muted/10"
              >
                <Background color="var(--border)" gap={28} size={1} />
                <Controls showInteractive={false} position="bottom-right" />
              </ReactFlow>
            </div>
          ) : (
            <div className="h-[560px] p-4">
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
                  onChange?.(nextWorkflow);
                }}
                className="h-full min-h-full resize-none font-mono text-xs"
                spellCheck={false}
              />
              {jsonError ? <p className="mt-2 text-xs text-destructive">{jsonError}</p> : null}
            </div>
          )}
        </CardContent>
      </AppCard>
      <AppCard className="min-h-0 gap-0 overflow-hidden py-0">
        <CardHeader className="border-b px-4 py-3">
          <CardTitle>{t('workflowEditor.inspector')}</CardTitle>
        </CardHeader>
        <CardContent className="min-h-0 p-0">
          <ScrollArea className="h-[620px]">
            <div className="space-y-4 p-4">
              <WorkflowControlInspector control={workflow.control} fieldErrors={fieldErrors} onUpdate={updateWorkflowControl} t={t} />
              {!agents.length ? <EmptyState>{t('workflowEditor.noAgents')}</EmptyState> : null}
              {selectedNode ? <NodeInspector node={selectedNode} agents={agents} profiles={profiles} workflow={workflow} workflowTemplates={workflowTemplates ?? null} fieldErrors={fieldErrors} onUpdate={updateNode} onOpenProfileManagement={onOpenProfileManagement} t={t} /> : null}
              {selectedEdge ? <EdgeInspector edge={selectedEdge} index={selectedEdgeIndex} workflow={workflow} fieldErrors={fieldErrors} onUpdate={updateEdge} onDelete={deleteSelectedEdge} t={t} /> : null}
              {!selectedNode && !selectedEdge ? <EmptyState>{t('workflowEditor.selectHint')}</EmptyState> : null}
            </div>
          </ScrollArea>
        </CardContent>
      </AppCard>
    </div>
    </>
  );
}

function WorkflowControlInspector({ control, fieldErrors, onUpdate, t }: { control: WorkflowControlDsl; fieldErrors: Record<string, string[]>; onUpdate: (patch: Partial<WorkflowControlDsl>) => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const errorsFor = (field: string) => fieldErrors[`control:${field}`] ?? [];
  const parseLimit = (value: string) => {
    if (!value.trim()) return null;
    const parsed = Number(value);
    return Number.isFinite(parsed) ? Math.trunc(parsed) : 0;
  };
  return (
    <div className="space-y-3 rounded-xl border bg-card/45 p-3">
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

function NodeInspector(props: { node: WorkflowNodeDsl; agents: ManagedAgentVm[]; profiles: ProfileVm[]; workflow: WorkflowDsl; workflowTemplates: WorkflowTemplateStore | null; fieldErrors: Record<string, string[]>; onUpdate: (nodeId: string, patch: Partial<WorkflowNodeDsl>) => void; onOpenProfileManagement?: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
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
    <div className="space-y-3 rounded-xl border bg-card/45 p-3">
      <div className="flex items-center justify-between gap-2">
        <strong className="text-sm">{t('workflowEditor.nodeConfig')}</strong>
        <Badge variant="outline">worker</Badge>
      </div>
      <Field label={t('workflowEditor.nodeId')} errors={errorsFor('id')}>
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
      <Field label={t('workflowEditor.agent')} errors={errorsFor('provider')}>
        <Select value={node.provider ?? ''} onValueChange={(provider) => updateWorker({ provider, permission_mode: null })}>
          <SelectTrigger className={errorClass(errorsFor('provider'))}><SelectValue placeholder={t('workflowEditor.selectAgent')} /></SelectTrigger>
          <SelectContent>{agents.map((agent) => <SelectItem value={agent.agentType} key={agent.agentType}>{agent.displayName}</SelectItem>)}</SelectContent>
        </Select>
        {agents.length === 0 ? <p className="text-xs text-muted-foreground">{t('workflowEditor.noDoctorReadyAgents')}</p> : null}
      </Field>
      <Field label={<ProfileLabel t={t} onOpenProfileManagement={onOpenProfileManagement} />} errors={errorsFor('profile')}>
        <ProfilePicker profiles={profiles} value={node.profile ?? null} invalid={errorsFor('profile').length > 0} onChange={(profile) => updateWorker({ profile })} t={t} />
      </Field>
      <Field label={t('workflowEditor.permissionMode')} errors={errorsFor('permission_mode')}>
        <Select value={node.permission_mode ?? DEFAULT_PERMISSION_MODE} onValueChange={(value) => updateWorker({ permission_mode: value === DEFAULT_PERMISSION_MODE ? null : value })}>
          <SelectTrigger className={errorClass(errorsFor('permission_mode'))}>
            <SelectValue placeholder={t('workflowEditor.permissionModeDefault')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={DEFAULT_PERMISSION_MODE}>{t('workflowEditor.permissionModeDefault')}</SelectItem>
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
            <Field label={t('workflowEditor.outputArtifact')} errors={errorsFor('output.artifact')}>
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
            <Field label={<HelpLabel label={t('workflowEditor.successExpression')} help={t('workflowEditor.successExpressionHelp')} />} errors={errorsFor('success_condition')}>
              <Input className={cn('font-mono', errorClass(errorsFor('success_condition')))} value={expression} placeholder="$.result == true" onChange={(event) => updateWorker({ manual_check: null, success_condition: { expression: event.target.value } })} />
            </Field>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function AiDynamicNodeInspector({ node, agents, profiles, workflowTemplates, fieldErrors, onUpdate, t }: { node: WorkflowAiDynamicNodeDsl; agents: ManagedAgentVm[]; profiles: ProfileVm[]; workflow: WorkflowDsl; workflowTemplates: WorkflowTemplateStore | null; fieldErrors: Record<string, string[]>; onUpdate: (nodeId: string, patch: Partial<WorkflowNodeDsl>) => void; onOpenProfileManagement?: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const [nodeIdDraft, setNodeIdDraft] = useState(node.id);
  const [nodeIdComposing, setNodeIdComposing] = useState(false);
  const control = { ...defaultDynamicControl(), ...(node.control ?? {}) };
  const templates = workflowTemplates?.templates ?? [];
  const strategy = node.agentStrategy.mode === 'dynamic'
    ? node.agentStrategy
    : node.agentStrategy as WorkflowAiDynamicFixedAgentStrategyDsl;
  const permissionModeAgentId = node.agentStrategy.mode === 'fixed'
    ? node.agentStrategy.provider
    : node.agentStrategy.bootstrapProvider;
  const permissionModes = agents.find((agent) => agent.agentType === permissionModeAgentId)?.supportedModes ?? [];
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
    <div className="space-y-3 rounded-xl border bg-card/45 p-3">
      <div className="flex items-center justify-between gap-2">
        <strong className="text-sm">{t('workflowEditor.nodeConfig')}</strong>
        <Badge variant="outline">{t('workflowEditor.addAiDynamicNode')}</Badge>
      </div>
      <Field label={t('workflowEditor.nodeId')} errors={errorsFor('id')}>
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
      <Field label={<HelpLabel label={t('workflowEditor.dynamicAgentStrategy')} help={t('workflowEditor.dynamicAgentStrategyHelp')} />} errors={errorsFor('agentStrategy.mode')}>
        <Select
          value={node.agentStrategy.mode}
          onValueChange={(mode) => {
            if (mode === 'fixed') {
              const nextProvider = node.agentStrategy.mode === 'fixed'
                ? node.agentStrategy.provider
                : node.agentStrategy.bootstrapProvider;
              updateDynamic({ permission_mode: null } as Partial<WorkflowAiDynamicNodeDsl>);
              updateAgentStrategy({ mode: 'fixed', provider: nextProvider });
              return;
            }
            const nextBootstrapProvider = node.agentStrategy.mode === 'dynamic'
              ? node.agentStrategy.bootstrapProvider
              : node.agentStrategy.provider;
            const nextRoutingPrompt = node.agentStrategy.mode === 'dynamic'
              ? node.agentStrategy.routingPrompt
              : '';
            updateDynamic({ permission_mode: null } as Partial<WorkflowAiDynamicNodeDsl>);
            updateAgentStrategy({
              mode: 'dynamic',
              bootstrapProvider: nextBootstrapProvider,
              routingPrompt: nextRoutingPrompt,
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
        <Field label={<HelpLabel label={t('workflowEditor.agent')} help={t('workflowEditor.dynamicFixedAgentHelp')} />} errors={errorsFor('agentStrategy.provider')}>
          <Select value={node.agentStrategy.provider} onValueChange={(provider) => { updateDynamic({ permission_mode: null } as Partial<WorkflowAiDynamicNodeDsl>); updateAgentStrategy({ mode: 'fixed', provider }); }}>
            <SelectTrigger className={errorClass(errorsFor('agentStrategy.provider'))}><SelectValue placeholder={t('workflowEditor.selectAgent')} /></SelectTrigger>
            <SelectContent>{agents.map((agent) => <SelectItem value={agent.agentType} key={agent.agentType}>{agent.displayName}</SelectItem>)}</SelectContent>
          </Select>
        </Field>
      ) : (
        <>
          <Field label={<HelpLabel label={t('workflowEditor.dynamicBootstrapAgent')} help={t('workflowEditor.dynamicBootstrapAgentHelp')} />} errors={errorsFor('agentStrategy.bootstrapProvider')}>
            <Select value={node.agentStrategy.bootstrapProvider} onValueChange={(bootstrapProvider) => { updateDynamic({ permission_mode: null } as Partial<WorkflowAiDynamicNodeDsl>); updateAgentStrategy({ ...(node.agentStrategy as WorkflowAiDynamicDynamicAgentStrategyDsl), bootstrapProvider }); }}>
              <SelectTrigger className={errorClass(errorsFor('agentStrategy.bootstrapProvider'))}><SelectValue placeholder={t('workflowEditor.selectAgent')} /></SelectTrigger>
              <SelectContent>{agents.map((agent) => <SelectItem value={agent.agentType} key={agent.agentType}>{agent.displayName}</SelectItem>)}</SelectContent>
            </Select>
          </Field>
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
      <Field label={<HelpLabel label={t('workflowEditor.permissionMode')} help={t('workflowEditor.dynamicPermissionModeHelp')} />} errors={errorsFor('permission_mode')}>
        <Select value={node.permission_mode ?? DEFAULT_PERMISSION_MODE} onValueChange={(value) => updateDynamic({ permission_mode: value === DEFAULT_PERMISSION_MODE ? null : value } as Partial<WorkflowAiDynamicNodeDsl>)}>
          <SelectTrigger className={errorClass(errorsFor('permission_mode'))}>
            <SelectValue placeholder={t('workflowEditor.permissionModeDefault')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={DEFAULT_PERMISSION_MODE}>{t('workflowEditor.permissionModeDefault')}</SelectItem>
            {permissionModes.map((mode) => <SelectItem value={mode.id} key={mode.id}>{mode.name}</SelectItem>)}
          </SelectContent>
        </Select>
      </Field>
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
          <Field key={field.key} label={<HelpLabel label={field.label} help={field.help} />} errors={errorsFor(`control.${field.key}`)}>
            <Input className={errorClass(errorsFor(`control.${field.key}`))} type="number" min={1} step={1} value={String(control[field.key])} onChange={(event) => updateControl({ [field.key]: parseLimit(event.target.value) } as Partial<DynamicControlDsl>)} />
          </Field>
        ))}
      </div>
    </div>
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

function ProfileMultiSelect({ profiles, selectedProfileIds, invalid, onChange, t }: { profiles: ProfileVm[]; selectedProfileIds: string[]; invalid: boolean; onChange: (profileIds: string[]) => void; t: (key: string) => string }) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const selected = new Set(selectedProfileIds);
  const profileById = new Map(profiles.map((profile) => [profile.id, profile] as const));
  const selectedProfiles = selectedProfileIds
    .map((profileId) => profileById.get(profileId))
    .filter((profile): profile is ProfileVm => Boolean(profile));
  const invalidProfileIds = selectedProfileIds.filter((profileId) => !profileById.has(profileId));
  const normalizedSearch = search.trim().toLowerCase();
  const filteredProfiles = normalizedSearch
    ? profiles.filter((profile) => profileSearchText(profile).includes(normalizedSearch))
    : profiles;
  const toggleProfile = (profileId: string) => {
    const next = selected.has(profileId)
      ? selectedProfileIds.filter((item) => item !== profileId)
      : [...selectedProfileIds, profileId];
    onChange(next);
  };
  const removeProfile = (profileId: string) => onChange(selectedProfileIds.filter((item) => item !== profileId));

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
                <span role="button" tabIndex={0} className="rounded-full hover:text-destructive" onClick={(event) => { event.preventDefault(); event.stopPropagation(); removeProfile(profile.id); }} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') removeProfile(profile.id); }}>
                  <X className="size-3" />
                </span>
              </Badge>
            ))}
            {invalidProfileIds.map((profileId) => (
              <Badge key={profileId} variant="destructive" className="max-w-full gap-1">
                <span className="max-w-44 truncate font-mono text-[10px]">{profileId}</span>
                <span role="button" tabIndex={0} className="rounded-full" onClick={(event) => { event.preventDefault(); event.stopPropagation(); removeProfile(profileId); }} onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') removeProfile(profileId); }}>
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
  const selected = new Set(selectedWorkflowIds);
  const workflowIdCounts = workflowIdCountMap(templates);
  const uniqueSelectableTemplateByWorkflowId = new Map(
    templates
      .filter((template) => workflowDisabledReason(template, workflowIdCounts, allowNestedDynamic, t) === null)
      .map((template) => [template.workflow.id.trim(), template] as const),
  );
  const selectedTemplates = selectedWorkflowIds
    .map((workflowId) => uniqueSelectableTemplateByWorkflowId.get(workflowId))
    .filter((template): template is WorkflowTemplate => Boolean(template));
  const invalidWorkflowIds = selectedWorkflowIds.filter((workflowId) => !uniqueSelectableTemplateByWorkflowId.has(workflowId));
  const workflowOptions = templates.map((template) => ({
    template,
    reason: workflowDisabledReason(template, workflowIdCounts, allowNestedDynamic, t),
  }));
  const selectableOptions = workflowOptions.filter((option) => option.reason === null);
  const disabledOptions = workflowOptions.filter((option) => option.reason !== null);
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
                <span className="max-w-40 truncate">{template.name}</span>
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
                  <CommandItem key={workflowId} value={workflowTemplateSearchText(template)} onSelect={() => toggleWorkflow(workflowId)} className="items-start py-2">
                    <Check className={cn('mt-0.5 size-4', selected.has(workflowId) ? 'opacity-100' : 'opacity-0')} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">{template.name}</span>
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
                    <CommandItem key={`${template.id}:${workflowId}:${index}`} value={workflowTemplateSearchText(template)} disabled className="items-start py-2 opacity-60">
                      <span className="mt-1 size-4 shrink-0 rounded-full border border-muted-foreground/40" />
                      <span className="min-w-0 flex-1">
                        <span className="block truncate font-medium">{template.name}</span>
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
    case 'project':
      return t('contextManagement.projectScope');
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

function workflowTemplateSearchText(template: WorkflowTemplate) {
  return [template.name, template.workflow.id].join('\n').toLowerCase();
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

function EdgeInspector({ edge, index, workflow, fieldErrors, onUpdate, onDelete, t }: { edge: WorkflowEdgeDsl; index: number; workflow: WorkflowDsl; fieldErrors: Record<string, string[]>; onUpdate: (index: number, patch: Partial<WorkflowEdgeDsl>) => void; onDelete: () => void; t: (key: string) => string }) {
  const errorsFor = (field: string) => fieldErrors[`edge:${index}:${field}`] ?? [];
  const targetOptions = edge.on === 'success' ? [END_NODE] : [END_NODE, NEW_ROUND_NODE];
  return (
    <div className="space-y-3 rounded-xl border bg-card/45 p-3">
      <div className="flex items-center justify-between gap-2">
        <strong className="text-sm">{t('workflowEditor.edgeConfig')}</strong>
        <Button size="sm" variant="outline" onClick={onDelete}>{t('workflowEditor.deleteEdge')}</Button>
      </div>
      <Field label={t('workflowEditor.edgeOutcome')} errors={errorsFor('on')}>
        <Select value={edge.on} onValueChange={(on) => onUpdate(index, { on: on as EdgeOutcome })}>
          <SelectTrigger className={errorClass(errorsFor('on'))}><SelectValue /></SelectTrigger>
          <SelectContent>{(['success', 'failure'] as EdgeOutcome[]).map((value) => <SelectItem value={value} key={value}>{value}</SelectItem>)}</SelectContent>
        </Select>
      </Field>
      <Field label={t('workflowEditor.edgeTarget')} errors={errorsFor('to')}>
        <Select value={edge.to} onValueChange={(to) => onUpdate(index, { to })}>
          <SelectTrigger className={errorClass(errorsFor('to'))}><SelectValue /></SelectTrigger>
          <SelectContent>
            {workflow.nodes.map((node) => <SelectItem value={node.id} key={node.id}>{node.id}</SelectItem>)}
            {targetOptions.map((target) => <SelectItem value={target} key={target}>{target}</SelectItem>)}
          </SelectContent>
        </Select>
      </Field>
      <Field label={t('workflowEditor.sessionMode')} errors={errorsFor('session')}>
        <Select value={edge.session ?? 'new'} onValueChange={(session) => onUpdate(index, { session: session as SessionMode })}>
          <SelectTrigger className={errorClass(errorsFor('session'))}><SelectValue /></SelectTrigger>
          <SelectContent>
            <SelectItem value="new">new</SelectItem>
            <SelectItem value="continue">continue</SelectItem>
          </SelectContent>
        </Select>
      </Field>
    </div>
  );
}

function Field({ label, children, errors = [] }: { label: React.ReactNode; children: React.ReactNode; errors?: string[] }) {
  return (
    <div className="grid gap-1.5 text-sm">
      <div className={cn('flex items-center gap-2 text-xs font-medium text-muted-foreground', errors.length > 0 && 'text-destructive')}>{label}</div>
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
  const lane = data?.lane;
  const sourceOffsetX = sourceX + 34;
  const targetOffsetX = targetX - 34;
  const laneY = lane === undefined ? null : Math.min(sourceY, targetY) - 82 - lane * 38;
  const [smoothPath, smoothLabelX, smoothLabelY] = getSmoothStepPath({ sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition });
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
            className="pointer-events-none absolute rounded-full border bg-background/90 px-2 py-0.5 text-[11px] font-semibold shadow-sm"
            style={{ color: style?.stroke, transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)` }}
          >
            {label}
          </span>
        </EdgeLabelRenderer>
      ) : null}
    </>
  );
}

function workflowToFlow(workflow: WorkflowDsl, selectedNodeId: string | null, selectedEdgeId: string | null, invalidNodeIds: Set<string>, visibleTerminalIds: Set<string>, t: (key: string) => string): { nodes: Node<EditorNodeData>[]; edges: Edge[] } {
  const collectedNodes = collectAuthoringNodes(workflow);
  const collectedIds = new Set(collectedNodes.map((node) => node.id));
  const allNodes = [
    ...collectedNodes,
    ...Array.from(visibleTerminalIds).filter((id) => !collectedIds.has(id)).map((id) => ({ id, terminal: true })),
  ];
  const nodeIds = new Set(allNodes.map((n) => n.id));
  const nodeOrder = workflowNodeOrder(workflow);
  const retryLaneByEdgeIndex = computeBackwardLanes(workflow.edges as Array<{ from: string; to: string; on: string }>, nodeOrder);
  const layoutPositions = layoutSuccessPath(
    allNodes.map((n) => ({ id: n.id, width: n.terminal ? TERMINAL_NODE_WIDTH : NODE_WIDTH, height: n.terminal ? TERMINAL_NODE_HEIGHT : NODE_HEIGHT })),
    workflow.edges.map((e) => ({ from: e.from, to: e.to, on: e.on })),
    nodeIds,
    nodeOrder,
  );

  const nodes: Node<EditorNodeData>[] = allNodes.map((item) => {
    const pos = layoutPositions.get(item.id) ?? { x: 0, y: 0 };
    const width = item.terminal ? TERMINAL_NODE_WIDTH : NODE_WIDTH;
    const height = item.terminal ? TERMINAL_NODE_HEIGHT : NODE_HEIGHT;
    const node = workflow.nodes.find((n) => n.id === item.id);
    const detail = node && 'goal' in node ? node.goal ?? '' : node?.type ?? item.id;
    const invalid = !item.terminal && invalidNodeIds.has(item.id);
    const provider = node && 'provider' in node ? node.provider : undefined;
    const iconKey = provider ? providerToIconKey(provider) : undefined;
    return {
      id: item.id,
      type: 'editorCanvas',
      position: topLeft(pos.x, pos.y, width, height),
      sourcePosition: SOURCE_POS,
      targetPosition: TARGET_POS,
      data: { label: workflowNodeLabel(item.id, item.terminal, node?.type, t), kind: item.terminal ? 'terminal' : node?.type ?? 'node', detail, terminal: item.terminal, iconKey },
      className: cn(!item.terminal && item.id === selectedNodeId && 'workflow-node-selected', invalid && 'ring-1 ring-destructive'),
      selected: !item.terminal && item.id === selectedNodeId,
      draggable: false,
      selectable: true,
      connectable: true,
      style: { width, height },
    };
  });

  const edges: Edge<WorkflowEdgeData>[] = workflow.edges.map((edge, index) => {
    const id = edgeId(edge, index);
    const retryLane = retryLaneByEdgeIndex.get(index);
    const color = authoringEdgeColor(edge.on);
    return {
      id,
      source: edge.from,
      target: edge.to,
      label: edge.on === 'success' ? undefined : workflowEdgeLabel(edge.on, t),
      type: retryLane === undefined && edge.on === 'success' ? 'smoothstep' : 'workflowRouted',
      animated: false,
      markerEnd: { type: MarkerType.ArrowClosed, width: 16, height: 16, color },
      style: { stroke: color, strokeWidth: edge.on === 'success' ? 2.2 : 2, strokeDasharray: '3 17' },
      className: cn('workflow-edge-flow', (edge.on !== 'success' || retryLane !== undefined) && 'workflow-edge-branch', id === selectedEdgeId && 'workflow-edge-selected'),
      selected: id === selectedEdgeId,
      labelStyle: { fill: color, fontSize: 11, fontWeight: 600 },
      labelBgStyle: { fill: 'var(--background)', fillOpacity: 0.86 },
      labelShowBg: false,
      data: { outcome: edge.on, lane: retryLane },
      zIndex: retryLane === undefined ? 0 : 2,
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
    nodes: workflow.nodes.map((node) => {
      if (node.type === 'ai-dynamic') {
        const rawNode = node as WorkflowAiDynamicNodeDsl & {
          provider?: string | null;
          profile?: string | null;
          goal?: string | null;
          agentStrategy?: WorkflowAiDynamicNodeDsl['agentStrategy'];
          permissionMode?: string | null;
          allowedProfiles?: string[];
          globalGoal?: string | null;
        };
        const normalizedStrategy = rawNode.agentStrategy ?? {
          mode: 'fixed',
          provider: rawNode.provider ?? '',
        };
        return {
          ...node,
          agentStrategy: normalizedStrategy,
          permission_mode: node.permission_mode ?? rawNode.permissionMode ?? null,
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
  const incomingEdgeCounts = workflow.edges.reduce<Record<string, number>>((counts, edge) => {
    if (edge.to.trim() && ![END_NODE, NEW_ROUND_NODE].includes(edge.to)) {
      counts[edge.to] = (counts[edge.to] ?? 0) + 1;
    }
    return counts;
  }, {});
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

  const addIssue = (message: string, fieldKey?: string, nodeId?: string, edgeIndex?: number) => {
    issues.push({ message, fieldKey, nodeId, edgeIndex });
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
  if (!workflow.entry.trim()) addIssue(t('workflowEditor.validationEntryRequired'));
  else if (!nodeIds.has(workflow.entry)) addIssue(t('workflowEditor.validationEntryMissingTarget', { node: workflow.entry }));
  if (!workflow.nodes.length) addIssue(t('workflowEditor.validationNodesRequired'));
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
    if ([END_NODE, NEW_ROUND_NODE].includes(node.id)) addIssue(t('workflowEditor.validationReservedNodeId', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    if ((nodeIdCounts[node.id] ?? 0) > 1) addIssue(t('workflowEditor.validationDuplicateNodeId', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    if (node.id !== workflow.entry && (incomingEdgeCounts[node.id] ?? 0) === 0) {
      addIssue(t('workflowEditor.validationUnreachableNode', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    }
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
  const permissionAgentId = node.agentStrategy.mode === 'fixed'
    ? node.agentStrategy.provider?.trim()
    : node.agentStrategy.bootstrapProvider?.trim();
  if (node.agentStrategy.mode === 'fixed') {
    const provider = node.agentStrategy.provider?.trim();
    if (!provider) {
      addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.provider'), node.id);
    } else if (!agentIds.has(provider)) {
      addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'agentStrategy.provider'), node.id);
    }
  } else {
    const bootstrapProvider = node.agentStrategy.bootstrapProvider?.trim();
    if (!bootstrapProvider) {
      addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.bootstrapProvider'), node.id);
    } else if (!agentIds.has(bootstrapProvider)) {
      addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'agentStrategy.bootstrapProvider'), node.id);
    }
    if (!node.agentStrategy.routingPrompt?.trim()) {
      addIssue(t('workflowEditor.validationDynamicRoutingPromptRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.routingPrompt'), node.id);
    }
  }
  if (node.permission_mode?.trim() && permissionAgentId) {
    const supportedModeIds = new Set((agentById.get(permissionAgentId)?.supportedModes ?? []).map((mode) => mode.id));
    if (supportedModeIds.size > 0 && !supportedModeIds.has(node.permission_mode)) {
      addIssue(t('workflowEditor.validationPermissionModeUnavailable', { node: nodeLabel }), nodeField(node, 'permission_mode'), node.id);
    }
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
