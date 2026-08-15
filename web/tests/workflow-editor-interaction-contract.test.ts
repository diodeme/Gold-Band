import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { getSmoothStepPath, Position } from '@xyflow/react';
import {
  createAuthoringFlowProjection,
  createAuthoringGraphLayout,
  nodeSupportsFailureOutcome,
  recordWorkflowHistory,
  redoWorkflowHistory,
  removeTerminalFromWorkflow,
  undoWorkflowHistory,
  validateWorkflowForSave,
} from '@/components/WorkflowEditor';
import type { WorkflowDsl, WorkflowWorkerNodeDsl } from '@/types';
import {
  NODE_HEIGHT,
  NODE_WIDTH,
  WORKFLOW_EDGE_LABEL_HEIGHT,
  WORKFLOW_EDGE_LABEL_WIDTH,
} from '@/components/workflowGraph';

const editorSource = readFileSync(fileURLToPath(new URL('../src/components/WorkflowEditor.tsx', import.meta.url)), 'utf8');
const runtimeGraphSource = readFileSync(fileURLToPath(new URL('../src/components/GraphView.tsx', import.meta.url)), 'utf8');
const stylesSource = readFileSync(fileURLToPath(new URL('../src/styles.css', import.meta.url)), 'utf8');

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

function orthogonalSegmentIntersectsRect(
  start: { x: number; y: number },
  end: { x: number; y: number },
  rect: { left: number; right: number; top: number; bottom: number },
) {
  if (start.x === end.x) {
    return start.x >= rect.left && start.x <= rect.right
      && Math.max(Math.min(start.y, end.y), rect.top) <= Math.min(Math.max(start.y, end.y), rect.bottom);
  }
  if (start.y === end.y) {
    return start.y >= rect.top && start.y <= rect.bottom
      && Math.max(Math.min(start.x, end.x), rect.left) <= Math.min(Math.max(start.x, end.x), rect.right);
  }
  return true;
}

function labelRect(point: { x: number; y: number }) {
  return {
    left: point.x - WORKFLOW_EDGE_LABEL_WIDTH / 2,
    right: point.x + WORKFLOW_EDGE_LABEL_WIDTH / 2,
    top: point.y - WORKFLOW_EDGE_LABEL_HEIGHT / 2,
    bottom: point.y + WORKFLOW_EDGE_LABEL_HEIGHT / 2,
  };
}

function rectsOverlap(
  left: { left: number; right: number; top: number; bottom: number },
  right: { left: number; right: number; top: number; bottom: number },
) {
  return left.left < right.right && left.right > right.left && left.top < right.bottom && left.bottom > right.top;
}

const t = (key: string) => key;

describe('workflow editor interaction contracts', () => {
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

    const beforeLayout = createAuthoringGraphLayout(before);
    const afterLayout = createAuthoringGraphLayout(after);

    expect([...afterLayout.layoutPositions]).toEqual([...beforeLayout.layoutPositions]);
    expect([...afterLayout.branchRouteByEdgeIndex]).toEqual([...beforeLayout.branchRouteByEdgeIndex]);
  });

  it('routes a forward failure branch around nodes on the success path', () => {
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
    const layout = createAuthoringGraphLayout(value);
    const route = layout.branchRouteByEdgeIndex.get(2);
    const test = layout.layoutPositions.get('test');
    const accept = layout.layoutPositions.get('accept');

    expect(route).toBeDefined();
    expect(test).toBeDefined();
    expect(accept).toBeDefined();
    const acceptRect = {
      left: accept!.x - NODE_WIDTH / 2,
      right: accept!.x + NODE_WIDTH / 2,
      top: accept!.y - NODE_HEIGHT / 2,
      bottom: accept!.y + NODE_HEIGHT / 2,
    };
    const crossesAccept = route!.points.slice(1).some((point, index) => orthogonalSegmentIntersectsRect(route!.points[index], point, acceptRect));
    expect(crossesAccept).toBe(false);
    expect(route!.labelX < acceptRect.left || route!.labelX > acceptRect.right || route!.labelY < acceptRect.top || route!.labelY > acceptRect.bottom).toBe(true);

    const [, successLabelX, successLabelY] = getSmoothStepPath({
      sourceX: test!.x + NODE_WIDTH / 2,
      sourceY: test!.y + NODE_HEIGHT * (0.34 - 0.5),
      sourcePosition: Position.Right,
      targetX: accept!.x - NODE_WIDTH / 2,
      targetY: accept!.y,
      targetPosition: Position.Left,
    });
    const successLabelRect = labelRect({ x: successLabelX, y: successLabelY });
    const failureLabelRect = labelRect({ x: route!.labelX, y: route!.labelY });
    const sourceRect = {
      left: test!.x - NODE_WIDTH / 2,
      right: test!.x + NODE_WIDTH / 2,
      top: test!.y - NODE_HEIGHT / 2,
      bottom: test!.y + NODE_HEIGHT / 2,
    };
    expect(rectsOverlap(failureLabelRect, successLabelRect)).toBe(false);
    expect(rectsOverlap(failureLabelRect, sourceRect)).toBe(false);
    expect(route!.points.slice(1).some((point, index) => orthogonalSegmentIntersectsRect(route!.points[index], point, successLabelRect))).toBe(false);
  });

  it('projects every authoring edge through the shared routed renderer and semantic source handle', () => {
    const value = workflow();
    const projection = createAuthoringFlowProjection(value, createAuthoringGraphLayout(value), null, null, new Set(), new Map(), t);

    expect(projection.edges.every((edge) => edge.type === 'workflowRouted')).toBe(true);
    expect(projection.edges.map((edge) => edge.sourceHandle)).toEqual(['success', 'success', 'failure', 'success']);
    expect(projection.edges.every((edge) => edge.className?.includes('workflow-edge-flow'))).toBe(true);
    expect(projection.edges[0].data?.route).toBeUndefined();
    expect(projection.edges[2].data?.route).toBeDefined();
    expect(editorSource).toContain('const path = route?.path ?? smoothPath;');
    expect(runtimeGraphSource).toContain('const path = route?.path ?? smoothPath;');
  });

  it('derives failure handles from AI output validation instead of node kind', () => {
    const value = workflow();
    const projection = createAuthoringFlowProjection(value, createAuthoringGraphLayout(value), null, null, new Set(), new Map(), t);

    expect(nodeSupportsFailureOutcome(value.nodes.find((node) => node.id === 'plan'))).toBe(false);
    expect(nodeSupportsFailureOutcome(value.nodes.find((node) => node.id === 'review'))).toBe(true);
    expect(projection.nodes.find((node) => node.id === 'plan')?.data.supportsFailureOutcome).toBe(false);
    expect(projection.nodes.find((node) => node.id === 'review')?.data.supportsFailureOutcome).toBe(true);

    const invalid = workflow({ edges: [...value.edges, { from: 'plan', to: 'review', on: 'failure' }] });
    const validation = validateWorkflowForSave(invalid, [], [], t);
    expect(validation.issues.some((issue) => issue.message === 'workflowEditor.validationFailureOutcomeRequiresOutputValidation')).toBe(true);
  });

  it('selects terminal projections without a node toolbar and deletes their incoming edges as one domain operation', () => {
    const value = workflow();
    const layout = createAuthoringGraphLayout(value, new Set(['$end']));
    const projection = createAuthoringFlowProjection(value, layout, null, null, new Set(), new Map(), t, undefined, undefined, '$end');
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
    const layout = createAuthoringGraphLayout(value, new Set(['$end']));
    const selectedNode = createAuthoringFlowProjection(value, layout, 'plan', null, new Set(), new Map(), t)
      .nodes.find((node) => node.id === 'plan');
    const selectedTerminal = createAuthoringFlowProjection(value, layout, null, null, new Set(), new Map(), t, undefined, undefined, '$end')
      .nodes.find((node) => node.id === '$end');

    expect(selectedNode?.className).toContain('workflow-node-selected');
    expect(selectedTerminal?.className).toContain('workflow-node-selected');
    expect(stylesSource).toContain('.workflow-terminal-node.workflow-node-selected');
    expect(editorSource).not.toContain("item.terminal && selected && 'rounded-full ring-2");
  });

  it('centers a single outcome handle and splits validation outcomes symmetrically', () => {
    expect(editorSource).toContain("const WORKFLOW_NODE_SINGLE_OUTCOME_TOP = '50%';");
    expect(editorSource).toContain("const WORKFLOW_NODE_SPLIT_OUTCOME_TOP = { success: '34%', failure: '66%' } as const;");
    expect(editorSource).toContain('data.supportsFailureOutcome ? WORKFLOW_NODE_SPLIT_OUTCOME_TOP.success : WORKFLOW_NODE_SINGLE_OUTCOME_TOP');
    expect(editorSource).toContain('style={{ top: WORKFLOW_NODE_SPLIT_OUTCOME_TOP.failure }}');
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
    const projection = createAuthoringFlowProjection(value, createAuthoringGraphLayout(value), null, null, new Set(), new Map(), t);

    expect(editorSource).toContain('const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);');
    expect(editorSource).not.toContain('setSelectedNodeId(initialWorkflow.nodes[0]?.id ?? null);');
    expect(editorSource).not.toContain('setSelectedNodeId((current) => result.workflow.nodes.some');
    expect(undoBlock).toContain('clearCanvasSelection();');
    expect(redoBlock).toContain('clearCanvasSelection();');
    expect(projection.nodes.every((node) => node.selected === false && node.data.selected === false)).toBe(true);
  });

  it('preserves the viewport, exposes keyboard/connection affordances, and lazily refreshes JSON', () => {
    expect(editorSource).toContain('viewport?: Viewport');
    expect(editorSource).toContain('[visibleTerminalSignature, workflowTopologySignature]');
    expect(editorSource).toContain('onMoveEnd={handleMoveEnd}');
    expect(editorSource).toContain('defaultViewport={viewportRef.current}');
    expect(editorSource).toContain('connectOnClick');
    expect(editorSource).toContain('connectionRadius={32}');
    expect(editorSource).toContain('<NodeToolbar');
    expect(editorSource).toContain('<MiniMap');
    expect(editorSource).toContain('<DropdownMenu>');
    expect(editorSource).toContain("aria-label={t('workflowEditor.addNode')}");
    expect(editorSource).toContain('<InspectorCollapsible');
    expect(editorSource).toContain('data.supportsFailureOutcome ?');
    expect(editorSource).toContain("event.key.toLowerCase() === 'z'");
    expect(editorSource).toContain("if (nextTab === 'json')");
    expect(editorSource).not.toContain('setJsonDraft(JSON.stringify(normalizedNext, null, 2))');
  });

  it('keeps authoring and runtime labels in the same opaque foreground layer without disabling edge flow', () => {
    const foregroundClass = 'workflow-edge-label pointer-events-none absolute z-20 rounded-full border bg-background';
    expect(editorSource).toContain(foregroundClass);
    expect(runtimeGraphSource).toContain(foregroundClass);
    expect(stylesSource).toContain('.workflow-graph .react-flow__edgelabel-renderer');
    expect(stylesSource).toContain('.workflow-edge-flow .react-flow__edge-path');
    expect(stylesSource).toContain('animation: workflow-edge-flow 3.6s linear infinite;');
  });
});
