import type { RuntimeApi, AcpSessionUpdatedEventVm, ConversationRunStateUpdatedEventVm } from '@/api/client';
import type { AcpSessionVm, AcpUiEventVm, ConversationRunVm, RuntimeDisplayVm, WorkflowDsl } from '@/types';
import type { Language } from '../site/content';
import { taskTitle } from '../shared/fixture';

const IDS = { projectId: 'default', taskId: 'mock-task', runId: 'run-052', roundId: 'round-001', attemptId: 'attempt-001', outerNodeId: null, outerAttemptId: null } as const;
const NODE_IDS = ['review', 'repair', 'continue'] as const;
type NodeId = typeof NODE_IDS[number];
export type WorkflowCue = 'streaming' | 'question-wait' | 'permission-wait' | 'result-false' | 'branch' | 'result-true';
const fail = (code: string) => { throw { code: `recording.${code}`, params: {} }; };

export function workflowDefinition(): WorkflowDsl {
  return { version: '0.1', id: 'recording-workflow', entry: 'review', control: { max_attempts: 1, max_rounds: 1 },
    nodes: NODE_IDS.map(id => ({ type: 'worker', id, executionSlotId: `recording:${id}`, profile: 'pf-builtin-dev-test',
      ...(id === 'continue' ? {} : { output: { kind: 'json', artifact: 'result.json', schema: { result: 'boolean', reason: 'String' } }, success_condition: { expression: '$.result == true' } }) })),
    edges: [{ from: 'review', to: 'repair', on: 'failure' }, { from: 'review', to: 'continue', on: 'success' }, { from: 'repair', to: 'continue', on: 'success' }, { from: 'continue', to: '$end', on: 'success' }] };
}

export function createWorkflowSource(baseApi: RuntimeApi, language: Language) {
  const zh = language === 'zh';
  const labels = { review: zh ? '检查配置' : 'Review configuration', repair: zh ? '修正配置' : 'Repair configuration', continue: zh ? '补充说明' : 'Write guide' };
  const sessionListeners = new Set<(event: AcpSessionUpdatedEventVm) => void>();
  const runListeners = new Set<(event: ConversationRunStateUpdatedEventVm) => void>();
  const definition = workflowDefinition();
  const ready = baseApi.getConversationRun(IDS.projectId, IDS.taskId, IDS.runId).then(base => {
    const sessions = Object.fromEntries(NODE_IDS.map(nodeId => {
      const session = structuredClone(base.selectedSession!);
      Object.assign(session, { ...IDS, nodeId, branchId: 'root', sessionId: `recording-${nodeId}`, title: labels[nodeId],
        status: nodeId === 'review' ? 'running' : 'pending', readOnly: false, events: [], pendingInteractions: [], timelineProjection: null,
        stopReason: null, systemPromptAppend: null, timing: null, usage: null, config: null, worktreePath: null, worktreeBranch: null,
        cwd: '/default', providerCwd: '/default', sessionStartedAt: new Date().toISOString(), sessionElapsedSeconds: 0 });
      return [nodeId, session];
    })) as Record<NodeId, AcpSessionVm>;
    return { base, sessions, current: 'review' as NodeId, revision: 1, outputs: {} as Partial<Record<NodeId, { result: boolean; reason: string }>> };
  });
  type Snapshot = Awaited<typeof ready>;
  function validate(project: unknown, task: unknown, run: unknown, round?: unknown, node?: unknown, attempt?: unknown, outer?: unknown, outerAttempt?: unknown) {
    if (project !== IDS.projectId || task !== IDS.taskId || run !== IDS.runId || (round !== undefined && round !== IDS.roundId)
      || (node !== undefined && !NODE_IDS.includes(node as NodeId)) || (attempt !== undefined && attempt !== IDS.attemptId) || outer != null || outerAttempt != null) fail('locator-invalid');
  }
  function session(s: Snapshot, node: NodeId): AcpSessionVm {
    const value = structuredClone(s.sessions[node]);
    const count = value.events.length;
    value.eventPage = { generation: 1, coveredRevision: s.revision, newestRevision: s.revision, loadedCount: count, total: count, oldestSeq: count ? 1 : null, newestSeq: count || null, hasOlder: false, hasNewer: false, oldestCursor: null, newestCursor: null };
    value.diagnostics = { rawFrameCount: 0, eventCount: count, errorCount: 0, lastError: null, lastErrorTimestamp: null };
    return value;
  }
  function project(s: Snapshot, selected?: string | null): ConversationRunVm {
    const run = structuredClone(s.base);
    const round = structuredClone(run.sessionTree.rounds[0]);
    const seed = round.nodes[0];
    round.nodes = NODE_IDS.map(nodeId => {
      const current = nodeId === s.current;
      const status = s.sessions[nodeId].status;
      const done = status === 'completed';
      const outcome = done ? s.outputs[nodeId]?.result === false ? 'failure' : 'success' : null;
      const waiting = s.sessions[nodeId].pendingInteractions[0]?.kind;
      const code = done ? outcome! : status;
      const display: RuntimeDisplayVm = { code, tone: done ? outcome === 'success' ? 'success' : 'danger' : current ? 'running' : 'muted', icon: done ? 'check' : 'dot', terminal: done, resumable: false, blockingError: false };
      const leaf = { ...structuredClone(seed.attempts[0]), ...IDS, nodeId, current, status, outcome, runtimeDisplay: display,
        sessionId: s.sessions[nodeId].sessionId, sessionEstablished: status !== 'pending', pathLabel: `${nodeId}/${IDS.attemptId}`,
        artifactCount: s.outputs[nodeId] ? 1 : 0, attachmentCount: 0, manualCheckPending: false, startedAt: s.sessions[nodeId].sessionStartedAt, worktreePath: null, worktreeBranch: null, finishedAt: done ? s.sessions[nodeId].sessionUpdatedAt : null,
        lifecycle: { runtime: { status, outcome, current, active: current, continuable: false, resumable: false, phase: status, revision: s.revision },
          control: { mode: 'runtime-controlled' as const }, acp: { revision: s.revision, sessionAvailability: status === 'pending' ? 'unavailable' as const : 'established' as const, liveTurnActivity: done || status === 'pending' ? 'idle' as const : 'running' as const, latestTurnStatus: done ? 'completed' as const : 'none' as const, stopping: false },
          displayStatus: code, runtimeDisplay: display, continueKind: null,
          composer: { mode: waiting ? 'interaction-blocked' : 'runtime-active', submitTarget: waiting === 'permission' ? 'permission-response' : 'none', processingKind: waiting ? 'processing' : 'responding', canStop: current, lockInput: true } } };
      return { ...seed, nodeId, label: labels[nodeId], nodeType: 'worker', status, runtimeDisplay: display, attempts: [leaf] };
    });
    round.roundId = IDS.roundId; round.status = 'running'; round.runtimeDisplay = round.nodes.find(node => node.nodeId === s.current)!.runtimeDisplay;
    const key = selected ?? `${IDS.roundId}/${s.current}/${IDS.attemptId}`;
    const selectedNode = round.nodes.find(node => key === `${IDS.roundId}/${node.nodeId}/${IDS.attemptId}`);
    if (!selectedNode) fail('locator-invalid');
    Object.assign(run, { ...IDS, runMode: 'workflow', runStatus: 'running', runOutcome: null, pauseReason: null, resumable: false, runtimeError: null, runtimeErrorMessage: null,
      worktree: null, directConfig: null, workflowTemplateId: definition.id, workflowJson: JSON.stringify(definition), workflowStatus: 'valid', workflowValid: true, workflowError: null,
      sessionTree: { rounds: [round], selectedSessionKey: key }, selectedSession: session(s, selectedNode!.nodeId as NodeId),
      activeSessions: [round.nodes.find(node => node.nodeId === s.current)!.attempts[0]],
      workflowGraph: { nodes: round.nodes.map(node => ({ id: node.nodeId, nodeId: node.nodeId, label: node.label, nodeType: node.nodeType, status: node.status, outcome: node.attempts[0].outcome, runtimeDisplay: node.runtimeDisplay, attemptId: IDS.attemptId, artifactCount: node.attempts[0].artifactCount, attachmentCount: 0, current: node.nodeId === s.current })), edges: definition.edges.filter(edge => !edge.to.startsWith('$')).map(edge => ({ from: edge.from, to: edge.to, label: edge.on })) } });
    return run;
  }
  function append(s: Snapshot, event: Omit<AcpUiEventVm, 'id' | 'seq' | 'timestamp'>) {
    const target = s.sessions[s.current];
    const seq = target.events.length + 1;
    target.events.push({ ...event, id: `${s.current}-${seq}`, seq, timestamp: new Date().toISOString() });
  }
  function publish(s: Snapshot, previous = s.current) {
    s.revision++;
    s.sessions[s.current].sessionUpdatedAt = new Date().toISOString();
    const run = project(s);
    for (const nodeId of new Set([previous, s.current])) {
      const lifecycle = run.sessionTree.rounds[0].nodes.find(node => node.nodeId === nodeId)!.attempts[0].lifecycle;
      for (const listener of sessionListeners) listener({ ...IDS, taskUuid: s.base.taskUuid, nodeId, session: session(s, nodeId), lifecycle });
    }
    for (const listener of runListeners) listener({ ...IDS, taskUuid: s.base.taskUuid, nodeId: s.current, eventKind: 'node-started', status: run.runStatus, outcome: null });
  }
  async function advance(cue: WorkflowCue) {
    const s = await ready;
    const target = s.sessions[s.current];
    if (target.pendingInteractions.length) fail('interaction-pending');
    if (cue === 'streaming') {
      if (s.current !== 'review' || target.events.length > 2) fail('cue-invalid');
      if (!target.events.length) {
        append(s, { kind: 'thoughtDelta', status: 'completed', content: zh ? '先核对配置默认值与文档约定。' : 'Check defaults against the docs.' });
        append(s, { kind: 'textDelta', status: 'streaming', content: zh ? '正在检查配置' : 'Checking configuration' });
      } else {
        Object.assign(target.events[1], { status: 'completed', content: zh ? '正在检查配置，并确认每轮文件快照是否开启。' : 'Checking per-turn file snapshots.' });
        append(s, { kind: 'toolCall', toolCallId: 'read-config', title: 'read_file', status: 'completed', raw: { toolCallId: 'read-config', title: 'read_file', status: 'completed', rawInput: { path: 'src/config.json' }, content: [{ type: 'text', text: '{ "snapshots": false }' }] } });
      }
    } else if (cue === 'question-wait') {
      if (s.current !== 'review' || target.events.some(event => event.kind === 'elicitationRequest')) fail('cue-invalid');
      const message = zh ? '每轮都保留文件快照吗？' : 'Keep a file snapshot for every turn?';
      const requestedSchema = { type: 'object', properties: { answer: { type: 'string', title: zh ? '回答' : 'Answer' } }, required: ['answer'] };
      target.pendingInteractions = [{ kind: 'elicitation', interactionId: 'snapshot-question', message, requestedSchema, raw: {} }];
      append(s, { kind: 'elicitationRequest', status: 'pending', content: message, raw: { elicitationId: 'snapshot-question', message, requestedSchema } });
    } else if (cue === 'permission-wait') {
      if (!target.events.some(event => event.kind === 'elicitationResponse') || target.events.some(event => event.kind === 'permissionRequest')) fail('cue-invalid');
      const title = zh ? '更新 src/config.json' : 'Update src/config.json';
      const options = [{ optionId: 'allow-once', name: zh ? '允许一次' : 'Allow once', kind: 'allow_once' }, { optionId: 'reject-once', name: zh ? '拒绝' : 'Reject', kind: 'reject_once' }];
      target.pendingInteractions = [{ kind: 'permission', interactionId: 'write-config', toolCallId: 'write-config', title, options, raw: {} }];
      append(s, { kind: 'permissionRequest', title, status: 'pending', raw: { requestId: 'write-config', title, options } });
    } else if (cue === 'result-false' || cue === 'result-true') {
      const result = cue === 'result-true';
      if (s.outputs[s.current] || s.current !== (result ? 'repair' : 'review') || (!result && !target.events.some(event => event.kind === 'permissionResponse'))) fail('cue-invalid');
      const output = { result, reason: result ? zh ? '已开启每轮文件快照。' : 'Snapshots enabled.' : zh ? '配置尚未启用文件快照。' : 'Snapshots are off.' };
      s.outputs[s.current] = output;
      const jsonText = JSON.stringify(output, null, 2);
      append(s, { kind: 'textDelta', status: 'completed', content: jsonText, raw: { runtimeControlOutputDisplay: { artifactName: 'result.json', kind: 'json', jsonText, start: 0, end: jsonText.length, parseStatus: 'valid' } } });
    } else {
      const output = s.outputs[s.current];
      if (!output) fail('output-required');
      const edge = definition.edges.find(edge => edge.from === s.current && edge.on === (output!.result ? 'success' : 'failure'));
      if (!edge || !NODE_IDS.includes(edge.to as NodeId)) fail('branch-invalid');
      const previous = s.current;
      target.status = 'completed'; target.stopReason = 'end_turn';
      s.current = edge!.to as NodeId;
      s.sessions[s.current].status = 'running';
      append(s, { kind: 'textDelta', status: 'completed', content: zh ? s.current === 'repair' ? '开启文件快照，并重新检查配置。' : '配置检查通过，继续补充工作区说明。' : s.current === 'repair' ? 'Enable file snapshots and check the configuration again.' : 'Configuration passed. Continue with the workspace guide.' });
      publish(s, previous); return;
    }
    publish(s);
  }
  const api: Partial<RuntimeApi> = {
    async subscribeScheduledNotifications() { return () => {}; },
    async getTurnFileChangeSet(locator) { validate(locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, locator.outerNodeId, locator.outerAttemptId); return fail('resource-not-found'); },
    async getFileComparison(locator) { validate(locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, locator.outerNodeId, locator.outerAttemptId); return fail('resource-not-found'); },
    async resolveTurnAttachmentFile(locator) { validate(locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, locator.outerNodeId, locator.outerAttemptId); return fail('resource-not-found'); },
    async getAcpRawFrames(p, t, r, round, node, attempt, _query, outer, outerAttempt) { validate(p, t, r, round, node, attempt, outer, outerAttempt); return fail('resource-not-found'); },
    async getConversationRun(projectId, taskId, runId, selected) { validate(projectId, taskId, runId); return project(await ready, selected); },
    async getAcpSession(p, t, r, round, node, attempt, query, _fallback, outer, outerAttempt) { validate(p, t, r, round, node, attempt, outer, outerAttempt); if (query?.branchId && query.branchId !== 'root') fail('locator-invalid'); return session(await ready, node as NodeId); },
    async getAcpActivityDetail(p, t, r, round, node, attempt, _query, outer, outerAttempt) { validate(p, t, r, round, node, attempt, outer, outerAttempt); return { items: session(await ready, node as NodeId).events.filter(event => event.kind === 'toolCall'), hasMoreEarlier: false, earlierCursor: null }; },
    async getAcpToolDetail(p, t, r, round, node, attempt, query, outer, outerAttempt) { validate(p, t, r, round, node, attempt, outer, outerAttempt); return { event: session(await ready, node as NodeId).events.find(event => event.toolCallId === query.toolCallId) ?? null }; },
    async respondElicitation(p, t, r, round, node, attempt, id, action, content, outer, outerAttempt) {
      validate(p, t, r, round, node, attempt, outer, outerAttempt);
      const s = await ready;
      const interaction = s.sessions[s.current].pendingInteractions[0];
      if (node !== s.current || interaction?.kind !== 'elicitation' || interaction.interactionId !== id) fail('interaction-stale');
      if (action !== 'accept' || typeof content?.answer !== 'string' || !content.answer.trim()) fail('answer-invalid');
      s.sessions[s.current].pendingInteractions = [];
      append(s, { kind: 'elicitationResponse', status: 'completed', content: content!.answer as string, raw: { elicitationId: id, action, content } }); publish(s);
    },
    async respondAcpPermission(p, t, r, round, node, attempt, id, option, _fallback, outer, outerAttempt) {
      validate(p, t, r, round, node, attempt, outer, outerAttempt);
      const s = await ready;
      const interaction = s.sessions[s.current].pendingInteractions[0];
      if (node !== s.current || interaction?.kind !== 'permission' || interaction.interactionId !== id) fail('interaction-stale');
      if (option !== 'allow-once') fail('permission-invalid');
      s.sessions[s.current].pendingInteractions = [];
      append(s, { kind: 'permissionResponse', status: 'completed', raw: { requestId: id, optionId: option, outcome: 'selected' } }); publish(s); return session(s, s.current);
    },
    async showArtifact(p, t, r, round, node, attempt, name, outer, outerAttempt) {
      validate(p, t, r, round, node, attempt, outer, outerAttempt);
      const output = (await ready).outputs[node as NodeId];
      if (name !== 'result.json' || !output) fail('resource-not-found');
      return { title: name, kind: 'json', content: JSON.stringify(output, null, 2), metadata: {} };
    },
    async getWorkflow(taskId, projectId) {
      validate(projectId, taskId, IDS.runId);
      const original = await baseApi.getWorkflow(taskId, projectId);
      return { ...original, workflowJson: JSON.stringify(definition), modelBindings: { definitionRevision: 'recording-1', bindingRevision: 1, bindings: NODE_IDS.map(id => ({ executionSlotId: `recording:${id}`, agentId: 'claude-acp' })) } };
    },
    async getConversationTaskPage(projectId) {
      validate(projectId, IDS.taskId, IDS.runId);
      return { projectId, tasks: [{ projectId, taskId: IDS.taskId, taskUuid: (await ready).base.taskUuid, title: taskTitle(language), autoTitle: false, runMode: 'workflow', lastActivityAt: new Date().toISOString(), runs: [], runHistoryStatus: 'ready-empty', runsNextCursor: null, pinned: false }], nextCursor: null, errors: [] };
    },
    async subscribeAcpSessionUpdates(listener) { sessionListeners.add(listener); return () => { sessionListeners.delete(listener); }; },
    async subscribeConversationRunStateUpdates(listener) { runListeners.add(listener); return () => { runListeners.delete(listener); }; },
  };
  return { api, advance, snapshot: async () => project(await ready), dispose() { sessionListeners.clear(); runListeners.clear(); } };
}
