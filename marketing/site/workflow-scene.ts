import type { AcpUiEventVm, ConversationRunVm, RuntimeDisplayVm, WorkflowDsl } from '@/types';
import { createPreviewRun, previewEvents } from './fixture';
import type { Language } from './content';

export const WORKFLOW_STEPS = ['ready', 'thinking', 'tool', 'question', 'answered', 'permission', 'approved', 'false', 'repair', 'true', 'deliver', 'complete'] as const;
export type WorkflowStep = typeof WORKFLOW_STEPS[number];
export const showcaseWorkflow: WorkflowDsl = {
  version: '0.1', id: 'site-workflow', entry: 'review', control: { max_attempts: 3, max_rounds: 1 },
  nodes: [
    { id: 'review', type: 'worker', executionSlotId: 'site:review', profile: 'pf-builtin-review', output: { kind: 'json', artifact: 'review-result', schema: { result: 'boolean', reason: 'String' } }, success_condition: { expression: '$.result == true' } },
    { id: 'repair', type: 'worker', executionSlotId: 'site:repair', profile: 'pf-builtin-dev-test', output: { kind: 'json', artifact: 'repair-result', schema: { result: 'boolean', reason: 'String' } }, success_condition: { expression: '$.result == true' } },
    { id: 'deliver', type: 'worker', executionSlotId: 'site:deliver', profile: 'pf-builtin-cleanup' },
  ],
  edges: [
    { from: 'review', to: 'repair', on: 'failure' }, { from: 'review', to: 'deliver', on: 'success' },
    { from: 'repair', to: 'deliver', on: 'success' }, { from: 'repair', to: 'repair', on: 'failure', session: 'continue' },
    { from: 'deliver', to: '$end', on: 'success' },
  ],
};
const displays = {
  pending: { code: 'pending', tone: 'neutral', icon: 'dot', terminal: false, resumable: false, blockingError: false },
  running: { code: 'running', tone: 'running', icon: 'dot', terminal: false, resumable: false, blockingError: false },
  success: { code: 'success', tone: 'success', icon: 'check', terminal: true, resumable: false, blockingError: false },
  failure: { code: 'failure', tone: 'danger', icon: 'error', terminal: true, resumable: false, blockingError: false },
} satisfies Record<string, RuntimeDisplayVm>;

export function createWorkflowScene(base: ConversationRunVm, language: Language, step: WorkflowStep, selectedNodeId?: string): ConversationRunVm {
  const run = createPreviewRun(base, language, 1);
  const index = WORKFLOW_STEPS.indexOf(step);
  if (index < 0) throw { code: 'site.scene-step-invalid' };
  const en = language === 'en';
  const question = en ? 'Should file snapshots be enabled by default?' : '是否默认启用文件变更快照？';
  const questionSchema = { type: 'object', properties: { snapshots: { type: 'boolean', title: en ? 'Enable file snapshots' : '启用文件变更快照', default: true } }, required: ['snapshots'] };
  const done = step === 'complete';
  const activeNode = index >= 10 ? 'deliver' : index >= 8 ? 'repair' : 'review';
  const selectedNode = selectedNodeId ?? activeNode;
  if (!showcaseWorkflow.nodes.some(node => node.id === selectedNode)) throw { code: 'site.scene-node-invalid' };
  const seed = run.sessionTree.rounds[0];
  const seedNode = seed.nodes[0];
  const names = en ? ['Review', 'Repair', 'Deliver'] : ['审阅', '修正', '交付'];
  run.runMode = 'workflow'; run.workflowTemplateId = 'site-workflow';
  run.workflowJson = JSON.stringify(showcaseWorkflow); run.workflowStatus = 'valid'; run.workflowValid = true;
  run.runStatus = done ? 'completed' : 'running'; run.runOutcome = done ? 'success' : null;
  run.sessionTree.rounds = [{ ...seed, roundId: 'round-001', status: run.runStatus, runtimeDisplay: done ? displays.success : displays.running,
    nodes: showcaseWorkflow.nodes.map((node, i) => {
      const result = node.id === 'review' && index >= 7 ? 'failure' : node.id === 'repair' && index >= 9 || node.id === 'deliver' && done ? 'success' : null;
      const status = result ? 'completed' : node.id === activeNode ? 'running' : 'pending';
      const display = result ? displays[result] : status === 'running' ? displays.running : displays.pending;
      const leaf = structuredClone(seedNode.attempts[0]);
      Object.assign(leaf, { nodeId: node.id, roundId: 'round-001', attemptId: 'attempt-001', pathLabel: `${node.id}/attempt-001`, sessionId: `site-${node.id}`, current: node.id === activeNode, status, outcome: result, runtimeDisplay: display });
      if (leaf.lifecycle) {
        Object.assign(leaf.lifecycle.runtime, { status, phase: status, outcome: result, current: leaf.current, active: status === 'running', revision: index + 1 });
        leaf.lifecycle.control = { mode: 'runtime-controlled' };
        leaf.lifecycle.runtimeDisplay = display; leaf.lifecycle.displayStatus = display.code;
        leaf.lifecycle.acp.liveTurnActivity = status === 'running' ? 'running' : 'idle';
        leaf.lifecycle.acp.latestTurnStatus = status === 'completed' ? 'completed' : 'none';
        leaf.lifecycle.composer.canStop = status === 'running';
        leaf.lifecycle.composer.lockInput = true;
      }
      return { ...seedNode, nodeId: node.id, label: names[i], status, runtimeDisplay: display, attempts: [leaf] };
    }),
  }];
  const nodes = run.sessionTree.rounds[0].nodes;
  run.workflowGraph = {
    nodes: nodes.map(node => ({ id: node.nodeId, nodeId: node.nodeId, label: node.label, nodeType: 'worker', status: node.status, outcome: node.attempts[0].outcome, runtimeDisplay: node.runtimeDisplay, current: node.nodeId === activeNode, attemptId: 'attempt-001', artifactCount: 0, attachmentCount: 0 })),
    edges: showcaseWorkflow.edges.filter(edge => edge.to !== '$end').map(edge => ({ from: edge.from, to: edge.to, label: edge.on })),
  };
  const session = run.selectedSession!;
  const selectedLeaf = nodes.find(node => node.nodeId === selectedNode)!.attempts[0];
  Object.assign(session, { nodeId: selectedNode, roundId: 'round-001', attemptId: 'attempt-001', sessionId: `site-${selectedNode}`, status: selectedLeaf.status, stopReason: selectedLeaf.status === 'completed' ? 'end_turn' : null });
  const events: AcpUiEventVm[] = [];
  const append = (kind: string, content: string, extra: Partial<AcpUiEventVm> = {}) => events.push({ id: `site-${selectedNode}-${events.length}`, seq: events.length + 1, timestamp: session.sessionStartedAt!, kind, content, raw: {}, ...extra });
  if (selectedNode === 'review') {
  events.push(previewEvents(language)[0]);
  if (index >= 1) append('thoughtDelta', en ? 'Check that file snapshots are enabled before writing the guide.' : '先确认已启用文件快照，再整理工作区说明。');
  if (index >= 2) append('toolCall', '', { toolCallId: 'site-read', title: 'Read src/config.json', status: 'completed', raw: { toolCallId: 'site-read', title: 'Read src/config.json', status: 'completed', rawInput: { path: 'src/config.json' }, rawOutput: { snapshots: false } } });
  if (index >= 3) append('elicitationRequest', question, { id: 'site-question', status: index === 3 ? 'pending' : 'completed', raw: { message: question, requestedSchema: questionSchema } });
  if (index >= 4) append('elicitationResponse', '', { status: 'completed', raw: { elicitationId: 'site-question', action: 'accept', content: { snapshots: true } } });
  if (index >= 5) append('permissionRequest', '', { toolCallId: 'site-write', title: en ? 'Update src/config.json' : '更新 src/config.json', status: index === 5 ? 'pending' : 'completed', raw: { interactionId: 'site-permission', toolCallId: 'site-write', options: [{ optionId: 'allow-once', name: en ? 'Allow once' : '允许一次', kind: 'allow_once' }, { optionId: 'reject-once', name: en ? 'Reject' : '拒绝', kind: 'reject_once' }] } });
  if (index >= 6) append('permissionResponse', '', { status: 'completed', raw: { interactionId: 'site-permission', optionId: 'allow-once', outcome: 'selected' } });
  if (index >= 7) append('textDelta', '```json\n' + JSON.stringify({ result: false, reason: en ? 'Snapshots are off.' : '文件快照未启用。' }, null, 2) + '\n```');
  }
  if (selectedNode === 'repair' && index >= 8) {
  append('userTextDelta', en ? 'Enable snapshots and update the workspace guide.' : '启用快照，并更新工作区说明。');
  if (index >= 9) append('textDelta', '```json\n' + JSON.stringify({ result: true, reason: en ? 'Snapshots verified.' : '快照已启用，说明已更新。' }, null, 2) + '\n```');
  }
  if (selectedNode === 'deliver' && index >= 10) append('textDelta', en ? 'The review is complete. Prepare the files for delivery.' : '审阅完成，整理待交付文件。');
  session.events = events;
  session.pendingInteractions = selectedNode === 'review' && step === 'permission' ? [{ kind: 'permission', interactionId: 'site-permission', title: en ? 'Update src/config.json' : '更新 src/config.json', toolCallId: 'site-write', options: [{ optionId: 'allow-once', name: en ? 'Allow once' : '允许一次', kind: 'allow_once' }, { optionId: 'reject-once', name: en ? 'Reject' : '拒绝', kind: 'reject_once' }], raw: {} }] : [];
  if (selectedNode === 'review' && step === 'question') session.pendingInteractions = [{ kind: 'elicitation', interactionId: 'site-question', message: question, requestedSchema: questionSchema, raw: {} }];
  session.eventPage = { loadedCount: events.length, total: events.length, oldestSeq: 1, newestSeq: events.length, hasOlder: false, hasNewer: false, oldestCursor: null, newestCursor: null };
  session.diagnostics.eventCount = events.length;
  run.sessionTree.selectedSessionKey = `round-001/${selectedNode}/attempt-001`;
  run.activeSessions = nodes.flatMap(node => node.attempts.filter(leaf => leaf.lifecycle?.runtime.active));
  return run;
}
