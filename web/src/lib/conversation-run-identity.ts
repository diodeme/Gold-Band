export interface ConversationTaskIdentityLocator {
  projectId: string;
  taskId: string;
  taskUuid?: string | null;
}

export interface ConversationRunIdentityLocator extends ConversationTaskIdentityLocator {
  runId: string;
}

function normalizedTaskUuid(locator: ConversationTaskIdentityLocator) {
  const taskUuid = locator.taskUuid?.trim();
  return taskUuid || null;
}

export function conversationTaskIdentityKey(locator: ConversationTaskIdentityLocator) {
  const taskUuid = normalizedTaskUuid(locator);
  return taskUuid ? JSON.stringify([locator.projectId, taskUuid]) : null;
}

export function conversationRunIdentityKey(locator: ConversationRunIdentityLocator) {
  const taskUuid = normalizedTaskUuid(locator);
  return taskUuid ? JSON.stringify([locator.projectId, taskUuid, locator.runId]) : null;
}

export function sameConversationTaskEntity(
  left: ConversationTaskIdentityLocator,
  right: ConversationTaskIdentityLocator,
) {
  const leftKey = conversationTaskIdentityKey(left);
  return leftKey !== null && leftKey === conversationTaskIdentityKey(right);
}

export function sameConversationRunEntity(
  left: ConversationRunIdentityLocator,
  right: ConversationRunIdentityLocator,
) {
  const leftKey = conversationRunIdentityKey(left);
  return leftKey !== null && leftKey === conversationRunIdentityKey(right);
}

/**
 * Resolves a readable/deep-link locator against a canonical Run snapshot.
 * A supplied task UUID is authoritative; an omitted UUID is accepted only for
 * the initial backend lookup and must never be used to restore client cache.
 */
export function conversationRunLocatorResolvesTo(
  locator: ConversationRunIdentityLocator,
  run: ConversationRunIdentityLocator,
) {
  return locator.projectId === run.projectId
    && locator.taskId === run.taskId
    && locator.runId === run.runId
    && (!normalizedTaskUuid(locator) || sameConversationRunEntity(locator, run));
}
