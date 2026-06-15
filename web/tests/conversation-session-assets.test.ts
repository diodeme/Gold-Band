import { describe, expect, it } from 'vitest';
import { conversationAssetsForLeaf } from '@/lib/conversation-session-assets';
import type { AssetItemVm, ConversationSessionLeafVm } from '@/types';

function leaf(overrides: Partial<ConversationSessionLeafVm> = {}): ConversationSessionLeafVm {
  return {
    roundId: 'round-001',
    nodeId: '测试',
    attemptId: 'attempt-002',
    outerNodeId: null,
    outerAttemptId: null,
    pathLabel: '测试/attempt-002',
    status: 'completed',
    outcome: 'success',
    runtimeDisplay: {
      code: 'success',
      tone: 'success',
      icon: 'check',
      terminal: true,
      resumable: false,
      reasonCode: null,
      blockingError: false,
    },
    current: false,
    startedAt: '2026-06-15T00:00:00Z',
    finishedAt: '2026-06-15T00:00:01Z',
    sessionId: null,
    artifactCount: 1,
    attachmentCount: 1,
    ...overrides,
  };
}

function asset(name: string, overrides: Partial<AssetItemVm> = {}): AssetItemVm {
  return {
    kind: 'artifact',
    name,
    title: name,
    tone: 'accent',
    preview: name,
    roundId: 'round-001',
    nodeId: '测试',
    attemptId: 'attempt-002',
    ...overrides,
  };
}

describe('conversationAssetsForLeaf', () => {
  it('keeps files that belong to the selected session leaf', () => {
    const items = [
      asset('测试-result'),
      asset('other-result', { nodeId: '验收', attemptId: 'attempt-001' }),
    ];

    expect(conversationAssetsForLeaf(items, leaf()).map((item) => item.name)).toEqual(['测试-result']);
  });

  it('returns no files without a selected leaf', () => {
    expect(conversationAssetsForLeaf([asset('测试-result')], null)).toEqual([]);
  });
});
