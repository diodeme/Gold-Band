import { getTurnFileChangeSet } from '@/api';
import type { ConversationRunVm, TurnFileChangeSetVm, TurnFileLocatorVm } from '@/types';
import { BoundedLruCache } from './bounded-lru-cache';

const TURN_FILE_CHANGE_SET_CACHE_LIMIT = 96;
const TURN_FILE_NAVIGATION_PREFETCH_LIMIT = 12;

type TurnFileChangeSetCacheEntry =
  | { status: 'loading'; promise: Promise<TurnFileChangeSetVm> }
  | { status: 'ready'; value: TurnFileChangeSetVm };

const turnFileChangeSetCache = new BoundedLruCache<string, TurnFileChangeSetCacheEntry>(TURN_FILE_CHANGE_SET_CACHE_LIMIT);

export function turnFileChangeSetCacheKey(locator: TurnFileLocatorVm, changeSetId: string) {
  return [
    locator.projectId,
    locator.taskId,
    locator.runId,
    locator.roundId,
    locator.nodeId,
    locator.attemptId,
    locator.branchId,
    locator.outerNodeId ?? '',
    locator.outerAttemptId ?? '',
    changeSetId,
  ].join('\0');
}

export function readCachedTurnFileChangeSet(locator: TurnFileLocatorVm, changeSetId: string) {
  const entry = turnFileChangeSetCache.get(turnFileChangeSetCacheKey(locator, changeSetId));
  return entry?.status === 'ready' ? entry.value : null;
}

export function loadTurnFileChangeSet(locator: TurnFileLocatorVm, changeSetId: string) {
  const key = turnFileChangeSetCacheKey(locator, changeSetId);
  const cached = turnFileChangeSetCache.get(key);
  if (cached?.status === 'ready') return Promise.resolve(cached.value);
  if (cached?.status === 'loading') return cached.promise;
  const promise = getTurnFileChangeSet(locator, changeSetId)
    .then((value) => {
      turnFileChangeSetCache.set(key, { status: 'ready', value });
      return value;
    })
    .catch((error: unknown) => {
      turnFileChangeSetCache.delete(key);
      throw error;
    });
  turnFileChangeSetCache.set(key, { status: 'loading', promise });
  return promise;
}

export async function preloadConversationTurnFileChangeSets(run: ConversationRunVm) {
  const session = run.selectedSession;
  if (!session?.roundId || !session.nodeId || !session.attemptId) return;
  const locator: TurnFileLocatorVm = {
    projectId: run.projectId,
    taskId: run.taskId,
    runId: run.runId,
    roundId: session.roundId,
    nodeId: session.nodeId,
    attemptId: session.attemptId,
    branchId: session.branchId || 'root',
    outerNodeId: session.outerNodeId,
    outerAttemptId: session.outerAttemptId,
  };
  const changeSetIds = session.events
    .filter((event) => event.kind === 'fileChangeSet')
    .map((event) => changeSetIdFromRaw(event.raw))
    .filter((id): id is string => id !== null)
    .slice(-TURN_FILE_NAVIGATION_PREFETCH_LIMIT);
  await Promise.allSettled(changeSetIds.map((changeSetId) => loadTurnFileChangeSet(locator, changeSetId)));
}

export function clearTurnFileChangeSetCacheForTests() {
  turnFileChangeSetCache.clear();
}

function changeSetIdFromRaw(raw: unknown) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const value = (raw as Record<string, unknown>).changeSetId;
  return typeof value === 'string' && value.length > 0 ? value : null;
}
