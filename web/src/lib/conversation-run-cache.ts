import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import type { ConversationPage, ConversationRunVm } from '@/types';

export const CONVERSATION_RUN_CACHE_LIMIT = 12;

type ConversationRunLocator = Pick<ConversationRunVm, 'projectId' | 'taskId' | 'runId'>
  | Extract<ConversationPage, { kind: 'conversation-run' }>;

export function conversationRunCacheKey(locator: ConversationRunLocator) {
  return `${locator.projectId}:${locator.taskId}:${locator.runId}`;
}

export class ConversationRunCache {
  private readonly entries = new BoundedLruCache<string, ConversationRunVm>(
    CONVERSATION_RUN_CACHE_LIMIT,
  );

  restore(locator: ConversationRunLocator) {
    return this.entries.get(conversationRunCacheKey(locator)) ?? null;
  }

  store(run: ConversationRunVm) {
    this.entries.set(conversationRunCacheKey(run), run);
  }
}
