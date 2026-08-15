import { describe, expect, it } from 'vitest';
import {
  CONVERSATION_RUN_CACHE_LIMIT,
  ConversationRunCache,
  conversationRunCacheKey,
} from '@/lib/conversation-run-cache';
import type { ConversationRunVm } from '@/types';

function run(index: number): ConversationRunVm {
  return {
    projectId: 'project-1',
    taskId: `task-${index}`,
    runId: `run-${index}`,
    title: `Run ${index}`,
    sessionTree: { rounds: [], selectedSessionKey: null },
  } as ConversationRunVm;
}

describe('ConversationRunCache', () => {
  it('restores a run only for the complete project/task/run identity', () => {
    const cache = new ConversationRunCache();
    cache.store(run(1));

    expect(cache.restore({
      kind: 'conversation-run',
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
    })?.title).toBe('Run 1');
    expect(cache.restore({
      kind: 'conversation-run',
      projectId: 'project-2',
      taskId: 'task-1',
      runId: 'run-1',
    })).toBeNull();
  });

  it('evicts the least recently used run at the shared finite limit', () => {
    const cache = new ConversationRunCache();
    for (let index = 0; index <= CONVERSATION_RUN_CACHE_LIMIT; index += 1) {
      cache.store(run(index));
    }

    expect(cache.restore(run(0))).toBeNull();
    expect(cache.restore(run(CONVERSATION_RUN_CACHE_LIMIT))?.runId)
      .toBe(`run-${CONVERSATION_RUN_CACHE_LIMIT}`);
  });
});

describe('conversationRunCacheKey', () => {
  it('includes every canonical run identity field', () => {
    expect(conversationRunCacheKey(run(3))).toBe('project-1:task-3:run-3');
  });
});
