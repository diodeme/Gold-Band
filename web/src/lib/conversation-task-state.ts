import type { ConversationRecentTaskRowVm, ConversationSidebarVm, ConversationTaskRowVm } from '@/types';
import { CONVERSATION_SIDEBAR_RECENT_WINDOW, CONVERSATION_SIDEBAR_TASK_WINDOW, promoteConversationLocatedTask } from '@/lib/conversation-sidebar-loading';
import { parseTimestamp } from '@/lib/datetime';

function sameTask(
  task: Pick<ConversationTaskRowVm, 'projectId' | 'taskId'>,
  projectId: string,
  taskId: string,
) {
  return task.projectId === projectId && task.taskId === taskId;
}

export function findConversationTask(
  sidebar: ConversationSidebarVm,
  projectId: string,
  taskId: string,
) {
  return sidebar.tasksByWorkspace[projectId]?.find((task) => sameTask(task, projectId, taskId))
    ?? sidebar.pinnedTasks.find((task) => sameTask(task, projectId, taskId))
    ?? null;
}

export function applyConversationTaskSnapshot(
  sidebar: ConversationSidebarVm,
  task: ConversationTaskRowVm,
): ConversationSidebarVm {
  const workspaceTasks = sidebar.tasksByWorkspace[task.projectId] ?? [];
  const taskIndex = workspaceTasks.findIndex((candidate) => sameTask(candidate, task.projectId, task.taskId));
  const existing = taskIndex >= 0
    ? workspaceTasks[taskIndex]
    : sidebar.pinnedTasks.find((candidate) => sameTask(candidate, task.projectId, task.taskId));
  const mergedTask = existing && task.runHistoryStatus === 'not-loaded'
    ? {
        ...task,
        runs: existing.runs,
        runHistoryStatus: existing.runHistoryStatus,
        runsNextCursor: existing.runsNextCursor,
      }
    : task;
  const nextWorkspaceTasks = taskIndex >= 0
    ? workspaceTasks.map((candidate, index) => index === taskIndex ? mergedTask : candidate)
    : [mergedTask, ...workspaceTasks];
  const located = sidebar.locatedTask;
  let ordinaryCount = 0;
  const boundedWorkspaceTasks = nextWorkspaceTasks.filter((candidate) => {
    if (located && sameTask(candidate, located.projectId, located.taskId)
      && (!located.taskUuid || candidate.taskUuid === located.taskUuid)) return true;
    return ordinaryCount++ < CONVERSATION_SIDEBAR_TASK_WINDOW;
  });

  const remainingPinnedTasks = sidebar.pinnedTasks
    .filter((candidate) => !sameTask(candidate, task.projectId, task.taskId));
  const nextPinnedTasks = mergedTask.pinned
    ? [...remainingPinnedTasks, mergedTask].sort((left, right) =>
        (left.pinnedOrder ?? Number.MAX_SAFE_INTEGER) - (right.pinnedOrder ?? Number.MAX_SAFE_INTEGER))
    : remainingPinnedTasks;

  const next = {
    ...sidebar,
    pinnedTasks: nextPinnedTasks,
    recentTasks: upsertConversationRecentTask(sidebar, mergedTask),
    tasksByWorkspace: {
      ...sidebar.tasksByWorkspace,
      [task.projectId]: boundedWorkspaceTasks,
    },
  };
  const activityAdvanced = existing
    && (parseTimestamp(task.lastActivityAt)?.getTime() ?? Number.NEGATIVE_INFINITY)
      > (parseTimestamp(existing.lastActivityAt)?.getTime() ?? Number.NEGATIVE_INFINITY);
  return activityAdvanced ? promoteConversationLocatedTask(next, mergedTask) : next;
}

/**
 * Fields the memoized `RecentTaskRow` renders. Snapshots arrive as fresh
 * objects on every task refresh, so identity alone cannot detect "no visible
 * change"; only these fields justify rebuilding the row (and the list).
 */
function sameConversationRecentTaskRow(
  left: ConversationRecentTaskRowVm,
  right: ConversationRecentTaskRowVm,
) {
  return left.projectId === right.projectId
    && left.taskId === right.taskId
    && left.taskUuid === right.taskUuid
    && left.title === right.title
    && left.runMode === right.runMode
    && left.lastActivityAt === right.lastActivityAt
    && left.workspaceName === right.workspaceName
    && left.latestRun?.runId === right.latestRun?.runId
    && left.latestRun?.status === right.latestRun?.status
    && left.latestRun?.outcome === right.latestRun?.outcome
    && left.agentIdentity?.iconKey === right.agentIdentity?.iconKey
    && left.agentIdentity?.displayName === right.agentIdentity?.displayName;
}

/**
 * Keeps the merged recent list in sync with one authoritative task snapshot:
 * an already-listed row is updated in place; a brand-new task joins the front
 * so a conversation created in this client is immediately visible. Rows from
 * workspaces outside the loaded page set are left untouched.
 */
function upsertConversationRecentTask(
  sidebar: ConversationSidebarVm,
  task: ConversationTaskRowVm,
): ConversationRecentTaskRowVm[] {
  const recentTasks = sidebar.recentTasks ?? [];
  const index = recentTasks.findIndex((candidate) => sameTask(candidate, task.projectId, task.taskId));
  if (index >= 0) {
    const current = recentTasks[index];
    const updated: ConversationRecentTaskRowVm = { ...task, workspaceName: current.workspaceName };
    if (sameConversationRecentTaskRow(current, updated)) return recentTasks;
    const next = [...recentTasks];
    next[index] = updated;
    return next;
  }
  // Only add unseen tasks when the recent page is already materialized; a
  // not-loaded list stays empty until its own page request fills it.
  if (sidebar.recentTaskPage?.status !== 'ready') return recentTasks;
  const workspaceName = sidebar.workspaces.find((workspace) => workspace.projectId === task.projectId)?.name ?? task.projectId;
  return [{ ...task, workspaceName }, ...recentTasks].slice(0, CONVERSATION_SIDEBAR_RECENT_WINDOW);
}
