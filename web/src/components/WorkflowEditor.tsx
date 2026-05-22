import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronsUpDown, Info, Plus, Sparkles, Trash2 } from 'lucide-react';
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
import type { AgentRegistryVm, ManagedAgentVm, ProfileVm, WorkflowControlDsl, WorkflowDsl, WorkflowEdgeDsl, WorkflowJsonConditionDsl, WorkflowNodeDsl, WorkflowOutputContractDsl, WorkflowWorkerNodeDsl } from '../types';
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
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/components/ui/command';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

function providerToIconKey(provider: string): string | undefined {
  const mapping: Record<string, string> = { 'claude-code': 'claude', 'codex-cli': 'codex', opencode: 'opencode', 'gemini-cli': 'gemini' };
  return mapping[provider];
}

const DEFAULT_PERMISSION_MODE = '__default_permission_mode__';

type EditorTab = 'canvas' | 'json';
type EdgeOutcome = 'success' | 'failure' | 'invalid';
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
        {data.iconKey ? <img src={`/agent-icons/${data.iconKey}.svg`} alt="" className="size-3.5 shrink-0 rounded-sm" /> : null}
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
  saving?: boolean;
}

export function WorkflowEditor({ value, agentRegistry, profiles = [], onOpenProfileManagement, onSave, onChange, onApplyDefaultTemplate, defaultWorkflow, saving }: WorkflowEditorProps) {
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
  const agents = agentRegistry?.agents.filter((agent) => agent.supported) ?? [];
  const selectedNode = selectedNodeId ? workflow.nodes.find((node) => node.id === selectedNodeId) ?? null : null;
  const selectedEdgeIndex = selectedEdgeId ? Number(selectedEdgeId.split(':').at(-1)) : -1;
  const selectedEdge = selectedEdgeIndex >= 0 ? workflow.edges[selectedEdgeIndex] ?? null : null;
  const workflowJson = useMemo(() => JSON.stringify(workflow, null, 2), [workflow]);
  const canSave = workflow.nodes.length > 0 && workflow.entry.trim() !== '' && agents.length > 0;
  const { nodes, edges } = useMemo(() => workflowToFlow(workflow, selectedNodeId, selectedEdgeId, invalidNodeIds, visibleTerminalIds, t), [invalidNodeIds, selectedEdgeId, selectedNodeId, t, visibleTerminalIds, workflow]);

  useEffect(() => {
    if (JSON.stringify(workflow) === JSON.stringify(initialWorkflow)) return;
    setWorkflow(initialWorkflow);
    setSelectedNodeId(initialWorkflow.nodes[0]?.id ?? null);
    setSelectedEdgeId(null);
    setVisibleTerminalIds(new Set());
    setTerminalMenu(null);
  }, [initialWorkflow]);

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
    setWorkflow(next);
    onChange?.(next);
  };

  const closeValidationDialog = (open: boolean) => {
    setValidationDialogOpen(open);
    if (open || !pendingValidation) return;
    setFieldErrors(pendingValidation.fieldErrors);
    setInvalidNodeIds(new Set(pendingValidation.issues.map((issue) => issue.nodeId).filter(Boolean) as string[]));
    setWorkflow(pendingValidation.sanitizedWorkflow);
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
    const edge: WorkflowEdgeDsl = { from: connection.source, to: connection.target, on: 'success' };
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
    const validation = validateWorkflowForSave(workflow, profiles, agents, t);
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
        issues: [{ message: String(error) }],
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
      goal: t('workflowEditor.defaultNodeGoal'),
      primary_artifact: null,
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

  const updateNode = (nodeId: string, patch: Partial<WorkflowWorkerNodeDsl>) => {
    const nextId = patch.id && patch.id !== nodeId ? sanitizeNodeId(patch.id, workflow, nodeId) : null;
    const next = {
      ...workflow,
      entry: nextId && workflow.entry === nodeId ? nextId : workflow.entry,
      nodes: workflow.nodes.map((node) => node.id === nodeId ? { ...node, ...patch, id: nextId ?? node.id } : node),
      edges: nextId ? workflow.edges.map((edge) => ({ ...edge, from: edge.from === nodeId ? nextId : edge.from, to: edge.to === nodeId ? nextId : edge.to })) : workflow.edges,
    };
    syncWorkflow(next);
    if (nextId) setSelectedNodeId(nextId);
  };

  const updateEdge = (index: number, patch: Partial<WorkflowEdgeDsl>) => {
    const currentEdge = workflow.edges[index];
    if (!currentEdge) return;
    const updatedEdge = { ...currentEdge, ...patch };
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
            <Button size="sm" disabled={!canSave || saving} onClick={() => void handleSave()}>{t('workflowEditor.saveWorkflow')}</Button>
          </div>
        </CardHeader>
        <CardContent className="min-h-0 flex-1 p-0">
          {tab === 'canvas' ? (
            <div className="relative h-[560px] min-h-0">
              <div className="absolute left-3 top-3 z-10 flex items-center gap-1 rounded-full border border-border/70 bg-background/75 p-1 shadow-sm shadow-background/20 backdrop-blur-md">
                <Button size="sm" variant="ghost" className="h-8 rounded-full px-3 text-xs font-medium hover:bg-muted/80" onClick={addWorkerNode}>
                  <Plus className="size-3.5" />
                  {t('workflowEditor.addNode')}
                </Button>
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
            <ScrollArea className="h-[560px] p-4">
              <CodeBlock>{workflowJson}</CodeBlock>
            </ScrollArea>
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
              {selectedNode ? <NodeInspector node={selectedNode} agents={agents} profiles={profiles} workflow={workflow} fieldErrors={fieldErrors} onUpdate={updateNode} onOpenProfileManagement={onOpenProfileManagement} t={t} /> : null}
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

function NodeInspector({ node, agents, profiles, workflow, fieldErrors, onUpdate, onOpenProfileManagement, t }: { node: WorkflowNodeDsl; agents: ManagedAgentVm[]; profiles: ProfileVm[]; workflow: WorkflowDsl; fieldErrors: Record<string, string[]>; onUpdate: (nodeId: string, patch: Partial<WorkflowWorkerNodeDsl>) => void; onOpenProfileManagement?: () => void; t: (key: string, options?: Record<string, unknown>) => string }) {
  const [nodeIdDraft, setNodeIdDraft] = useState(node.id);
  const [nodeIdComposing, setNodeIdComposing] = useState(false);
  const [schemaDraft, setSchemaDraft] = useState('');
  const [schemaError, setSchemaError] = useState<string | null>(null);
  const [schemaDirty, setSchemaDirty] = useState(false);
  const schemaSelfUpdateNodeId = useRef<string | null>(null);

  const validationEnabled = Boolean(node.output || node.success_condition || node.primary_artifact);
  const manualCheckEnabled = Boolean(node.manual_check);
  const resultMode = validationEnabled ? 'ai' : manualCheckEnabled ? 'manual' : 'none';
  const expression = conditionExpression(node.success_condition);
  const selectedAgent = agents.find((agent) => agent.agentType === node.provider) ?? null;
  const supportedModes = selectedAgent?.supportedModes ?? [];
  const permissionModes = node.permission_mode && !supportedModes.some((mode) => mode.id === node.permission_mode)
    ? [...supportedModes, { id: node.permission_mode, name: node.permission_mode }]
    : supportedModes;
  const errorsFor = (field: string) => fieldErrors[`node:${node.id}:${field}`] ?? [];
  const clearValidationPatch = { output: null, success_condition: null, primary_artifact: null };
  const updateOutput = useCallback((patch: Partial<WorkflowOutputContractDsl>) => {
    const artifact = patch.artifact ?? node.output?.artifact ?? `${node.id}-result`;
    onUpdate(node.id, {
      manual_check: null,
      primary_artifact: artifact,
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
    onUpdate(node.id, { id: value });
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
        <Select value={node.provider ?? ''} onValueChange={(provider) => onUpdate(node.id, { provider })}>
          <SelectTrigger className={errorClass(errorsFor('provider'))}><SelectValue placeholder={t('workflowEditor.selectAgent')} /></SelectTrigger>
          <SelectContent>{agents.map((agent) => <SelectItem value={agent.agentType} key={agent.agentType}>{agent.displayName}</SelectItem>)}</SelectContent>
        </Select>
      </Field>
      <Field label={<ProfileLabel t={t} onOpenProfileManagement={onOpenProfileManagement} />} errors={errorsFor('profile')}>
        <ProfilePicker profiles={profiles} value={node.profile ?? null} invalid={errorsFor('profile').length > 0} onChange={(profile) => onUpdate(node.id, { profile })} t={t} />
      </Field>
      <Field label={t('workflowEditor.permissionMode')}>
        <Select value={node.permission_mode ?? DEFAULT_PERMISSION_MODE} onValueChange={(value) => onUpdate(node.id, { permission_mode: value === DEFAULT_PERMISSION_MODE ? null : value })}>
          <SelectTrigger>
            <SelectValue placeholder={t('workflowEditor.permissionModeDefault')} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={DEFAULT_PERMISSION_MODE}>{t('workflowEditor.permissionModeDefault')}</SelectItem>
            {permissionModes.map((mode) => <SelectItem value={mode.id} key={mode.id}>{mode.name}</SelectItem>)}
          </SelectContent>
        </Select>
      </Field>
      <Field label={t('workflowEditor.goal')} errors={errorsFor('goal')}>
        <Textarea className={errorClass(errorsFor('goal'))} value={node.goal ?? ''} onChange={(event) => onUpdate(node.id, { goal: event.target.value })} />
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
            if (mode === 'ai') onUpdate(node.id, { ...defaultValidationPatch(node.id), manual_check: null });
            if (mode === 'manual') onUpdate(node.id, { ...clearValidationPatch, manual_check: true });
            if (mode === 'none') onUpdate(node.id, { ...clearValidationPatch, manual_check: null });
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
                    <TooltipContent showArrow={false} className="rounded-md border border-border/70 bg-popover px-2 py-1 text-xs text-popover-foreground shadow-lg">
                      {t('workflowEditor.outputSchemaBeautify')}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </div>
              {schemaError ? <span className="text-xs text-destructive">{schemaError}</span> : null}
            </Field>
            <Field label={<HelpLabel label={t('workflowEditor.successExpression')} help={t('workflowEditor.successExpressionHelp')} />} errors={errorsFor('success_condition')}>
              <Input className={cn('font-mono', errorClass(errorsFor('success_condition')))} value={expression} placeholder="$.result == true" onChange={(event) => onUpdate(node.id, { manual_check: null, success_condition: { expression: event.target.value } })} />
            </Field>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function ProfileLabel({ t, onOpenProfileManagement }: { t: (key: string) => string; onOpenProfileManagement?: () => void }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span>{t('workflowEditor.profile')}</span>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button type="button" variant="ghost" size="icon-xs" onClick={(event) => event.preventDefault()} aria-label={t('workflowEditor.profileHelp')}>
              <Info className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent showArrow={false} className="max-w-80 border border-amber-400/25 bg-neutral-950/95 px-3.5 py-2.5 text-[12px] leading-relaxed text-neutral-100 shadow-2xl shadow-black/60 ring-1 ring-white/10 backdrop-blur-md">{t('workflowEditor.profileHelp')}</TooltipContent>
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
                      <span className="flex items-center justify-between gap-2 font-medium"><span className="truncate">{profile.name}</span><span className="shrink-0 text-[11px] text-muted-foreground">{profile.scope}</span></span>
                      <span className="mt-1 block truncate font-mono text-[11px] text-muted-foreground">{profile.id}</span>
                      <span className="mt-1 block truncate text-xs text-muted-foreground" title={profile.summary}>{profile.summary}</span>
                      <span className="mt-1 block text-[11px] text-muted-foreground">{profile.createdAt} / {profile.updatedAt}</span>
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
        <TooltipContent showArrow={false} className="max-w-80 border border-amber-400/25 bg-neutral-950/95 px-3.5 py-2.5 text-[12px] leading-relaxed text-neutral-100 shadow-2xl shadow-black/60 ring-1 ring-white/10 backdrop-blur-md">
          <div className="space-y-1">
            <p className="font-semibold">{profile.name}</p>
            <p className="font-mono text-[11px] text-neutral-300">{profile.id}</p>
            <p>{profile.summary}</p>
            <p className="text-neutral-400">{profile.createdAt} / {profile.updatedAt}</p>
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function profileSearchText(profile: ProfileVm) {
  return [profile.id, profile.name, profile.summary, profile.content, profile.scope].join('\n').toLowerCase();
}

function profileCommandScore(itemValue: string, search: string) {
  const normalizedSearch = search.trim().toLowerCase();
  if (!normalizedSearch) return 1;
  return itemValue.toLowerCase().includes(normalizedSearch) ? 1 : 0;
}

function EdgeInspector({ edge, index, workflow, fieldErrors, onUpdate, onDelete, t }: { edge: WorkflowEdgeDsl; index: number; workflow: WorkflowDsl; fieldErrors: Record<string, string[]>; onUpdate: (index: number, patch: Partial<WorkflowEdgeDsl>) => void; onDelete: () => void; t: (key: string) => string }) {
  const errorsFor = (field: string) => fieldErrors[`edge:${index}:${field}`] ?? [];
  return (
    <div className="space-y-3 rounded-xl border bg-card/45 p-3">
      <div className="flex items-center justify-between gap-2">
        <strong className="text-sm">{t('workflowEditor.edgeConfig')}</strong>
        <Button size="sm" variant="outline" onClick={onDelete}>{t('workflowEditor.deleteEdge')}</Button>
      </div>
      <Field label={t('workflowEditor.edgeOutcome')} errors={errorsFor('on')}>
        <Select value={edge.on} onValueChange={(on) => onUpdate(index, { on: on as EdgeOutcome })}>
          <SelectTrigger className={errorClass(errorsFor('on'))}><SelectValue /></SelectTrigger>
          <SelectContent>{(['success', 'failure', 'invalid'] as EdgeOutcome[]).map((value) => <SelectItem value={value} key={value}>{value}</SelectItem>)}</SelectContent>
        </Select>
      </Field>
      <Field label={t('workflowEditor.edgeTarget')} errors={errorsFor('to')}>
        <Select value={edge.to} onValueChange={(to) => onUpdate(index, { to })}>
          <SelectTrigger className={errorClass(errorsFor('to'))}><SelectValue /></SelectTrigger>
          <SelectContent>
            {workflow.nodes.map((node) => <SelectItem value={node.id} key={node.id}>{node.id}</SelectItem>)}
            <SelectItem value={END_NODE}>{END_NODE}</SelectItem>
            <SelectItem value={NEW_ROUND_NODE}>{NEW_ROUND_NODE}</SelectItem>
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
      <Label className={cn('text-xs text-muted-foreground', errors.length > 0 && 'text-destructive')}>{label}</Label>
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
              variant="outline"
              size="icon-xs"
              aria-label={help}
              onClick={(event) => event.preventDefault()}
            >
              ?
            </Button>
          </TooltipTrigger>
          <TooltipContent
            showArrow={false}
            className="max-w-80 border border-amber-400/25 bg-neutral-950/95 px-3.5 py-2.5 text-[12px] leading-relaxed text-neutral-100 shadow-2xl shadow-black/60 ring-1 ring-white/10 backdrop-blur-md"
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
    const detail = node?.type === 'worker' ? (node as WorkflowWorkerNodeDsl).goal ?? '' : node?.type ?? item.id;
    const invalid = !item.terminal && invalidNodeIds.has(item.id);
    const provider = node?.type === 'worker' ? (node as WorkflowWorkerNodeDsl).provider : undefined;
    const iconKey = provider ? providerToIconKey(provider) : undefined;
    return {
      id: item.id,
      type: 'editorCanvas',
      position: topLeft(pos.x, pos.y, width, height),
      sourcePosition: SOURCE_POS,
      targetPosition: TARGET_POS,
      data: { label: workflowNodeLabel(item.id, item.terminal, t), kind: item.terminal ? 'terminal' : node?.type ?? 'node', detail, terminal: item.terminal, iconKey },
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

function workflowNodeLabel(id: string, terminal: boolean, t: (key: string) => string) {
  if (id === END_NODE) return t('workflowEditor.nodeLabels.end');
  if (id === NEW_ROUND_NODE) return t('workflowEditor.nodeLabels.newRound');
  return id;
}

function workflowEdgeLabel(outcome: WorkflowEdgeDsl['on'], t: (key: string) => string) {
  if (['failure', 'invalid'].includes(outcome)) return t(`workflowEditor.edgeLabels.${outcome}`);
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
    primary_artifact: artifact,
    output: { kind: 'json', artifact, schema: null },
    success_condition: { expression: '' },
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
      if (!node.output?.schema) return node;
      return {
        ...node,
        output: {
          ...node.output,
          schema: normalizeOutputSchema(node.output.schema),
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
): WorkflowValidationResult {
  const sanitizedWorkflow = normalizeWorkflowSchemas(cloneWorkflow(workflow));
  const issues: WorkflowValidationIssue[] = [];
  const fieldErrors: Record<string, string[]> = {};
  const profileIds = new Set(profiles.map((profile) => profile.id));
  const agentIds = new Set(agents.map((agent) => agent.agentType));
  const nodeIds = new Set(workflow.nodes.map((node) => node.id).filter(Boolean));
  const edgeOutcomeCounts = workflow.edges.reduce<Record<string, number>>((counts, edge) => {
    if (edge.from.trim() && ['success', 'failure', 'invalid'].includes(edge.on)) {
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
  if (!workflow.entry.trim()) addIssue(t('workflowEditor.validationEntryRequired'));
  else if (!nodeIds.has(workflow.entry)) addIssue(t('workflowEditor.validationEntryMissingTarget', { node: workflow.entry }));
  if (!workflow.nodes.length) addIssue(t('workflowEditor.validationNodesRequired'));
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

    if (!node.provider?.trim()) addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'provider'), node.id);
    else if (!agentIds.has(node.provider)) addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'provider'), node.id);
    if (!node.profile?.trim()) {
      addIssue(t('workflowEditor.validationNodeProfileRequired', { node: nodeLabel }), nodeField(node, 'profile'), node.id);
    } else if (!profileIds.has(node.profile)) {
      addIssue(t('workflowEditor.validationNodeProfileVisibilityChanged', { node: nodeLabel }), nodeField(node, 'profile'), node.id);
      const sanitized = sanitizedWorkflow.nodes[nodeIndex];
      if (sanitized) sanitized.profile = null;
    }
    if (!node.goal?.trim()) addIssue(t('workflowEditor.validationNodeGoalRequired', { node: nodeLabel }), nodeField(node, 'goal'), node.id);

    const validationEnabled = Boolean(node.output || node.success_condition || node.primary_artifact);
    if (validationEnabled && node.manual_check) {
      addIssue(t('workflowEditor.validationResultModeExclusive', { node: nodeLabel }), nodeField(node, 'success_condition'), node.id);
    }
    if (validationEnabled) {
      if (!node.output?.artifact?.trim()) addIssue(t('workflowEditor.validationOutputArtifactRequired', { node: nodeLabel }), nodeField(node, 'output.artifact'), node.id);
      if (node.output?.artifact && node.primary_artifact && node.output.artifact !== node.primary_artifact) {
        addIssue(t('workflowEditor.validationOutputArtifactMismatch', { node: nodeLabel }), nodeField(node, 'output.artifact'), node.id);
      }
      if (!node.success_condition) addIssue(t('workflowEditor.validationSuccessExpressionRequired', { node: nodeLabel }), nodeField(node, 'success_condition'), node.id);
      let path: PathSegment[] | null = null;
      if (node.success_condition) {
        try {
          path = successConditionPath(node.success_condition);
        } catch {
          addIssue(t('workflowEditor.saveErrorInvalidExpression', { node: nodeLabel }), nodeField(node, 'success_condition'), node.id);
        }
      }
      const schema = node.output?.schema;
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
    if (!['success', 'failure', 'invalid'].includes(edge.on)) addIssue(t('workflowEditor.validationEdgeOutcomeRequired', { index: index + 1 }), edgeField(index, 'on'), undefined, index);
    else if (edge.from.trim()) {
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
