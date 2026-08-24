import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import type { ConversationSessionFollowMode } from '@/lib/conversation-session-follow';
import type { ConversationPage, ConversationRunVm } from '@/types';

export const CONVERSATION_RUN_CACHE_LIMIT = 12;

export type ConversationSessionTreeExpansion = Readonly<Record<string, boolean>>;

type ConversationRunLocator = Pick<ConversationRunVm, 'projectId' | 'taskId' | 'runId'>
  | Extract<ConversationPage, { kind: 'conversation-run' }>;

export interface ConversationRunViewState {
  followMode: ConversationSessionFollowMode;
  selectedSessionKey: string | null;
  sessionTreeExpansion: ConversationSessionTreeExpansion;
}

export interface ConversationRunCacheEntry {
  run: ConversationRunVm;
  viewState: ConversationRunViewState;
}

export function conversationRunCacheKey(locator: ConversationRunLocator) {
  return `${locator.projectId}:${locator.taskId}:${locator.runId}`;
}

export class ConversationRunCache {
  private readonly entries = new BoundedLruCache<string, ConversationRunCacheEntry>(
    CONVERSATION_RUN_CACHE_LIMIT,
  );

  restore(locator: ConversationRunLocator) {
    return this.restoreEntry(locator)?.run ?? null;
  }

  restoreEntry(locator: ConversationRunLocator) {
    return this.entries.get(conversationRunCacheKey(locator)) ?? null;
  }

  peekViewState(locator: ConversationRunLocator) {
    return this.entries.peek(conversationRunCacheKey(locator))?.viewState ?? null;
  }

  store(run: ConversationRunVm, viewState?: Partial<ConversationRunViewState>) {
    const key = conversationRunCacheKey(run);
    const current = this.entries.peek(key);
    const canonicalRun = {
      ...run,
    } as ConversationRunVm & { title?: string; autoTitle?: boolean };
    delete canonicalRun.title;
    delete canonicalRun.autoTitle;
    this.entries.set(key, {
      run: canonicalRun,
      viewState: {
        followMode: 'auto',
        selectedSessionKey: run.sessionTree.selectedSessionKey ?? null,
        sessionTreeExpansion: {},
        ...current?.viewState,
        ...viewState,
      },
    });
  }

  updateViewState(run: ConversationRunVm, patch: Partial<ConversationRunViewState>) {
    const key = conversationRunCacheKey(run);
    const current = this.entries.peek(key);
    if (!current) {
      this.store(run, patch);
      return;
    }
    this.entries.set(key, {
      ...current,
      viewState: { ...current.viewState, ...patch },
    });
  }
}
