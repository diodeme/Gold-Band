/** @vitest-environment jsdom */

import { beforeEach, describe, expect, it } from 'vitest';

import {
  applyAcpScrollAnchorCompensation,
  captureAcpBranchScrollState,
  captureAcpBranchViewState,
  resetAcpResourceCache,
  restoreAcpBranchViewState,
  restoreAcpLoadedEvents,
  restoreAcpSession,
  storeAcpBranchViewState,
  storeAcpLoadedEvents,
  storeAcpSession,
} from '@/components/acp/ACPChatDialog';
import type { AcpSessionVm, AcpUiEventVm } from '@/types';

const state = (scrollTop: number) => ({
  anchorKey: `message-${scrollTop}`,
  anchorOffset: 12,
  scrollTop,
  atBottom: false,
  hasOlder: true,
  hasNewer: false,
});

const session = (branchId: string): AcpSessionVm => ({
  branchId,
  parentBranchId: 'root',
  readOnly: true,
  branchExecution: {
    agentExecutionId: branchId,
    parentAgentExecutionId: null,
    executionStatus: 'completed',
    eventCount: 1,
    toolCallCount: 0,
    readFileCount: 0,
    writtenFileCount: 0,
    hasAttention: false,
    todoEntries: [],
  },
  sessionId: 'session-1',
  title: branchId,
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
  provider: 'test',
  status: 'completed',
  restored: true,
  events: [],
  eventPage: {
    loadedCount: 0,
    total: 0,
    hasOlder: false,
    hasNewer: false,
  },
  timelineProjection: { agents: [], todoEntries: [] },
  pendingPermissions: [],
  pendingElicitations: [],
  diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
});

const event = (id: string): AcpUiEventVm => ({
  id,
  seq: 1,
  timestamp: '2026-01-01T00:00:00Z',
  kind: 'assistantTextDelta',
  sessionId: 'session-1',
  content: id,
});

beforeEach(() => resetAcpResourceCache());

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

  it('restores a finite Agent session VM cache independently from mounted Tab DOM', () => {
    for (let index = 0; index < 13; index += 1) {
      storeAcpSession(`session-lru-${index}`, session(`agent-${index}`));
    }
    expect(restoreAcpSession('session-lru-0')).toBeNull();
    expect(restoreAcpSession('session-lru-12')?.branchId).toBe('agent-12');
  });

  it('evicts session, events, and view state atomically by resource key', () => {
    storeAcpSession('combined-oldest', session('agent-oldest'));
    storeAcpLoadedEvents('combined-oldest', [event('event-oldest')], 100);
    storeAcpBranchViewState('combined-oldest', state(100));
    for (let index = 0; index < 12; index += 1) {
      storeAcpSession(`combined-${index}`, session(`agent-${index}`));
    }

    expect(restoreAcpSession('combined-oldest')).toBeNull();
    expect(restoreAcpBranchViewState('combined-oldest')).toBeNull();
    expect(restoreAcpLoadedEvents('combined-oldest', [], 100)).toEqual([]);
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

  it('updates scroll state without reading or scanning message DOM in the scroll hot path', () => {
    const querySelectorAll = () => {
      throw new Error('scroll hot path must not scan message DOM');
    };
    const scroller = { scrollTop: 240, querySelectorAll };

    expect(captureAcpBranchScrollState(scroller, true, false, true)).toEqual({
      anchorKey: null,
      anchorOffset: 0,
      scrollTop: 240,
      atBottom: true,
      hasOlder: false,
      hasNewer: true,
    });
  });
});
