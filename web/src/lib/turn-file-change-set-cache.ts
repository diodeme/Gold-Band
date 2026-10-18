import { getTurnFileChangeSet } from '@/api';
import type { TurnFileChangeSetVm, TurnFileLocatorVm } from '@/types';
import { BoundedLruCache } from './bounded-lru-cache';

const TURN_FILE_CHANGE_SET_CACHE_LIMIT = 96;

type TurnFileChangeSetCacheEntry =
  | { status: 'loading'; promise: Promise<TurnFileChangeSetVm> }
  | { status: 'ready'; value: TurnFileChangeSetVm };

const turnFileChangeSetCache = new BoundedLruCache<string, TurnFileChangeSetCacheEntry>(TURN_FILE_CHANGE_SET_CACHE_LIMIT);

export function fileChangeSetOwnerBranch(raw: unknown): string | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null;
  const record = raw as Record<string, unknown>;
  const meta = record._meta && typeof record._meta === 'object' && !Array.isArray(record._meta)
    ? record._meta as Record<string, unknown>
    : null;
  const conversation = meta?.conversation && typeof meta.conversation === 'object' && !Array.isArray(meta.conversation)
    ? meta.conversation as Record<string, unknown>
    : null;
  if (typeof conversation?.branchId === 'string' && conversation.branchId.length > 0) {
    return conversation.branchId;
  }
  const goldBand = meta?.goldBandConversation && typeof meta.goldBandConversation === 'object' && !Array.isArray(meta.goldBandConversation)
    ? meta.goldBandConversation as Record<string, unknown>
    : null;
  if (typeof goldBand?.branchId === 'string' && goldBand.branchId.length > 0) {
    return goldBand.branchId;
  }
  return typeof record.branchId === 'string' && record.branchId.length > 0 ? record.branchId : null;
}

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

export function clearTurnFileChangeSetCacheForTests() {
  turnFileChangeSetCache.clear();
}
