import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import ELK from 'elkjs/lib/elk.bundled.js';
import { Position } from '@xyflow/react';
import {
  createAuthoringFlowProjection,
  createAuthoringGraph,
  mergeBufferedNodePatches,
  nodeSupportsFailureOutcome,
  recordWorkflowHistory,
  redoWorkflowHistory,
  removeTerminalFromWorkflow,
  undoWorkflowHistory,
  validateWorkflowForSave,
} from '@/components/WorkflowEditor';
import type { WorkflowDsl, WorkflowWorkerNodeDsl } from '@/types';
import { readyProfileCatalog } from '@/lib/profile-catalog';
import {
  NEW_ROUND_NODE,
  layoutWorkflowGraph,
  workflowLayoutKey,
  type Rect,
} from '@/components/workflowGraph';

const editorSource = readFileSync(fileURLToPath(new URL('../src/components/WorkflowEditor.tsx', import.meta.url)), 'utf8');
const runtimeGraphSource = readFileSync(fileURLToPath(new URL('../src/components/GraphView.tsx', import.meta.url)), 'utf8');
const graphControlsSource = readFileSync(fileURLToPath(new URL('../src/components/GraphControls.tsx', import.meta.url)), 'utf8');
const stylesSource = readFileSync(fileURLToPath(new URL('../src/styles.css', import.meta.url)), 'utf8');
const workspaceShellSource = readFileSync(fileURLToPath(new URL('../src/components/workspace/WorkspaceShell.tsx', import.meta.url)), 'utf8');

function worker(id: string, patch: Partial<WorkflowWorkerNodeDsl> = {}): WorkflowWorkerNodeDsl {
  return { type: 'worker', id, provider: 'claude-acp', profile: 'developer', goal: `Run ${id}`, ...patch };
}

function workflow(patch: Partial<WorkflowDsl> = {}): WorkflowDsl {
  return {
    version: '0.1',
    id: 'editor-contract',
    entry: 'plan',
    control: {},
    nodes: [
      worker('plan'),
      worker('build'),
      worker('review', {
        output: { kind: 'json', artifact: 'review-result' },
        success_condition: { expression: '$.result == true' },
      }),
    ],
    edges: [
      { from: 'plan', to: 'build', on: 'success' },
      { from: 'build', to: 'review', on: 'success' },
      { from: 'review', to: 'build', on: 'failure' },
      { from: 'review', to: '$end', on: 'success' },
    ],
    ...patch,
  };
}

function segmentCrossesRect(start: { x: number; y: number }, end: { x: number; y: number }, rect: Rect) {
  const left = rect.x + 1;
  const right = rect.x + rect.width - 1;
  const top = rect.y + 1;
  const bottom = rect.y + rect.height - 1;
  if (start.y === end.y) return start.y > top && start.y < bottom && Math.max(start.x, end.x) > left && Math.min(start.x, end.x) < right;
  return start.x > left && start.x < right && Math.max(start.y, end.y) > top && Math.min(start.y, end.y) < bottom;
}

const elk = new ELK();

async function authoringLayout(value: WorkflowDsl, visibleTerminalIds: ReadonlySet<string> = new Set()) {
  const graph = createAuthoringGraph(value, visibleTerminalIds, t);
  return { graph, layout: await layoutWorkflowGraph(graph.spec, elk) };
}

const t = (key: string) => key;

describe('workflow editor interaction contracts', () => {
  it('commits buffered node fields and a rename as one patch', () => {
    expect(mergeBufferedNodePatches(
      { goal: 'Updated goal', model: 'gpt-5.6', output: { kind: 'json', artifact: 'result' } },
      { id: 'renamed-node' },
    )).toEqual({
      goal: 'Updated goal',
      model: 'gpt-5.6',
      output: { kind: 'json', artifact: 'result' },
      id: 'renamed-node',
    });
  });

  it('keeps topology layout independent from inspector-only configuration', () => {
    const before = workflow();
    const after = workflow({
      control: { max_attempts: 4, max_rounds: 2 },
      nodes: [
        worker('plan', { goal: 'A long edited goal', model: 'gpt-5.6' }),
        worker('build'),
        worker('review', {
          output: { kind: 'json', artifact: 'review-result' },
          success_condition: { expression: '$.result == true' },
        }),
      ],
    });

    expect(workflowLayoutKey(createAuthoringGraph(after, new Set(), t).spec)).toBe(workflowLayoutKey(createAuthoringGraph(before, new Set(), t).spec));
  });

  it('routes a forward failure branch around nodes on the success path', async () => {
    const value = workflow({
      entry: 'test',
      nodes: [
        worker('test', {
          output: { kind: 'json', artifact: 'test-result' },
          success_condition: { expression: '$.result == true' },
        }),
        worker('accept'),
      ],
      edges: [
        { from: 'test', to: 'accept', on: 'success' },
        { from: 'accept', to: '$end', on: 'success' },
        { from: 'test', to: '$end', on: 'failure' },
      ],
    });
    const { layout } = await authoringLayout(value);
    const route = layout.edges.get('test:$end:failure:2')!;
    const accept = layout.nodes.get('accept')!;

    expect(route.points.slice(1).some((point, index) => segmentCrossesRect(route.points[index], point, accept))).toBe(false);
    const labelInsideAccept = route.labelX > accept.x && route.labelX < accept.x + accept.width
      && route.labelY > accept.y && route.labelY < accept.y + accept.height;
    expect(labelInsideAccept).toBe(false);
  });

  it('names a single new Round restart node on the target and several restart nodes on their edges', () => {
    const translate = (key: string, options?: Record<string, unknown>) => (options ? `${key}:${JSON.stringify(options)}` : key);
    const single = workflow({
      nodes: [...workflow().nodes, worker('accept', { manual_check: true })],
      edges: [...workflow().edges, { from: 'accept', to: NEW_ROUND_NODE, on: 'failure', new_round_entry: '$entry' }],
    });
    const singleGraph = createAuthoringGraph(single, new Set(), translate);
    expect(singleGraph.nodeLabels.get(NEW_ROUND_NODE)).toBe('workflowEditor.nodeLabels.newRoundFrom:{"node":"plan"}');
    expect(singleGraph.edgeLabels.get(`accept:${NEW_ROUND_NODE}:failure:4`)).toBe('workflowEditor.edgeLabels.failure');

    const several = workflow({
      nodes: single.nodes,
      edges: [...single.edges, { from: 'review', to: NEW_ROUND_NODE, on: 'failure', new_round_entry: 'build' }],
    });
    const severalGraph = createAuthoringGraph(several, new Set(), translate);
    expect(severalGraph.nodeLabels.get(NEW_ROUND_NODE)).toBe('workflowEditor.nodeLabels.newRound');
    expect(severalGraph.edgeLabels.get(`review:${NEW_ROUND_NODE}:failure:5`))
      .toBe('workflowEditor.edgeLabels.newRoundFrom:{"outcome":"workflowEditor.edgeLabels.failure","node":"build"}');
    expect(severalGraph.spec.nodes.find((node) => node.id === NEW_ROUND_NODE)?.pinBelow).toBe('plan');
  });

  it('hides nodes until their first layout and accepts restart edges on the right of the pinned target', async () => {
    const value = workflow({
      edges: [...workflow().edges, { from: 'review', to: NEW_ROUND_NODE, on: 'failure' }],
    });
    const graph = createAuthoringGraph(value, new Set(), t);
    const pending = createAuthoringFlowProjection(value, graph, null, null, null, new Set(), new Map(), t);
    expect(pending.nodes.every((node) => node.hidden)).toBe(true);

    const { layout } = await authoringLayout(value);
    const laidOut = createAuthoringFlowProjection(value, graph, layout, null, null, new Set(), new Map(), t);
    const newRound = laidOut.nodes.find((node) => node.id === NEW_ROUND_NODE)!;
    expect(laidOut.nodes.some((node) => node.hidden)).toBe(false);
    expect(newRound.targetPosition).toBe(Position.Right);
    expect(newRound.data.targetPosition).toBe(Position.Right);
  });

  it('projects every authoring edge through the shared routed renderer and semantic source handle', async () => {
    const value = workflow();
    const { graph, layout } = await authoringLayout(value);
    const projection = createAuthoringFlowProjection(value, graph, layout, null, null, new Set(), new Map(), t);

    expect(projection.edges.every((edge) => edge.type === 'workflowRouted')).toBe(true);
    expect(projection.edges.map((edge) => edge.sourceHandle)).toEqual(['success', 'success', 'failure', 'success']);
    expect(projection.edges.every((edge) => edge.className?.includes('workflow-edge-flow'))).toBe(true);
    expect(projection.edges.every((edge) => edge.data?.route?.path)).toBe(true);
    expect(editorSource).toContain('const path = route?.path ?? smoothPath;');
    expect(runtimeGraphSource).toContain('const path = route?.path ?? smoothPath;');
  });

  it('derives failure handles from AI output validation or manual check instead of node kind', () => {
    const value = workflow({
      nodes: [
        worker('plan', { manual_check: true }),
        worker('build'),
        worker('review', {
          output: { kind: 'json', artifact: 'review-result' },
          success_condition: { expression: '$.result == true' },
        }),
      ],
    });
    const projection = createAuthoringFlowProjection(value, createAuthoringGraph(value, new Set(), t), null, null, null, new Set(), new Map(), t);

    expect(nodeSupportsFailureOutcome(value.nodes.find((node) => node.id === 'plan'))).toBe(true);
    expect(nodeSupportsFailureOutcome(value.nodes.find((node) => node.id === 'build'))).toBe(false);
    expect(nodeSupportsFailureOutcome(value.nodes.find((node) => node.id === 'review'))).toBe(true);
    expect(projection.nodes.find((node) => node.id === 'plan')?.data.supportsFailureOutcome).toBe(true);
    expect(projection.nodes.find((node) => node.id === 'build')?.data.supportsFailureOutcome).toBe(false);
    expect(projection.nodes.find((node) => node.id === 'review')?.data.supportsFailureOutcome).toBe(true);

    const manualFailure = workflow({
      nodes: value.nodes,
      edges: [...value.edges, { from: 'plan', to: 'review', on: 'failure' }],
    });
    const manualValidation = validateWorkflowForSave(manualFailure, readyProfileCatalog([]), [], t);
    expect(manualValidation.issues.some((issue) => issue.message === 'workflowEditor.validationFailureOutcomeRequiresResultDecision')).toBe(false);

    const invalid = workflow({ edges: [...value.edges, { from: 'build', to: 'plan', on: 'failure' }] });
    const validation = validateWorkflowForSave(invalid, readyProfileCatalog([]), [], t);
    expect(validation.issues.some((issue) => issue.message === 'workflowEditor.validationFailureOutcomeRequiresResultDecision')).toBe(true);
  });

  it('defers only profile reference validation while the profile catalog is loading', () => {
    const value = workflow({
      id: '',
      nodes: [
        worker('plan', { executionSlotId: 'slot-plan', profile: 'missing-profile' }),
        worker('build', { executionSlotId: 'slot-build' }),
        worker('review', {
          executionSlotId: 'slot-review',
          output: { kind: 'json', artifact: 'review-result' },
          success_condition: { expression: '$.result == true' },
        }),
      ],
    });

    const loadingValidation = validateWorkflowForSave(
      value,
      { status: 'loading', profiles: [] },
      [],
      t,
      null,
      null,
      null,
      true,
      { definitionRevision: '', bindingRevision: 0, bindings: [] },
      false,
    );
    const readyValidation = validateWorkflowForSave(
      value,
      readyProfileCatalog([]),
      [],
      t,
      null,
      null,
      null,
      true,
      { definitionRevision: '', bindingRevision: 0, bindings: [] },
      false,
    );

    expect(loadingValidation.issues.map((issue) => issue.message)).toContain('workflowEditor.validationWorkflowIdRequired');
    expect(loadingValidation.issues.map((issue) => issue.message)).not.toContain('workflowEditor.validationNodeProfileVisibilityChanged');
    expect(readyValidation.issues.map((issue) => issue.message)).toContain('workflowEditor.validationNodeProfileVisibilityChanged');

    const dynamicValue = workflow({
      nodes: [{
        id: 'dynamic',
        type: 'ai-dynamic',
        agentStrategy: { mode: 'fixed', provider: 'claude-acp' },
        allowedProfiles: ['missing-profile'],
        allowedWorkflows: [],
        control: {
          maxDynamicNodes: 20,
          maxFanout: 5,
          maxDepth: 6,
          maxParallel: 3,
          maxGroupDepth: 1,
          maxWorkflowInvocations: 10,
          allowNestedDynamic: false,
        },
      }],
      edges: [{ from: 'dynamic', to: '$end', on: 'success' }],
    });
    const dynamicLoading = validateWorkflowForSave(dynamicValue, { status: 'loading', profiles: [] }, [], t);
    const dynamicReady = validateWorkflowForSave(dynamicValue, readyProfileCatalog([]), [], t);

    expect(dynamicLoading.issues.map((issue) => issue.message)).not.toContain('workflowEditor.validationAllowedProfileMissing');
    expect(dynamicReady.issues.map((issue) => issue.message)).toContain('workflowEditor.validationAllowedProfileMissing');
  });

  it('selects terminal projections without a node toolbar and deletes their incoming edges as one domain operation', () => {
    const value = workflow();
    const graph = createAuthoringGraph(value, new Set(['$end']), t);
    const projection = createAuthoringFlowProjection(value, graph, null, null, null, new Set(), new Map(), t, undefined, undefined, '$end');
    const terminal = projection.nodes.find((node) => node.id === '$end');

    expect(terminal?.selected).toBe(true);
    expect(terminal?.className).toContain('workflow-node-selected');
    expect(terminal?.className).toContain('workflow-terminal-node');
    expect(terminal?.data.onDelete).toBeUndefined();
    const next = removeTerminalFromWorkflow(value, '$end');
    expect(next.edges.some((edge) => edge.to === '$end')).toBe(false);
    expect(value.edges.some((edge) => edge.to === '$end')).toBe(true);
    const undo = undoWorkflowHistory(recordWorkflowHistory({ past: [], future: [] }, value), next);
    expect(undo?.workflow).toEqual(value);
  });

  it('uses one selection treatment for regular and terminal nodes', () => {
    const value = workflow();
    const graph = createAuthoringGraph(value, new Set(['$end']), t);
    const selectedNode = createAuthoringFlowProjection(value, graph, null, 'plan', null, new Set(), new Map(), t)
      .nodes.find((node) => node.id === 'plan');
    const selectedTerminal = createAuthoringFlowProjection(value, graph, null, null, null, new Set(), new Map(), t, undefined, undefined, '$end')
      .nodes.find((node) => node.id === '$end');

    expect(selectedNode?.className).toContain('workflow-node-selected');
    expect(selectedTerminal?.className).toContain('workflow-node-selected');
    expect(stylesSource).toContain('.workflow-terminal-node.workflow-node-selected');
    expect(editorSource).not.toContain("item.terminal && selected && 'rounded-full ring-2");
  });

  it('centers a single outcome handle and splits validation outcomes symmetrically', () => {
    // Single-outcome exits share the split success lane (34%) so mixed
    // chains stay on one visual track with straight hops.
    expect(editorSource).toContain("const WORKFLOW_NODE_SINGLE_OUTCOME_TOP = '34%';");
    expect(editorSource).toContain("const WORKFLOW_NODE_SPLIT_OUTCOME_TOP = { success: '34%', failure: '66%' } as const;");
    expect(editorSource).toContain('data.supportsFailureOutcome ? WORKFLOW_NODE_SPLIT_OUTCOME_TOP.success : WORKFLOW_NODE_SINGLE_OUTCOME_TOP');
    expect(editorSource).toContain('const failureHandleStyle = vertical ? { left: WORKFLOW_NODE_SPLIT_OUTCOME_TOP.failure } : { top: WORKFLOW_NODE_SPLIT_OUTCOME_TOP.failure };');
    expect(editorSource).toContain('const successHandleStyle = vertical ? { left: successHandleOffset } : { top: successHandleOffset };');
    expect(editorSource).toContain('updateNodeInternals(id);');
    expect(editorSource).toContain('[data.sourcePosition, data.supportsFailureOutcome, data.targetPosition, id, updateNodeInternals]');
  });

  it('routes node, terminal, and edge deletion through the canvas toolbar', () => {
    expect(editorSource).toContain('const deleteSelectedCanvasElement = useCallback(() => {');
    expect(editorSource).toContain('else if (selectedEdgeIndex >= 0) deleteSelectedEdge();');
    expect(editorSource).toContain("t(selectedEdgeIndex >= 0 ? 'workflowEditor.deleteEdge' : 'workflowEditor.deleteNode')");
    expect(editorSource).not.toContain('onDelete={deleteSelectedEdge}');
    expect(editorSource).not.toContain("action={<Button size=\"sm\" variant=\"outline\" onClick={onDelete}");
  });

  it('keeps a single split divider and uses flat inspector sections', () => {
    expect(editorSource).toContain('border-0 bg-transparent py-0 shadow-none');
    expect(editorSource).toContain('<ResizableHandle withHandle className="mx-1 bg-border/60" />');
    expect(editorSource).toContain('overflow-hidden rounded-lg bg-muted/20');
    expect(editorSource).not.toContain('className="rounded-xl border bg-card/45"');
    expect(editorSource).not.toContain('className="rounded-xl border bg-card/30"');
  });

  it('bounds workflow history and preserves undo/redo semantics', () => {
    let history = { past: [] as WorkflowDsl[], future: [] as WorkflowDsl[] };
    let current = workflow();
    for (let index = 0; index < 75; index += 1) {
      history = recordWorkflowHistory(history, current);
      current = { ...current, id: `workflow-${index}` };
    }

    expect(history.past).toHaveLength(50);
    const undone = undoWorkflowHistory(history, current);
    expect(undone?.workflow.id).toBe('workflow-73');
    const redone = undone ? redoWorkflowHistory(undone.history, undone.workflow) : null;
    expect(redone?.workflow.id).toBe('workflow-74');
  });

  it('keeps workflow initialization and history navigation selection-neutral', () => {
    const undoBlock = editorSource.slice(editorSource.indexOf('const undoWorkflow = useCallback'), editorSource.indexOf('const redoWorkflow = useCallback'));
    const redoBlock = editorSource.slice(editorSource.indexOf('const redoWorkflow = useCallback'), editorSource.indexOf('const closeValidationDialog'));
    const value = workflow();
    const projection = createAuthoringFlowProjection(value, createAuthoringGraph(value, new Set(), t), null, null, null, new Set(), new Map(), t);

    expect(editorSource).toContain('const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);');
    expect(editorSource).not.toContain('setSelectedNodeId(initialWorkflow.nodes[0]?.id ?? null);');
    expect(editorSource).not.toContain('setSelectedNodeId((current) => result.workflow.nodes.some');
    expect(undoBlock).toContain('clearCanvasSelection();');
    expect(redoBlock).toContain('clearCanvasSelection();');
    expect(projection.nodes.every((node) => node.selected === false && node.data.selected === false)).toBe(true);
  });

  it('preserves the viewport, exposes keyboard/connection affordances, and lazily refreshes JSON', () => {
    expect(editorSource).toContain('viewport?: Viewport');
    expect(editorSource).toContain('[visibleTerminalSignature, workflowTopologySignature, layoutDirection, t]');
    expect(editorSource).toContain('onMoveEnd={handleMoveEnd}');
    expect(editorSource).toContain('defaultViewport={viewportRef.current}');
    expect(editorSource).toContain('connectOnClick');
    expect(editorSource).toContain('connectionRadius={32}');
    expect(editorSource).toContain('<NodeToolbar');
    expect(editorSource).not.toContain('<MiniMap');
    expect(editorSource).toContain('<DropdownMenu>');
    expect(editorSource).toContain("aria-label={t('workflowEditor.addNode')}");
    expect(editorSource).toContain('<InspectorCollapsible');
    expect(editorSource).toContain('data.supportsFailureOutcome ?');
    expect(editorSource).toContain("event.key.toLowerCase() === 'z'");
    expect(editorSource).toContain("if (nextTab === 'json')");
    expect(editorSource).not.toContain('setJsonDraft(JSON.stringify(normalizedNext, null, 2))');
  });

  it('keeps workflow navigation on the canvas controls without a minimap lifecycle', () => {
    expect(editorSource).toContain('<GraphControls');
    expect(runtimeGraphSource).toContain('<GraphControls');
    expect(graphControlsSource).toContain('showZoom={false} showFitView={false} showInteractive={false}');
    expect(graphControlsSource).toContain('<TooltipTrigger asChild>');
    expect(stylesSource).toContain('.workflow-graph .react-flow__controls-button svg');
    const normalizedStylesSource = stylesSource.replace(/\r\n/g, '\n');
    expect(normalizedStylesSource).toContain('fill: none;\n  stroke: currentColor;');
    expect(normalizedStylesSource).not.toContain('.workflow-graph .react-flow__controls-button svg {\n  fill: currentColor;');
    expect(editorSource).not.toContain('<MiniMap');
    expect(editorSource).not.toContain('showMiniMap');
    expect(editorSource).not.toContain('workflowGraphExceedsViewport');
    expect(editorSource).not.toContain('canvasViewportRef');
    expect(stylesSource).not.toContain('workflow-minimap');
    expect(stylesSource).not.toContain('--xy-minimap-background-color-default');
  });

  it('keeps authoring and runtime labels in the same opaque foreground layer without disabling edge flow', () => {
    const foregroundClass = 'workflow-edge-label pointer-events-none absolute z-20 rounded-full border bg-background';
    expect(editorSource).toContain(foregroundClass);
    expect(runtimeGraphSource).toContain(foregroundClass);
    expect(stylesSource).toContain('.workflow-graph .react-flow__edgelabel-renderer');
    expect(stylesSource).toContain('.workflow-edge-flow .react-flow__edge-path');
    expect(stylesSource).toContain('animation: workflow-edge-flow 3.6s linear infinite;');
  });

  it('seeds the canvas layout direction from a locked preference before the first layout', () => {
    // A vertical preference must not pay a wasted horizontal first layout plus
    // a refit; the seed mirrors GraphView's immediate-preference contract.
    expect(editorSource).toContain("useState<WorkflowLayoutDirection>(() =>");
    expect(editorSource).toContain("directionPreference === 'vertical' ? 'DOWN' : 'RIGHT')");
    expect(editorSource).toContain('useRef<WorkflowLayoutDirection>(layoutDirection)');
  });

  it('expands fullscreen editing as a viewport-level app overlay', () => {
    // The element Fullscreen API maps to a WM window-state change on
    // WebKitGTK (the WSL dev build): the compositor transition flashes
    // white over the taskbar and stalls the frame, and on WebView2
    // (Windows production) it only fills the webview anyway. Fullscreen
    // is therefore a fixed viewport overlay — the same surface the
    // runtime graphs' fullscreen dialog uses — with an opaque app
    // background and the clamped height dropped.
    // The overlay and the clamped height are mutually exclusive branches:
    // two height utilities on one element resolve by stylesheet order
    // (the arbitrary clamp sorts after h-auto in Tailwind v4), which
    // sized the overlay as the clamped strip instead of the viewport.
    expect(editorSource).toContain(
      "? 'fixed inset-0 z-50 max-h-none bg-background text-foreground'",
    );
    expect(editorSource).toContain(
      ": 'h-[clamp(520px,calc(100dvh-11rem),760px)]',",
    );
    expect(editorSource).not.toContain("editorFullscreen && 'fixed");
    expect(editorSource).not.toContain('requestFullscreen');
    expect(editorSource).not.toContain('exitFullscreen');
    expect(editorSource).not.toContain('fullscreenchange');
  });

  it('hides the editing card header inside the fullscreen overlay', () => {
    // Fullscreen is a focused canvas surface: the card header (title,
    // subtitle, canvas/JSON tabs, undo/redo, template and save actions)
    // would ride along as a strip of editing chrome — the subtitle text
    // and its bottom divider — stealing vertical space. It only renders
    // outside fullscreen; the canvas toolbar, GraphControls (with the
    // exit action) and the inspector remain.
    expect(editorSource).toMatch(/\{editorFullscreen \? null : \(\s*<CardHeader/);
  });

  it('lifts the right workspace dock above the shell while fullscreen', () => {
    // The editor mounts as a right-dock resource panel (RightWorkspaceDock
    // renders ConversationRunWorkspaceResourcePanel), so the fullscreen
    // overlay lives inside #workspace-right — a z-10 stacking context whose
    // siblings (center panel z-10, separators z-20) would paint over it.
    // While fullscreen, the editor marks the document root and a global rule
    // raises #workspace-right above every shell sibling; the overlay portal
    // host (z-50, after #root) still shades it for dialogs.
    expect(editorSource).toContain('data-workspace-editor-fullscreen');
    expect(stylesSource).toContain(':root[data-workspace-editor-fullscreen] #workspace-right');
    expect(stylesSource).not.toContain(':root[data-workspace-editor-fullscreen] #workspace-center');
  });

  it('yields the fullscreen overlay to the navigation sidebar', () => {
    // WebKitGTK renders a fixed element nested in the clipped workspace
    // panel tree differently from Chromium (the sidebar shows through the
    // overlay while its hit region still eats sidebar clicks), so the
    // overlay must not overlap the sidebar at all: the shell publishes the
    // live left-panel width and the fullscreen surface starts at its right
    // edge — the sidebar stays visible, clickable and resizable, and the
    // canvas fit excludes the sidebar strip instead of centering under it.
    expect(workspaceShellSource).toContain('--workspace-left-panel-width');
    expect(workspaceShellSource).toContain("root.style.removeProperty('--workspace-left-panel-width')");
    expect(editorSource).toContain("{ left: 'var(--workspace-left-panel-width, 0px)', top: 'var(--workspace-top-offset, 0px)' }");
  });

  it('starts the fullscreen surface below the workspace shell top', () => {
    // WebKit clips a fixed element nested in the panel tree to the group
    // box (below the 36px title bar): the canvas toolbar at m-3 lost its
    // upper half to that clip. The shell publishes the group's live top
    // offset and the fullscreen surface starts there — inside the clip box
    // on WebKit, below the title bar on every engine, no clipping either
    // way.
    expect(workspaceShellSource).toContain('--workspace-top-offset');
    expect(workspaceShellSource).toContain("root.style.removeProperty('--workspace-top-offset')");
  });

  it('refits the viewport when fullscreen toggles', () => {
    // The stable-viewport guard means a canvas resize never reframes the
    // graph: entering fullscreen carried the narrow dock's viewport into the
    // huge canvas, parking the graph in a corner. A fullscreen toggle is an
    // explicit framing action (same contract as a direction flip): the guard
    // resets and the layout is refitted for the resized canvas on both
    // enter and exit.
    expect(editorSource).toContain('previousFullscreenRef.current === editorFullscreen');
    expect(editorSource).toMatch(/\}, \[editorFullscreen, fitLayoutViewport, graphLayout, layoutDirection\]\);/);
  });

  it('exits the fullscreen overlay on Escape while fullscreen stays editor-local state', () => {
    // The editor is a stateful work surface: fullscreen drives the editor
    // container itself (toolbars, inspector and drafts travel along)
    // instead of duplicating a second editor instance inside a dialog.
    // Escape is consumed at capture so the same press does not also close
    // an open dialog or cancel an inline edit, and the page scroll is
    // locked while the overlay covers it (the fullscreen dialog's
    // contract).
    expect(editorSource).toContain('const toggleEditorFullscreen = useCallback(() => {');
    expect(editorSource).toContain('setEditorFullscreen((active) => !active)');
    expect(editorSource).toContain("event.key !== 'Escape'");
    expect(editorSource).toContain('onFullscreen={toggleEditorFullscreen}');
    expect(editorSource).toContain('fullscreenActive={editorFullscreen}');
  });
});
