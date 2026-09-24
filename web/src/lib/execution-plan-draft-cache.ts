import { BoundedLruCache } from '@/lib/bounded-lru-cache';

export interface ExecutionPlanDraftLocator {
  projectId: string;
  taskId: string;
  taskUuid: string;
  runId: string;
}

const CACHE_LIMIT = 24;
const drafts = new BoundedLruCache<string, unknown>(CACHE_LIMIT);

export function executionPlanDraftKey(locator: ExecutionPlanDraftLocator) {
  return `${locator.projectId}/${locator.taskId}/${locator.taskUuid}/${locator.runId}`;
}

export function readExecutionPlanDraft<T>(locator: ExecutionPlanDraftLocator): T | undefined {
  return drafts.get(executionPlanDraftKey(locator)) as T | undefined;
}

export function writeExecutionPlanDraft<T>(locator: ExecutionPlanDraftLocator, value: T) {
  drafts.set(executionPlanDraftKey(locator), value);
}

export function clearExecutionPlanDraftCache() {
  drafts.deleteWhere(() => true);
}
