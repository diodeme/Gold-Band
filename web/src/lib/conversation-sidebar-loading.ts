import type {
  ConversationPinnedTaskPageVm,
  ConversationRunSummaryPageVm,
  ConversationSidebarBootstrapVm,
  ConversationSidebarVm,
  ConversationTaskPageVm,
  ConversationTaskRowVm,
} from '@/types';

export const CONVERSATION_TASK_PAGE_SIZE = 24;
export const CONVERSATION_RUN_PAGE_SIZE = 20;
export const CONVERSATION_SIDEBAR_TASK_WINDOW = 120;
export const CONVERSATION_SIDEBAR_RUN_WINDOW = 100;

function taskKey(task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>) {
  return JSON.stringify([task.projectId, task.taskUuid ?? task.taskId]);
}

function sameTask(
  left: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
  right: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
) {
  return taskKey(left) === taskKey(right);
}

function mergeTasks(
  first: ConversationTaskRowVm[],
  second: ConversationTaskRowVm[],
  limit = CONVERSATION_SIDEBAR_TASK_WINDOW,
) {
  const seen = new Set<string>();
  const merged: ConversationTaskRowVm[] = [];
  for (const task of [...first, ...second]) {
    const key = taskKey(task);
    if (seen.has(key)) continue;
    seen.add(key);
    merged.push(task);
    if (merged.length >= limit) break;
  }
  return merged;
}

function normalizeTask(task: ConversationTaskRowVm): ConversationTaskRowVm {
  return {
    ...task,
    runs: task.runs ?? [],
    runHistoryStatus: task.runHistoryStatus ?? 'not-loaded',
    runsNextCursor: task.runsNextCursor ?? null,
  };
}

function pinOrderMap(bootstrap: Pick<ConversationSidebarBootstrapVm, 'pinRefs'>) {
  return new Map(bootstrap.pinRefs.map((pin, index) => [`${pin.projectId}\u0000${pin.taskId}`, index]));
}

function projectPinnedTasks(sidebar: ConversationSidebarVm) {
  const order = pinOrderMap(sidebar);
  const candidates = mergeTasks(
    Object.values(sidebar.tasksByWorkspace).flat(),
    sidebar.pinnedTasks,
    CONVERSATION_SIDEBAR_TASK_WINDOW,
  );
  return candidates
    .flatMap((task) => {
      const pinnedOrder = order.get(`${task.projectId}\u0000${task.taskId}`);
      return pinnedOrder === undefined ? [] : [{ ...task, pinned: true, pinnedOrder }];
    })
    .sort((left, right) => (left.pinnedOrder ?? 0) - (right.pinnedOrder ?? 0));
}

export function createEmptyConversationSidebar(): ConversationSidebarVm {
  return {
    loadStatus: 'not-loaded',
    workspaces: [],
    pinRefs: [],
    pinnedTasks: [],
    pinnedTaskPage: { status: 'not-loaded', nextCursor: null },
    tasksByWorkspace: {},
    workspaceTaskPages: {},
    lastActiveWorkspaceId: null,
    preferences: {},
  };
}

export function beginConversationSidebarBootstrap(sidebar: ConversationSidebarVm): ConversationSidebarVm {
  return { ...sidebar, loadStatus: 'loading' };
}

export function failConversationSidebarBootstrap(sidebar: ConversationSidebarVm): ConversationSidebarVm {
  return { ...sidebar, loadStatus: 'error' };
}

export function applyConversationSidebarBootstrap(
  sidebar: ConversationSidebarVm,
  bootstrap: ConversationSidebarBootstrapVm,
): ConversationSidebarVm {
  const projectIds = new Set(bootstrap.workspaces.map((workspace) => workspace.projectId));
  const tasksByWorkspace = Object.fromEntries(bootstrap.workspaces.map((workspace) => [
    workspace.projectId,
    sidebar.tasksByWorkspace[workspace.projectId] ?? [],
  ]));
  const workspaceTaskPages = Object.fromEntries(bootstrap.workspaces.map((workspace) => [
    workspace.projectId,
    sidebar.workspaceTaskPages[workspace.projectId] ?? { status: 'not-loaded', nextCursor: null },
  ]));
  const next: ConversationSidebarVm = {
    ...sidebar,
    loadStatus: bootstrap.workspaces.length === 0 ? 'ready-empty' : 'ready',
    workspaces: bootstrap.workspaces,
    pinRefs: bootstrap.pinRefs,
    pinnedTasks: sidebar.pinnedTasks.filter((task) => projectIds.has(task.projectId)),
    pinnedTaskPage: bootstrap.pinRefs.length === 0
      ? { status: 'ready-empty', nextCursor: null }
      : { status: 'not-loaded', nextCursor: null },
    tasksByWorkspace,
    workspaceTaskPages,
    lastActiveWorkspaceId: bootstrap.lastActiveWorkspaceId ?? null,
    preferences: bootstrap.preferences,
  };
  return { ...next, pinnedTasks: projectPinnedTasks(next) };
}

export function beginConversationWorkspaceTaskLoad(
  sidebar: ConversationSidebarVm,
  projectId: string,
): ConversationSidebarVm {
  if (!sidebar.workspaces.some((workspace) => workspace.projectId === projectId)) return sidebar;
  return {
    ...sidebar,
    workspaceTaskPages: {
      ...sidebar.workspaceTaskPages,
      [projectId]: {
        ...sidebar.workspaceTaskPages[projectId],
        status: 'loading',
      },
    },
  };
}

export function failConversationWorkspaceTaskLoad(
  sidebar: ConversationSidebarVm,
  projectId: string,
): ConversationSidebarVm {
  if (!sidebar.workspaceTaskPages[projectId]) return sidebar;
  return {
    ...sidebar,
    workspaceTaskPages: {
      ...sidebar.workspaceTaskPages,
      [projectId]: { ...sidebar.workspaceTaskPages[projectId], status: 'error' },
    },
  };
}

export function applyConversationTaskPage(
  sidebar: ConversationSidebarVm,
  page: ConversationTaskPageVm,
  append: boolean,
): ConversationSidebarVm {
  if (!sidebar.workspaces.some((workspace) => workspace.projectId === page.projectId)) return sidebar;
  const incoming = page.tasks.map(normalizeTask);
  const current = sidebar.tasksByWorkspace[page.projectId] ?? [];
  const tasks = append ? mergeTasks(current, incoming) : mergeTasks(incoming, current);
  const status = tasks.length === 0 && !page.nextCursor ? 'ready-empty' : 'ready';
  const next: ConversationSidebarVm = {
    ...sidebar,
    tasksByWorkspace: { ...sidebar.tasksByWorkspace, [page.projectId]: tasks },
    workspaceTaskPages: {
      ...sidebar.workspaceTaskPages,
      [page.projectId]: {
        status,
        nextCursor: tasks.length >= CONVERSATION_SIDEBAR_TASK_WINDOW ? null : page.nextCursor ?? null,
      },
    },
  };
  return { ...next, pinnedTasks: projectPinnedTasks(next) };
}

export function beginConversationPinnedTaskLoad(sidebar: ConversationSidebarVm): ConversationSidebarVm {
  return { ...sidebar, pinnedTaskPage: { ...sidebar.pinnedTaskPage, status: 'loading' } };
}

export function failConversationPinnedTaskLoad(sidebar: ConversationSidebarVm): ConversationSidebarVm {
  return { ...sidebar, pinnedTaskPage: { ...sidebar.pinnedTaskPage, status: 'error' } };
}

export function applyConversationPinnedTaskPage(
  sidebar: ConversationSidebarVm,
  page: ConversationPinnedTaskPageVm,
  append: boolean,
): ConversationSidebarVm {
  const incoming = page.tasks.map(normalizeTask);
  const pinnedTasks = append ? mergeTasks(sidebar.pinnedTasks, incoming) : mergeTasks(incoming, sidebar.pinnedTasks);
  return {
    ...sidebar,
    pinnedTasks,
    pinnedTaskPage: {
      status: pinnedTasks.length === 0 && !page.nextCursor ? 'ready-empty' : 'ready',
      nextCursor: pinnedTasks.length >= CONVERSATION_SIDEBAR_TASK_WINDOW ? null : page.nextCursor ?? null,
    },
  };
}

function updateTaskCopies(
  sidebar: ConversationSidebarVm,
  predicate: (task: ConversationTaskRowVm) => boolean,
  update: (task: ConversationTaskRowVm) => ConversationTaskRowVm,
) {
  return {
    ...sidebar,
    pinnedTasks: sidebar.pinnedTasks.map((task) => predicate(task) ? update(task) : task),
    tasksByWorkspace: Object.fromEntries(Object.entries(sidebar.tasksByWorkspace).map(([projectId, tasks]) => [
      projectId,
      tasks.map((task) => predicate(task) ? update(task) : task),
    ])),
  };
}

export function beginConversationRunHistoryLoad(
  sidebar: ConversationSidebarVm,
  task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
) {
  return updateTaskCopies(sidebar, (candidate) => sameTask(candidate, task), (candidate) => ({
    ...candidate,
    runHistoryStatus: 'loading',
  }));
}

export function failConversationRunHistoryLoad(
  sidebar: ConversationSidebarVm,
  task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
) {
  return updateTaskCopies(sidebar, (candidate) => sameTask(candidate, task), (candidate) => ({
    ...candidate,
    runHistoryStatus: 'error',
  }));
}

export function applyConversationRunSummaryPage(
  sidebar: ConversationSidebarVm,
  page: ConversationRunSummaryPageVm,
  append: boolean,
) {
  const matches = (task: ConversationTaskRowVm) => task.projectId === page.projectId
    && task.taskId === page.taskId
    && (!page.taskUuid || !task.taskUuid || task.taskUuid === page.taskUuid);
  return updateTaskCopies(sidebar, matches, (task) => {
    const existingIds = new Set<string>();
    const ordered = append ? [...task.runs, ...page.runs] : [...page.runs, ...task.runs];
    const runs = ordered.filter((run) => {
      if (existingIds.has(run.runId)) return false;
      existingIds.add(run.runId);
      return true;
    }).slice(0, CONVERSATION_SIDEBAR_RUN_WINDOW);
    return {
      ...task,
      latestRun: !append && page.runs[0] ? page.runs[0] : task.latestRun,
      runs,
      runHistoryStatus: runs.length === 0 && !page.nextCursor ? 'ready-empty' : 'ready',
      runsNextCursor: runs.length >= CONVERSATION_SIDEBAR_RUN_WINDOW ? null : page.nextCursor ?? null,
    };
  });
}

export function removeConversationSidebarTask(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
) {
  return {
    ...sidebar,
    pinnedTasks: sidebar.pinnedTasks.filter((task) => task.projectId !== projectId || task.taskId !== taskId),
    tasksByWorkspace: {
      ...sidebar.tasksByWorkspace,
      [projectId]: (sidebar.tasksByWorkspace[projectId] ?? [])
        .filter((task) => task.projectId !== projectId || task.taskId !== taskId),
    },
  };
}

export class ConversationSidebarSingleFlight {
  private readonly flights = new Map<string, Promise<unknown>>();
  private readonly generations = new Map<string, number>();

  generation(key: string) {
    return this.generations.get(key) ?? 0;
  }

  isCurrent(key: string, generation: number) {
    return this.generation(key) === generation;
  }

  run<T>(key: string, request: () => Promise<T>): Promise<T> {
    const current = this.flights.get(key) as Promise<T> | undefined;
    if (current) return current;
    const flight = request().finally(() => {
      if (this.flights.get(key) === flight) this.flights.delete(key);
    });
    this.flights.set(key, flight);
    return flight;
  }

  invalidate(key: string) {
    this.generations.set(key, this.generation(key) + 1);
    this.flights.delete(key);
  }
}
