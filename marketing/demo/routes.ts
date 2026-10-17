import type { ConversationSessionLocator } from '@/lib/conversation-navigation';
import type { ConversationPage, ConversationRunModeVm } from '@/types';
import { DEMO_PROJECT_ID, DEMO_RUN_ID, DEMO_TASKS } from './fixtures';

export function demoPageFromHash(hash: string): ConversationPage {
  const value = hash.replace(/^#/, '').split('?')[0];
  if (value === 'multica-tasks' || value === 'scheduled-tasks' || value === 'scheduled-task-create') return { kind: value };
  if (value === 'scheduled-task-detail') return { kind: value, projectId: DEMO_PROJECT_ID, scheduledTaskId: demoLinkParameters(hash).get('id') ?? 'demo-daily' };
  if (value === 'contexts' || value === 'settings' || value === 'run-mode-management' || value === 'agents' || value === 'conversation-home') return { kind: value };
  const taskId = value || DEMO_TASKS[0];
  const requestedRun = demoLinkParameters(hash).get('run');
  const runId = requestedRun ?? DEMO_RUN_ID;
  return { kind: 'conversation-run', projectId: demoLinkParameters(hash).get('project') ?? DEMO_PROJECT_ID, taskId, runId };
}

export function demoLinkParameters(hash: string) {
  return new URLSearchParams(hash.split('?')[1] ?? '');
}

export function demoHashForPage(page: ConversationPage, session?: ConversationSessionLocator) {
  if (page.kind === 'conversation-run') {
    const params = new URLSearchParams({ project: page.projectId, run: page.runId });
    if (session) {
      params.set('round', session.roundId); params.set('node', session.nodeId); params.set('attempt', session.attemptId);
      if (session.outerNodeId) params.set('outerNode', session.outerNodeId);
      if (session.outerAttemptId) params.set('outerAttempt', session.outerAttemptId);
    }
    return `${page.taskId}?${params}`;
  }
  if (page.kind === 'scheduled-task-detail') return `${page.kind}?${new URLSearchParams({ id: page.scheduledTaskId })}`;
  return page.kind;
}

export function embeddedDemoParentHref(parentHref: string, demoHash: string): string | null {
  const url = new URL(parentHref);
  if (!url.pathname.replace(/\/+$/, '').endsWith('/demo')) return null;
  const hash = demoHash.startsWith('#') ? demoHash : `#${demoHash}`;
  return `${url.origin}${url.pathname}${url.search}${hash}`;
}

export function publishEmbeddedDemoHash(demoHash: string) {
  if (typeof window === 'undefined' || window.parent === window) return;
  let parentHref: string;
  try { parentHref = window.parent.location.href; } catch { return; }
  const next = embeddedDemoParentHref(parentHref, demoHash);
  if (!next || next === parentHref) return;
  window.parent.history.replaceState(null, '', next);
}

export function demoRunModeFromHash(hash: string): ConversationRunModeVm {
  const params = demoLinkParameters(hash);
  const mode = params.get('mode');
  return { mode: mode === 'workflow' || mode === 'auto' || mode === 'direct' ? mode : params.has('template') ? 'workflow' : 'direct', directConfig: { agentType: 'claude-acp' }, workflowTemplateId: params.get('template') === 'default-lightweight' ? 'default-lightweight' : 'default' };
}
