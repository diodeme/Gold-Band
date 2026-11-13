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

describe('ConversationTurnRail rendering', () => {
  let host: HTMLDivElement;
  let root: Root;
  const jumps: number[] = [];

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
    jumps.length = 0;
    vi.mock('@/api/client', () => ({
      getRuntimeApi: () => ({
        getConversationTurnIndex: () => Promise.resolve(railEntries),
      }),
    }));
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.resetModules();
  });

  const mount = () => {
    act(() => {
      root.render(
        <ConversationTurnRail
          locator={{ projectId: null, taskId: 't', runId: 'r', roundId: 'rd', nodeId: 'n', attemptId: 'a' }}
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
    vi.doMock('@/api/client', () => ({
      getRuntimeApi: () => ({ getConversationTurnIndex: () => Promise.resolve([]) }),
    }));
    await act(async () => { mount(); });
    expect(host.querySelector('[data-acp-conversation-rail="turn-rail"]')).toBeNull();
  });
});
