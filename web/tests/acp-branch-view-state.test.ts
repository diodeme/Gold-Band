/** @vitest-environment jsdom */

import { beforeEach, describe, expect, it } from 'vitest';

import {
  applyAcpScrollAnchorCompensation,
  captureAcpBranchScrollState,
  captureAcpBranchViewState,
  hasHydratedAcpSessionContent,
  markAcpSessionContentHydrated,
  resetAcpResourceCache,
  restoreAcpBranchViewState,
  restoreAcpLoadedEventWindow,
  restoreAcpSession,
  shouldInitiallyFollowAcpBranch,
  storeAcpBranchViewState,
  storeAcpLoadedEventWindow,
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
    generation: 1,
    loadedCount: 0,
    total: 0,
    hasOlder: false,
    hasNewer: false,
  },
  timelineProjection: { agents: [], todoEntries: [] },
  pendingInteractions: [],
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
  it('starts a remounted viewport from the cached follow intent', () => {
    expect(shouldInitiallyFollowAcpBranch(state(100))).toBe(false);
    expect(shouldInitiallyFollowAcpBranch({ ...state(100), atBottom: true })).toBe(true);
    expect(shouldInitiallyFollowAcpBranch(null)).toBe(true);
  });

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

  it('distinguishes a hydrated content response from a cached session projection', () => {
    storeAcpSession('hydration-session', session('agent-summary'));
    expect(hasHydratedAcpSessionContent('hydration-session')).toBe(false);

    markAcpSessionContentHydrated('hydration-session');
    storeAcpSession('hydration-session', session('agent-refreshed-summary'));

    expect(hasHydratedAcpSessionContent('hydration-session')).toBe(true);
    expect(restoreAcpSession('hydration-session')?.branchId).toBe('agent-refreshed-summary');
  });

  it('evicts session, events, and view state atomically by resource key', () => {
    storeAcpSession('combined-oldest', session('agent-oldest'));
    storeAcpLoadedEventWindow('combined-oldest', {
      sessionId: 'session-1',
      timelineGeneration: 1,
      events: [event('event-oldest')],
    }, 100);
    storeAcpBranchViewState('combined-oldest', state(100));
    for (let index = 0; index < 12; index += 1) {
      storeAcpSession(`combined-${index}`, session(`agent-${index}`));
    }

    expect(restoreAcpSession('combined-oldest')).toBeNull();
    expect(restoreAcpBranchViewState('combined-oldest')).toBeNull();
    expect(restoreAcpLoadedEventWindow('combined-oldest', null, 100).events).toEqual([]);
  });

  it('restores a historical visible window without merging a cached live-head session', () => {
    const key = 'historical-window-with-live-head';
    const historicalEvent = event('historical-visible-event');
    const liveHeadEvent = { ...event('live-head-event'), seq: 20 };
    storeAcpLoadedEventWindow(key, {
      sessionId: 'session-1',
      timelineGeneration: 1,
      events: [historicalEvent],
    }, 100);
    const liveHeadSession = {
      ...session('root'),
      events: [liveHeadEvent],
      eventPage: {
        generation: 2,
        loadedCount: 1,
        total: 20,
        hasOlder: true,
        hasNewer: false,
      },
    };
    storeAcpSession(key, liveHeadSession);
    storeAcpBranchViewState(key, {
      ...state(500),
      hasNewer: true,
    });

    expect(restoreAcpLoadedEventWindow(key, liveHeadSession, 100, true)).toEqual({
      sessionId: 'session-1',
      timelineGeneration: 1,
      events: [historicalEvent],
    });
    expect(restoreAcpBranchViewState(key)?.hasNewer).toBe(true);
  });

  it('does not preserve a historical window owned by a different ACP session', () => {
    const key = 'historical-window-replaced-session';
    const historicalEvent = event('historical-session-one');
    storeAcpLoadedEventWindow(key, {
      sessionId: 'session-1',
      timelineGeneration: 1,
      events: [historicalEvent],
    }, 100);
    const replacementEvent = {
      ...event('replacement-session-two'),
      sessionId: 'session-2',
    };
    const replacementSession = {
      ...session('root'),
      sessionId: 'session-2',
      events: [replacementEvent],
    };

    expect(restoreAcpLoadedEventWindow(key, replacementSession, 100, true)).toEqual({
      sessionId: 'session-2',
      timelineGeneration: 1,
      events: [replacementEvent],
    });
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
