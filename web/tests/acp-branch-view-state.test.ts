/** @vitest-environment jsdom */

import { describe, expect, it } from 'vitest';

import {
  applyAcpScrollAnchorCompensation,
  captureAcpBranchViewState,
  restoreAcpBranchViewState,
  storeAcpBranchViewState,
} from '@/components/acp/ACPChatDialog';

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

  it('captures and compensates a real DOM item anchor for either root or Agent branches', () => {
    const scroller = document.createElement('div');
    const item = document.createElement('div');
    item.dataset.acpItemKey = 'message-anchor';
    scroller.append(item);
    scroller.scrollTop = 100;
    Object.defineProperty(scroller, 'getBoundingClientRect', {
      configurable: true,
      value: () => ({ top: 20, bottom: 420, left: 0, right: 400, width: 400, height: 400, x: 0, y: 20, toJSON() {} }),
    });
    Object.defineProperty(item, 'getBoundingClientRect', {
      configurable: true,
      value: () => ({ top: 80, bottom: 120, left: 0, right: 400, width: 400, height: 40, x: 0, y: 80, toJSON() {} }),
    });

    expect(captureAcpBranchViewState(scroller, false, true, false)).toMatchObject({
      anchorKey: 'message-anchor',
      anchorOffset: 60,
      scrollTop: 100,
    });
    expect(applyAcpScrollAnchorCompensation(scroller, 'message-anchor', 60)).toBe(true);
    expect(scroller.scrollTop).toBe(120);
    expect(applyAcpScrollAnchorCompensation(scroller, 'missing', 60)).toBe(false);
  });
});
