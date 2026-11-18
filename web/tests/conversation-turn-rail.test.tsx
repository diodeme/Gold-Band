/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ConversationTurnRail } from '../src/components/acp/ConversationTurnRail';
import type { ConversationTurnIndexEntryVm } from '../src/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string, params?: Record<string, string>) =>
    params?.preview ? `${key}:${params.preview}` : key }),
}));

const railEntries: ConversationTurnIndexEntryVm[] = [
  { seq: 1, timestamp: '2026-11-12T01:00:00Z', kind: 'prompt', promptPreview: '第一问' },
  { seq: 4, timestamp: '2026-11-12T02:00:00Z', kind: 'permission', promptPreview: '授权后继续' },
];

const newerEntries: ConversationTurnIndexEntryVm[] = [
  { seq: 7, timestamp: '2026-11-12T03:00:00Z', kind: 'prompt', promptPreview: '追加了新 turn' },
];

interface FetchCall {
  afterSeq: number | null;
  beforeSeq?: number | null;
}

// Module-level mutable mock state: vi.mock factories are hoisted, so the
// implementation reads through these bindings instead of closing over
// per-test locals.
let fetchCalls: FetchCall[] = [];
let resolveWith: (afterSeq: number | null, beforeSeq?: number | null) => ConversationTurnIndexEntryVm[] | Promise<ConversationTurnIndexEntryVm[]> = () => railEntries;

vi.mock('@/api/client', () => ({
  getRuntimeApi: () => ({
    getConversationTurnIndex: (
      _projectId: string | null,
      _taskId: string,
      _runId: string,
      _roundId: string,
      _nodeId: string,
      _attemptId: string,
      _branchId: string | null,
      afterSeq: number | null,
      _outerNodeId: string | null,
      _outerAttemptId: string | null,
      beforeSeq: number | null,
    ) => Promise.resolve(resolveWith(afterSeq, beforeSeq)),
  }),
}));

describe('ConversationTurnRail rendering', () => {
  let host: HTMLDivElement;
  let root: Root;
  let refreshSignal: number;
  let runId: string;
  const jumps: number[] = [];

  beforeEach(() => {
    fetchCalls = [];
    resolveWith = (afterSeq) => {
      fetchCalls.push({ afterSeq });
      return afterSeq == null ? railEntries : newerEntries;
    };
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
    jumps.length = 0;
    refreshSignal = 0;
    runId = 'r';
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    resolveWith = () => railEntries;
  });

  const mount = () => {
    act(() => {
      root.render(
        <ConversationTurnRail
          locator={{ projectId: null, taskId: 't', runId, roundId: 'rd', nodeId: 'n', attemptId: 'a' }}
          refreshSignal={refreshSignal}
          onJumpToSeq={(seq) => jumps.push(seq)}
        />,
      );
    });
  };

  it('renders one tick per turn with no chapter lines or expand controls', async () => {
    await act(async () => { mount(); });
    const nav = host.querySelector('[data-acp-conversation-rail="turn-rail"]');
    expect(nav).not.toBeNull();
    const ticks = [...host.querySelectorAll<HTMLButtonElement>('[data-turn-seq]')];
    expect(ticks.map((tick) => tick.dataset.turnSeq)).toEqual(['1', '4']);
    expect(host.querySelector('[data-turn-rail-chapter]')).toBeNull();
    expect(host.querySelector('[data-turn-rail-expand]')).toBeNull();
    expect(host.querySelector('[data-turn-rail-collapse]')).toBeNull();
  });

  it('reports jump seqs on tick clicks and opens pinned to the newest turn', async () => {
    await act(async () => { mount(); });
    const scroller = host.querySelector<HTMLElement>('[data-acp-conversation-rail="turn-rail"] > div');
    expect(scroller?.scrollTop).toBe(scroller?.scrollHeight);
    const ticks = [...host.querySelectorAll<HTMLButtonElement>('[data-turn-seq]')];
    await act(async () => {
      ticks[1].click();
    });
    expect(jumps).toEqual([4]);
  });

  it('renders nothing while the index is empty', async () => {
    resolveWith = () => [];
    await act(async () => { mount(); });
    expect(host.querySelector('[data-acp-conversation-rail="turn-rail"]')).toBeNull();
  });

  it('merges incremental turns when the canonical watermark advances', async () => {
    await act(async () => { mount(); });
    // Initial load: full projection, no cursor.
    expect(fetchCalls.map((call) => call.afterSeq)).toEqual([null]);
    // Same-session follow-up prompt: watermark bumps, the rail asks only for
    // turns after its newest known seq and merges them by seq.
    refreshSignal = 7;
    await act(async () => { mount(); });
    expect(fetchCalls.map((call) => call.afterSeq)).toEqual([null, 3]);
    const ticks = [...host.querySelectorAll<HTMLButtonElement>('[data-turn-seq]')];
    expect(ticks.map((tick) => tick.dataset.turnSeq)).toEqual(['1', '4', '7']);
  });

  it('does not fetch incrementally before the initial load resolves', async () => {
    refreshSignal = 4;
    await act(async () => { mount(); });
    // The watermark effect is skipped while newestSeq is still unknown.
    expect(fetchCalls.map((call) => call.afterSeq)).toEqual([null]);
    const ticks = [...host.querySelectorAll<HTMLButtonElement>('[data-turn-seq]')];
    expect(ticks.map((tick) => tick.dataset.turnSeq)).toEqual(['1', '4']);
  });
  it('refreshes the first committed turn after an empty initial result', async () => {
    resolveWith = () => [];
    await act(async () => { mount(); });
    resolveWith = () => railEntries;
    refreshSignal = 4;
    await act(async () => { mount(); });
    expect(host.querySelectorAll('[data-turn-seq]').length).toBe(2);
  });

  it('coalesces watermark updates received during the initial request', async () => {
    let resolve!: (entries: ConversationTurnIndexEntryVm[]) => void;
    let calls = 0;
    resolveWith = () => { calls++; return calls === 1 ? new Promise(done => { resolve = done; }) : newerEntries; };
    await act(async () => { mount(); });
    refreshSignal = 7;
    await act(async () => { mount(); });
    expect(calls).toBe(1);
    await act(async () => { resolve(railEntries); });
    expect(calls).toBe(2);
    expect([...host.querySelectorAll<HTMLElement>('[data-turn-seq]')].map(n => n.dataset.turnSeq)).toEqual(['1', '4', '7']);
  });

  it('pages backward and bounds the resident DOM to 256 turns', async () => {
    resolveWith = (_after, before) => {
      const end = before ?? 10001;
      return Array.from({ length: 64 }, (_, n) => ({ ...railEntries[0], seq: end - 64 + n }));
    };
    await act(async () => { mount(); });
    for (let page = 0; page < 5; page++) {
      await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="acp.turnRail.older"]')!.click());
      expect(host.querySelectorAll('[data-turn-seq]').length).toBeLessThanOrEqual(256);
    }
    expect(host.querySelector('[data-turn-seq="9617"]')).not.toBeNull();
    expect(host.querySelector('[aria-label="acp.turnRail.newer"]')).not.toBeNull();
  });

  it('keeps refreshing after an older page when the latest turn is still resident', async () => {
    resolveWith = (after, before) => before != null
      ? [{ ...railEntries[0], seq: 1 }]
      : after != null
        ? [{ ...railEntries[0], seq: 66 }]
        : Array.from({ length: 64 }, (_, n) => ({ ...railEntries[0], seq: n + 2 }));
    await act(async () => { mount(); });
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="acp.turnRail.older"]')!.click());
    refreshSignal = 66;
    await act(async () => { mount(); });
    expect(host.querySelector('[data-turn-seq="66"]')).not.toBeNull();
  });

  it('rejects a late page from the previous Run', async () => {
    let resolve!: (entries: ConversationTurnIndexEntryVm[]) => void;
    resolveWith = () => new Promise(done => { resolve = done; });
    await act(async () => { mount(); });
    runId = 'other-run';
    resolveWith = () => newerEntries;
    await act(async () => { mount(); });
    await act(async () => { resolve(railEntries); });
    expect([...host.querySelectorAll<HTMLElement>('[data-turn-seq]')].map(n => n.dataset.turnSeq)).toEqual(['7']);
  });

});
