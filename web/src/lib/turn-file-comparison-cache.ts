import { getFileComparison } from '@/api';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import type { FileComparisonVm, TurnFileLocatorVm } from '@/types';

export const TURN_FILE_COMPARISON_CACHE_LIMIT = 2;

type TurnFileComparisonCacheEntry =
  | { status: 'loading'; promise: Promise<FileComparisonVm> }
  | { status: 'ready'; value: FileComparisonVm };

const turnFileComparisonCache = new BoundedLruCache<string, TurnFileComparisonCacheEntry>(TURN_FILE_COMPARISON_CACHE_LIMIT);

export function turnFileComparisonCacheKey(locator: TurnFileLocatorVm, changeSetId: string, changeId: string) {
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
    changeId,
  ].join('\0');
}

export function readCachedTurnFileComparison(locator: TurnFileLocatorVm, changeSetId: string, changeId: string) {
  const entry = turnFileComparisonCache.get(turnFileComparisonCacheKey(locator, changeSetId, changeId));
  return entry?.status === 'ready' ? entry.value : null;
}

export function loadTurnFileComparison(locator: TurnFileLocatorVm, changeSetId: string, changeId: string) {
  const key = turnFileComparisonCacheKey(locator, changeSetId, changeId);
  const cached = turnFileComparisonCache.get(key);
  if (cached?.status === 'ready') return Promise.resolve(cached.value);
  if (cached?.status === 'loading') return cached.promise;

  let loadingEntry: Extract<TurnFileComparisonCacheEntry, { status: 'loading' }>;
  const promise = getFileComparison(locator, changeSetId, changeId)
    .then((value) => {
      if (turnFileComparisonCache.peek(key) === loadingEntry) {
        turnFileComparisonCache.set(key, { status: 'ready', value });
      }
      return value;
    })
    .catch((reason: unknown) => {
      if (turnFileComparisonCache.peek(key) === loadingEntry) {
        turnFileComparisonCache.delete(key);
      }
      throw reason;
    });
  loadingEntry = { status: 'loading', promise };
  turnFileComparisonCache.set(key, loadingEntry);
  return promise;
}

export function clearTurnFileComparisonCacheForTests() {
  turnFileComparisonCache.clear();
}
