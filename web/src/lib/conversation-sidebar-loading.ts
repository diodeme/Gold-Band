import type {
  ConversationPinnedTaskPageVm,
  ConversationRecentTaskPageVm,
  ConversationRecentTaskRowVm,
  ConversationRunSummaryPageVm,
  ConversationSidebarBootstrapVm,
  ConversationSidebarVm,
  ConversationTaskPageVm,
  ConversationTaskRowVm,
} from '@/types';
import { parseTimestamp } from '@/lib/datetime';

export const CONVERSATION_TASK_PAGE_SIZE = 7;
export const CONVERSATION_RUN_PAGE_SIZE = 7;
/** Every sidebar section pages at the same documented 7-row size (2026-11-05 由 10 下调). */
export const CONVERSATION_PINNED_TASK_PAGE_SIZE = 7;
export const CONVERSATION_SIDEBAR_TASK_WINDOW = 120;
export const CONVERSATION_SIDEBAR_RUN_WINDOW = 100;

function orderTaskRows(tasks: ConversationTaskRowVm[]) {
  return tasks.sort((left, right) =>
    (parseTimestamp(right.lastActivityAt)?.getTime() ?? 0) - (parseTimestamp(left.lastActivityAt)?.getTime() ?? 0)
    || right.taskId.localeCompare(left.taskId));
}

export function clearConversationLocatedTask(
  sidebar: ConversationSidebarVm,
  destination?: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
): ConversationSidebarVm {
  const target = sidebar.locatedTask;
  if (!target) return sidebar;
  if (destination && destination.projectId === target.projectId && destination.taskId === target.taskId
    && (!destination.taskUuid || !target.taskUuid || destination.taskUuid === target.taskUuid)) return sidebar;
  return {
    ...sidebar,
    locatedTask: null,
    tasksByWorkspace: {
      ...sidebar.tasksByWorkspace,
      [target.projectId]: (sidebar.tasksByWorkspace[target.projectId] ?? []).filter((task) => !sameTask(task, target)),
    },
  };
}

export function locateConversationSidebarTask(sidebar: ConversationSidebarVm, task: ConversationTaskRowVm): ConversationSidebarVm {
  if (!sidebar.workspaces.some((workspace) => workspace.projectId === task.projectId)) return sidebar;
  const current = sidebar.tasksByWorkspace[task.projectId] ?? [];
  if (current.some((candidate) => sameTask(candidate, task))) return sidebar;
  const cleared = clearConversationLocatedTask(sidebar);
  return {
    ...cleared,
    locatedTask: { projectId: task.projectId, taskId: task.taskId, taskUuid: task.taskUuid },
    tasksByWorkspace: {
      ...cleared.tasksByWorkspace,
      [task.projectId]: orderTaskRows([...(cleared.tasksByWorkspace[task.projectId] ?? []), normalizeTask(task)]),
    },
  };
}

function taskKey(task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>) {
  return JSON.stringify([task.projectId, task.taskUuid ?? task.taskId]);
}

function sameTask(
  left: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
  right: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
) {
  return taskKey(left) === taskKey(right);
}

/** New canonical activity makes a search-only row part of the ordinary window. */
export function promoteConversationLocatedTask(
  sidebar: ConversationSidebarVm,
  task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId' | 'taskUuid'>,
): ConversationSidebarVm {
  if (!sidebar.locatedTask || !sameTask(sidebar.locatedTask, task)) return sidebar;
  const tasks = sidebar.tasksByWorkspace[task.projectId] ?? [];
  return {
    ...sidebar,
    locatedTask: null,
    tasksByWorkspace: {
      ...sidebar.tasksByWorkspace,
      [task.projectId]: [
        ...tasks.filter((candidate) => sameTask(candidate, task)),
        ...tasks.filter((candidate) => !sameTask(candidate, task)),
      ].slice(0, CONVERSATION_SIDEBAR_TASK_WINDOW),
    },
  };
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
    recentTasks: [],
    recentTaskPage: { status: 'not-loaded', nextCursor: null },
    tasksByWorkspace: {},
    workspaceTaskPages: {},
    lastActiveWorkspaceId: null,
    preferences: {},
  };
}

export function beginConversationSidebarBootstrap(sidebar: ConversationSidebarVm): ConversationSidebarVm {
  if (sidebar.loadStatus === 'ready') return sidebar;
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
    loadStatus: 'ready',
    locatedTask: sidebar.locatedTask && projectIds.has(sidebar.locatedTask.projectId) ? sidebar.locatedTask : null,
    workspaces: bootstrap.workspaces,
    pinRefs: bootstrap.pinRefs,
    pinnedTasks: sidebar.pinnedTasks.filter((task) => projectIds.has(task.projectId)),
    pinnedTaskPage: bootstrap.pinRefs.length === 0
      ? { status: 'ready', nextCursor: null }
      : { status: 'not-loaded', nextCursor: null },
    // The workspace identity set changed; the merged recent list restarts from
    // a fresh page so removed workspaces cannot linger in it.
    recentTasks: sidebar.recentTasks.filter((task) => projectIds.has(task.projectId)),
    recentTaskPage: { status: 'not-loaded', nextCursor: null },
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
  const target = sidebar.locatedTask?.projectId === page.projectId ? sidebar.locatedTask : null;
  const extraRow = target ? incoming.find((task) => sameTask(task, target)) ?? current.find((task) => sameTask(task, target)) : null;
  const ordinary = target ? current.filter((task) => !sameTask(task, target)) : current;
  const tasks = append ? mergeTasks(ordinary, incoming) : mergeTasks(incoming, ordinary);
  const targetInPage = target && tasks.some((task) => sameTask(task, target));
  const visibleTasks = extraRow && !targetInPage ? orderTaskRows([...tasks, extraRow]) : tasks;
  const next: ConversationSidebarVm = {
    ...sidebar,
    locatedTask: targetInPage ? null : sidebar.locatedTask,
    tasksByWorkspace: { ...sidebar.tasksByWorkspace, [page.projectId]: visibleTasks },
    workspaceTaskPages: {
      ...sidebar.workspaceTaskPages,
      [page.projectId]: {
        status: 'ready',
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
      status: 'ready',
      nextCursor: pinnedTasks.length >= CONVERSATION_SIDEBAR_TASK_WINDOW ? null : page.nextCursor ?? null,
    },
  };
}

/** Merged recent rows stay ordered by the backend page order (activity desc). */
export const CONVERSATION_SIDEBAR_RECENT_WINDOW = 100;

function mergeRecentTasks(
  first: ConversationRecentTaskRowVm[],
  second: ConversationRecentTaskRowVm[],
) {
  const seen = new Set<string>();
  const merged: ConversationRecentTaskRowVm[] = [];
  for (const task of [...first, ...second]) {
    const key = taskKey(task);
    if (seen.has(key)) continue;
    seen.add(key);
    merged.push(task);
    if (merged.length >= CONVERSATION_SIDEBAR_RECENT_WINDOW) break;
  }
  return merged;
}

export function beginConversationRecentTaskLoad(sidebar: ConversationSidebarVm): ConversationSidebarVm {
  return { ...sidebar, recentTaskPage: { ...sidebar.recentTaskPage, status: 'loading' } };
}

export function failConversationRecentTaskLoad(sidebar: ConversationSidebarVm): ConversationSidebarVm {
  return { ...sidebar, recentTaskPage: { ...sidebar.recentTaskPage, status: 'error' } };
}

export function applyConversationRecentTaskPage(
  sidebar: ConversationSidebarVm,
  page: ConversationRecentTaskPageVm,
  append: boolean,
): ConversationSidebarVm {
  const recentTasks = append
    ? mergeRecentTasks(sidebar.recentTasks, page.tasks)
    : mergeRecentTasks(page.tasks, sidebar.recentTasks);
  return {
    ...sidebar,
    recentTasks,
    recentTaskPage: {
      status: 'ready',
      nextCursor: recentTasks.length >= CONVERSATION_SIDEBAR_RECENT_WINDOW ? null : page.nextCursor ?? null,
    },
  };
}

/** Collapsing a list drops everything paged beyond the default first page so
 *  reopening shows the default page size again; the next expand re-requests
 *  page one through the existing not-loaded path. */
export function resetConversationWorkspaceTaskPage(
  sidebar: ConversationSidebarVm,
  projectId: string,
): ConversationSidebarVm {
  if (!sidebar.workspaces.some((workspace) => workspace.projectId === projectId)) return sidebar;
  return {
    ...sidebar,
    tasksByWorkspace: { ...sidebar.tasksByWorkspace, [projectId]: [] },
    workspaceTaskPages: {
      ...sidebar.workspaceTaskPages,
      [projectId]: { status: 'not-loaded', nextCursor: null },
    },
  };
}

export function resetConversationRecentTaskPage(sidebar: ConversationSidebarVm): ConversationSidebarVm {
  return {
    ...sidebar,
    recentTasks: [],
    recentTaskPage: { status: 'not-loaded', nextCursor: null },
  };
}

/** Projects one task-row transform into recent rows, preserving `workspaceName`. */
export function mapConversationRecentTasks(
  recentTasks: ConversationRecentTaskRowVm[],
  transform: (task: ConversationTaskRowVm) => ConversationTaskRowVm,
): ConversationRecentTaskRowVm[] {
  if (recentTasks.length === 0) return recentTasks;
  let changed = false;
  const next = recentTasks.map((row) => {
    const transformed = transform(row);
    if (transformed === row) return row;
    changed = true;
    return { ...transformed, workspaceName: row.workspaceName };
  });
  return changed ? next : recentTasks;
}

/** Moves recent rows whose activity advanced to the merged front. */
export function promoteConversationRecentTasks(
  recentTasks: ConversationRecentTaskRowVm[],
  predicate: (task: ConversationTaskRowVm) => boolean,
): ConversationRecentTaskRowVm[] {
  if (recentTasks.length === 0) return recentTasks;
  const front = recentTasks.filter(predicate);
  if (front.length === 0) return recentTasks;
  return [...front, ...recentTasks.filter((task) => !predicate(task))];
}

function updateTaskCopies(
  sidebar: ConversationSidebarVm,
  predicate: (task: ConversationTaskRowVm) => boolean,
  update: (task: ConversationTaskRowVm) => ConversationTaskRowVm,
) {
  return {
    ...sidebar,
    pinnedTasks: sidebar.pinnedTasks.map((task) => predicate(task) ? update(task) : task),
    recentTasks: mapConversationRecentTasks(sidebar.recentTasks ?? [], (task) => predicate(task) ? update(task) : task),
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
      runHistoryStatus: 'ready',
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
    locatedTask: sidebar.locatedTask?.projectId === projectId && sidebar.locatedTask.taskId === taskId ? null : sidebar.locatedTask,
    pinnedTasks: sidebar.pinnedTasks.filter((task) => task.projectId !== projectId || task.taskId !== taskId),
    // A deleted conversation must leave every projection, including the merged
    // recent list, or it stays clickable there until the next page reload.
    recentTasks: (sidebar.recentTasks ?? []).filter((task) => task.projectId !== projectId || task.taskId !== taskId),
    tasksByWorkspace: {
      ...sidebar.tasksByWorkspace,
      [projectId]: (sidebar.tasksByWorkspace[projectId] ?? [])
        .filter((task) => task.projectId !== projectId || task.taskId !== taskId),
    },
  };
}

/**
 * Flight keys whose in-flight page loads are premise-void once a bootstrap
 * identity set lands: workspace pages, the pinned page, and the merged recent
 * page all paginate a workspace set that just changed. Invalidating them makes
 * their results no-op (generation check) and lets the bootstrap's own follow-up
 * loads start fresh instead of being deduped onto the stale flights.
 */
export function conversationSidebarBootstrapInvalidationKeys(
  sidebar: ConversationSidebarVm,
  bootstrap: ConversationSidebarBootstrapVm,
): string[] {
  const projectIds = new Set([
    ...sidebar.workspaces.map((workspace) => workspace.projectId),
    ...bootstrap.workspaces.map((workspace) => workspace.projectId),
    ...Object.keys(sidebar.tasksByWorkspace),
  ]);
  return [
    ...[...projectIds].map((projectId) => `workspace:${projectId}`),
    'pinned',
    'recent',
  ];
}
