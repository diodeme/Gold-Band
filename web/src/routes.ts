import type { PrimaryModule, TaskPage } from './types';

export interface AppRoute {
  module: PrimaryModule;
  taskPage: TaskPage;
}

export const taskListPage: TaskPage = { kind: 'task-list' };

export function routeFromPath(pathname: string): AppRoute {
  const segments = pathname.split('/').filter(Boolean).map(decodeURIComponent);
  if (segments[0] === 'settings') return { module: 'settings', taskPage: taskListPage };
  if (segments[0] === 'agents') return { module: 'agent-management', taskPage: taskListPage };
  if (segments[0] === 'contexts') return { module: 'knowledge-base', taskPage: taskListPage };
  if (segments[0] !== 'tasks') return { module: 'task-orchestration', taskPage: taskListPage };
  if (!segments[1]) return { module: 'task-orchestration', taskPage: taskListPage };
  if (segments[2] === 'workflow') return { module: 'task-orchestration', taskPage: { kind: 'workflow', taskId: segments[1] } };
  if (segments[2] === 'runs' && segments[3] && segments[4] === 'rounds' && segments[5]) {
    return { module: 'task-orchestration', taskPage: { kind: 'round-detail', taskId: segments[1], runId: segments[3], roundId: segments[5] } };
  }
  return { module: 'task-orchestration', taskPage: taskListPage };
}

export function pathFromRoute(module: PrimaryModule, taskPage: TaskPage) {
  if (module === 'settings') return '/settings';
  if (module === 'agent-management') return '/agents';
  if (module === 'knowledge-base') return '/contexts';
  if (taskPage.kind === 'workflow') return `/tasks/${encodeURIComponent(taskPage.taskId)}/workflow`;
  if (taskPage.kind === 'round-detail') {
    return `/tasks/${encodeURIComponent(taskPage.taskId)}/runs/${encodeURIComponent(taskPage.runId)}/rounds/${encodeURIComponent(taskPage.roundId)}`;
  }
  return '/tasks';
}

export function replaceRoute(module: PrimaryModule, taskPage: TaskPage) {
  updateHistory(module, taskPage, 'replace');
}

export function pushRoute(module: PrimaryModule, taskPage: TaskPage) {
  updateHistory(module, taskPage, 'push');
}

function updateHistory(module: PrimaryModule, taskPage: TaskPage, mode: 'push' | 'replace') {
  const nextPath = pathFromRoute(module, taskPage);
  if (window.location.pathname === nextPath) return;
  const nextUrl = `${nextPath}${window.location.search}${window.location.hash}`;
  if (mode === 'push') window.history.pushState(null, '', nextUrl);
  else window.history.replaceState(null, '', nextUrl);
}
