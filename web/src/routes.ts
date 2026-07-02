import type { ConversationPage, DesktopUiMode, PrimaryModule, TaskPage } from './types';

export interface AppRoute {
  uiMode: DesktopUiMode;
  module: PrimaryModule;
  taskPage: TaskPage;
  conversationPage: ConversationPage;
}

export const taskListPage: TaskPage = { kind: 'task-list' };
export const conversationHomePage: ConversationPage = { kind: 'conversation-home' };

export function routeFromPath(pathname: string): AppRoute {
  const segments = pathname.split('/').filter(Boolean).map(decodeURIComponent);

  // ── Conversation paths ──
  if (segments[0] === 'chat') {
    if (segments[1] === 'agents') return { uiMode: 'conversation', module: 'agent-management', taskPage: taskListPage, conversationPage: { kind: 'agents' } };
    if (segments[1] === 'contexts') return { uiMode: 'conversation', module: 'knowledge-base', taskPage: taskListPage, conversationPage: { kind: 'contexts' } };
    if (segments[1] === 'run-modes') return { uiMode: 'conversation', module: 'task-orchestration', taskPage: taskListPage, conversationPage: { kind: 'run-mode-management' } };
    if (segments[1] === 'projects' && segments[3] === 'tasks' && segments[5] === 'runs' && segments[6]) {
      return { uiMode: 'conversation', module: 'task-orchestration', taskPage: taskListPage, conversationPage: { kind: 'conversation-run', projectId: segments[2], taskId: segments[4], runId: segments[6] } };
    }
    return { uiMode: 'conversation', module: 'task-orchestration', taskPage: taskListPage, conversationPage: conversationHomePage };
  }

  // ── Workbench paths ──
  const workbenchBase: Pick<AppRoute, 'uiMode' | 'conversationPage'> = { uiMode: 'workbench', conversationPage: conversationHomePage };
  if (segments[0] === 'settings') return { ...workbenchBase, module: 'settings', taskPage: taskListPage };
  if (segments[0] === 'agents') return { ...workbenchBase, module: 'agent-management', taskPage: taskListPage };
  if (segments[0] === 'contexts') return { ...workbenchBase, module: 'knowledge-base', taskPage: taskListPage };
  if (segments[0] !== 'tasks') return { ...workbenchBase, module: 'task-orchestration', taskPage: taskListPage };
  if (!segments[1]) return { ...workbenchBase, module: 'task-orchestration', taskPage: taskListPage };
  if (segments[2] === 'workflow') return { ...workbenchBase, module: 'task-orchestration', taskPage: { kind: 'workflow', taskId: segments[1] } };
  if (segments[2] === 'runs' && segments[3] && segments[4] === 'rounds' && segments[5]) {
    return { ...workbenchBase, module: 'task-orchestration', taskPage: { kind: 'round-detail', taskId: segments[1], runId: segments[3], roundId: segments[5] } };
  }
  return { ...workbenchBase, module: 'task-orchestration', taskPage: taskListPage };
}

export function pathFromRoute(module: PrimaryModule, taskPage: TaskPage, conversationPage?: ConversationPage) {
  // ── Conversation paths ──
  if (conversationPage) {
    if (conversationPage.kind === 'agents') return '/chat/agents';
    if (conversationPage.kind === 'contexts') return '/chat/contexts';
    if (conversationPage.kind === 'run-mode-management') return '/chat/run-modes';
    if (conversationPage.kind === 'conversation-run') return `/chat/projects/${encodeURIComponent(conversationPage.projectId)}/tasks/${encodeURIComponent(conversationPage.taskId)}/runs/${encodeURIComponent(conversationPage.runId)}`;
    return '/chat';
  }
  // ── Workbench paths ──
  if (module === 'settings') return '/settings';
  if (module === 'agent-management') return '/agents';
  if (module === 'knowledge-base') return '/contexts';
  if (taskPage.kind === 'workflow') return `/tasks/${encodeURIComponent(taskPage.taskId)}/workflow`;
  if (taskPage.kind === 'round-detail') {
    return `/tasks/${encodeURIComponent(taskPage.taskId)}/runs/${encodeURIComponent(taskPage.runId)}/rounds/${encodeURIComponent(taskPage.roundId)}`;
  }
  return '/tasks';
}

export function replaceRoute(module: PrimaryModule, taskPage: TaskPage, conversationPage?: ConversationPage) {
  updateHistory(module, taskPage, 'replace', conversationPage);
}

export function pushRoute(module: PrimaryModule, taskPage: TaskPage, conversationPage?: ConversationPage) {
  updateHistory(module, taskPage, 'push', conversationPage);
}

function updateHistory(module: PrimaryModule, taskPage: TaskPage, mode: 'push' | 'replace', conversationPage?: ConversationPage) {
  const nextPath = pathFromRoute(module, taskPage, conversationPage);
  if (window.location.pathname === nextPath) return;
  const nextUrl = `${nextPath}${window.location.search}${window.location.hash}`;
  if (mode === 'push') window.history.pushState(null, '', nextUrl);
  else window.history.replaceState(null, '', nextUrl);
}
