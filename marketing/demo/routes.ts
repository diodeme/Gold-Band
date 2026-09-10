import type { ConversationPage, ConversationRunModeVm, ConversationRunVm, ConversationSessionLeafVm, ConversationTreeNodeVm } from '@/types';
import { DEMO_PROJECT_ID, DEMO_RUN_ID, DEMO_TASKS } from './fixtures';

export function demoPageFromHash(hash: string): ConversationPage {
  const value = hash.replace(/^#/, '').split('?')[0];
  if (value === 'multica-tasks' || value === 'scheduled-tasks' || value === 'scheduled-task-create') return { kind: value };
  if (value === 'scheduled-task-detail') return { kind: value, projectId: DEMO_PROJECT_ID, scheduledTaskId: demoLinkParameters(hash).get('id') ?? 'demo-daily' };
  if (value === 'contexts' || value === 'settings' || value === 'run-mode-management' || value === 'agents' || value === 'conversation-home') return { kind: value };
  const params = demoLinkParameters(hash);
  return { kind: 'conversation-run', projectId: params.get('project') ?? DEMO_PROJECT_ID,
    taskId: value || DEMO_TASKS[0], runId: params.get('run') ?? DEMO_RUN_ID };
}

const sessionFields = { round: 'roundId', node: 'nodeId', attempt: 'attemptId', outerNode: 'outerNodeId', outerAttempt: 'outerAttemptId' } as const;

export function demoSessionLink(projectId: string, taskId: string, runId: string, leaf: ConversationSessionLeafVm, branchId = 'root') {
  const params = new URLSearchParams({ project: projectId, run: runId, branch: branchId });
  for (const [key, field] of Object.entries(sessionFields)) {
    const value = leaf[field as keyof typeof leaf];
    if (typeof value === 'string') params.set(key, value);
  }
  return `${taskId}?${params}`;
}

export function selectDemoSession(run: ConversationRunVm, params: URLSearchParams): ConversationSessionLeafVm {
  const leaves: ConversationSessionLeafVm[] = [];
  const visit = (node: ConversationTreeNodeVm) => {
    leaves.push(...node.attempts);
    node.outerNodes?.forEach(visit);
  };
  run.sessionTree.rounds.forEach(round => round.nodes.forEach(visit));
  const explicit = Object.keys(sessionFields).some(key => params.has(key));
  const candidates = leaves.filter(leaf => Object.entries(sessionFields).every(([key, field]) => {
    const actual = leaf[field as keyof typeof leaf] ?? null;
    if (explicit) return params.has(key) ? actual === params.get(key) : !key.startsWith('outer') || actual === null;
    if (!run.selectedSession) return [leaf.roundId, leaf.outerNodeId, leaf.outerAttemptId, leaf.nodeId, leaf.attemptId].filter(Boolean).join('/') === run.sessionTree.selectedSessionKey;
    return actual === (run.selectedSession[field as keyof NonNullable<typeof run.selectedSession>] ?? null);
  }));
  if (candidates.length !== 1) throw { code: 'demo.resource-not-found', params: {} };
  return candidates[0];
}

export function demoLinkParameters(hash: string) {
  return new URLSearchParams(hash.split('?')[1] ?? '');
}

export function demoRunModeFromHash(hash: string, current?: ConversationRunModeVm): ConversationRunModeVm {
  const params = demoLinkParameters(hash);
  const mode = params.get('mode');
  const base: ConversationRunModeVm = current ?? { mode: 'direct', directConfig: { agentType: 'claude-acp' }, workflowTemplateId: 'default' };
  return { ...base,
    mode: mode === 'workflow' || mode === 'auto' || mode === 'direct' ? mode : params.has('template') ? 'workflow' : base.mode,
    ...(params.has('template') ? { workflowTemplateId: params.get('template') === 'default-lightweight' ? 'default-lightweight' : 'default' } : {}),
  };
}
