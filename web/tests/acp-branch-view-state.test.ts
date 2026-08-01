import { describe, expect, it } from 'vitest';

import { restoreAcpBranchViewState, storeAcpBranchViewState } from '@/components/acp/ACPChatDialog';

const state = (scrollTop: number) => ({
  anchorKey: `message-${scrollTop}`,
  anchorOffset: 12,
  scrollTop,
  atBottom: false,
  hasOlder: true,
  hasNewer: false,
});

describe('ACP branch view state cache', () => {
  it('stores independent scroll, cursor, and bottom-lock state per branch', () => {
    storeAcpBranchViewState('attempt:root', state(100));
    storeAcpBranchViewState('attempt:agent-a', { ...state(500), atBottom: true, hasOlder: false });
    expect(restoreAcpBranchViewState('attempt:root')).toEqual(state(100));
    expect(restoreAcpBranchViewState('attempt:agent-a')).toEqual({ ...state(500), atBottom: true, hasOlder: false });
  });

  it('evicts the least recently used branch state from the finite cache', () => {
    for (let index = 0; index < 13; index += 1) {
      storeAcpBranchViewState(`lru-branch-${index}`, state(index));
    }
    expect(restoreAcpBranchViewState('lru-branch-0')).toBeNull();
    expect(restoreAcpBranchViewState('lru-branch-12')).toEqual(state(12));
  });
});
