/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const { streamingDiagnostic } = vi.hoisted(() => ({
  streamingDiagnostic: vi.fn(),
}));

vi.mock('@/api', () => ({
  openExternalUrl: vi.fn(),
}));

vi.mock('@/lib/acp-streaming-diagnostics', () => ({
  recordAcpStreamingDiagnostic: streamingDiagnostic,
}));

import { Markdown } from '@/components/prompt-kit/markdown';
import {
  STREAMING_MARKDOWN_MAX_CHARS_PER_SECOND,
  STREAMING_MARKDOWN_MIN_CHARS_PER_SECOND,
  streamingMarkdownCharactersPerSecond,
} from '@/lib/streaming-markdown-playback';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

type FrameHarness = {
  elapse: (elapsed: number) => void;
  flush: (elapsed?: number) => Promise<void>;
  pending: () => number;
};

function installFrameHarness(): FrameHarness {
  let nextFrameId = 1;
  let now = 0;
  const frames = new Map<number, FrameRequestCallback>();
  vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
    const frameId = nextFrameId;
    nextFrameId += 1;
    frames.set(frameId, callback);
    return frameId;
  });
  vi.spyOn(window, 'cancelAnimationFrame').mockImplementation((frameId) => {
    frames.delete(frameId);
  });
  return {
    elapse(elapsed) {
      now += elapsed;
    },
    async flush(elapsed = 24) {
      const next = frames.entries().next().value as [number, FrameRequestCallback] | undefined;
      if (!next) return;
      frames.delete(next[0]);
      now += elapsed;
      await act(async () => next[1](now));
    },
    pending: () => frames.size,
  };
}

function tokenStates(container: HTMLElement) {
  return [...container.querySelectorAll<HTMLElement>('[data-sd-animate]')]
    .map((token) => token.dataset.gbStreamState ?? null);
}

afterEach(() => {
  document.body.replaceChildren();
  streamingDiagnostic.mockClear();
  vi.restoreAllMocks();
});

describe('streaming Markdown render budget', () => {
  it('uses one document playback frame and reveals renderer tokens as a strict prefix across blocks', async () => {
    const frames = installFrameHarness();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(
        <Markdown streaming>{'1. 甲\n2. 乙\n3. 丙\n\n## 最终结论\n\n继续。'}</Markdown>,
      ));

      expect(container.textContent).toContain('甲');
      expect(container.textContent).toContain('最终结论');
      expect(tokenStates(container).every((state) => state === 'pending')).toBe(true);
      expect(frames.pending()).toBe(1);
      expect(streamingDiagnostic).toHaveBeenCalledWith(
        'markdown-playback-init',
        expect.any(Function),
      );

      for (let step = 0; step < 12; step += 1) {
        await frames.flush();
        const states = tokenStates(container);
        const firstPending = states.indexOf('pending');
        if (firstPending >= 0) {
          expect(states.slice(0, firstPending).every((state) => state !== 'pending')).toBe(true);
          expect(states.slice(firstPending).every((state) => state === 'pending')).toBe(true);
        }
        expect(frames.pending()).toBeLessThanOrEqual(1);
      }

      const heading = [...container.querySelectorAll<HTMLElement>('[data-gb-stream-block]')]
        .find((block) => block.textContent?.includes('最终结论'));
      const list = container.querySelector('ol');
      if (heading?.dataset.gbStreamBlockState === 'visible') {
        expect(list?.querySelectorAll('li[data-gb-stream-item-visible="true"]')).toHaveLength(3);
      }
    } finally {
      await act(async () => root.unmount());
      expect(frames.pending()).toBe(0);
    }
  });

  it('preserves the document playback watermark when Streamdown replaces an append-only tail', async () => {
    const frames = installFrameHarness();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(<Markdown streaming>第一段</Markdown>));
      while (tokenStates(container).filter((state) => state !== 'pending').length < 2) {
        await frames.flush();
      }
      const revealedBeforeAppend = tokenStates(container).filter((state) => state !== 'pending').length;

      await act(async () => root.render(<Markdown streaming>第一段，继续追加</Markdown>));
      await act(async () => Promise.resolve());

      const states = tokenStates(container);
      expect(states.slice(0, revealedBeforeAppend).every((state) => state === 'settled')).toBe(true);
      expect(states.slice(revealedBeforeAppend).every((state) => state === 'pending')).toBe(true);
      expect(frames.pending()).toBe(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('settles a static baseline before animating only content appended after re-entry', async () => {
    const frames = installFrameHarness();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const baseline = '切换会话前已经完整展示';

    try {
      await act(async () => root.render(<Markdown>{baseline}</Markdown>));
      expect(container.textContent).toBe(baseline);
      expect(container.querySelector('[data-sd-animate]')).toBeNull();

      await act(async () => root.render(<Markdown streaming>{baseline}</Markdown>));
      await act(async () => Promise.resolve());
      const baselineTokenCount = tokenStates(container).length;
      expect(baselineTokenCount).toBeGreaterThan(0);
      expect(tokenStates(container).every((state) => state === 'settled')).toBe(true);
      expect(frames.pending()).toBe(0);

      await act(async () => root.render(<Markdown streaming>{`${baseline}，只播放新内容`}</Markdown>));
      await act(async () => Promise.resolve());
      const appendedStates = tokenStates(container);
      expect(appendedStates.slice(0, baselineTokenCount).every((state) => state === 'settled')).toBe(true);
      expect(appendedStates.slice(baselineTokenCount).every((state) => state === 'pending')).toBe(true);
      expect(frames.pending()).toBe(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('reuses stable Streamdown block indexes and scans only the replaced tail tokens', async () => {
    installFrameHarness();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(
        <Markdown streaming>{'第一段保持稳定。\n\n第二段仍是可变尾部。'}</Markdown>,
      ));
      await act(async () => Promise.resolve());
      streamingDiagnostic.mockClear();

      await act(async () => root.render(
        <Markdown streaming>{'第一段保持稳定。\n\n第二段仍是可变尾部，并继续增长。'}</Markdown>,
      ));
      await act(async () => Promise.resolve());

      const reconciles = streamingDiagnostic.mock.calls
        .filter(([stage]) => stage === 'markdown-playback-reconcile')
        .map(([, createDetails]) => createDetails());
      expect(reconciles.length).toBeGreaterThan(0);
      expect(reconciles.some((details) => (
        Number(details.reusedBlockCount) >= 1
        && Number(details.rebuiltBlockCount) >= 1
        && Number(details.scannedUnitCount) < Number(details.unitCount)
      ))).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('settles a non-append rewrite and the previous message without leaving a playback backlog', async () => {
    const frames = installFrameHarness();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const rewrittenCanonical = '完全改写后的消息必须立即成为稳定基线。';

    try {
      await act(async () => root.render(<Markdown streaming>原始流式内容仍未播完</Markdown>));
      await frames.flush();

      await act(async () => root.render(<Markdown streaming>{rewrittenCanonical}</Markdown>));
      await act(async () => Promise.resolve());
      expect(container.textContent).toBe(rewrittenCanonical);
      expect(tokenStates(container).every((state) => state === 'settled')).toBe(true);

      await act(async () => root.render(<Markdown>{rewrittenCanonical}</Markdown>));
      expect(container.textContent).toBe(rewrittenCanonical);
      expect(container.querySelector('[data-sd-animate]')).toBeNull();
      expect(frames.pending()).toBe(0);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps one Streamdown document context for cross-block Markdown semantics', async () => {
    installFrameHarness();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const markdown = '引用脚注[^1]\n\n第二段\n\n[^1]: 脚注内容';

    try {
      await act(async () => root.render(<Markdown streaming>{markdown}</Markdown>));

      expect(container.textContent).toContain('引用脚注');
      expect(container.textContent).toContain('脚注内容');
      expect(container.querySelectorAll(':scope > div > div')).toHaveLength(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('accelerates only within the configured backlog bounds', () => {
    expect(streamingMarkdownCharactersPerSecond(0)).toBe(STREAMING_MARKDOWN_MIN_CHARS_PER_SECOND);
    expect(streamingMarkdownCharactersPerSecond(10)).toBe(STREAMING_MARKDOWN_MIN_CHARS_PER_SECOND);
    expect(streamingMarkdownCharactersPerSecond(40)).toBe(125);
    expect(streamingMarkdownCharactersPerSecond(10_000)).toBe(STREAMING_MARKDOWN_MAX_CHARS_PER_SECOND);
  });

  it('samples playback and reports long frames without logging each revealed token', async () => {
    const frames = installFrameHarness();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(
        <Markdown streaming>{'足够长的正文用于验证播放采样，而不是为每个字符写一条诊断记录。'.repeat(8)}</Markdown>,
      ));
      streamingDiagnostic.mockClear();

      await frames.flush(16);
      streamingDiagnostic.mockClear();
      for (let frame = 0; frame < 7; frame += 1) await frames.flush(80);

      const stages = streamingDiagnostic.mock.calls.map(([stage]) => stage);
      expect(stages).toContain('markdown-playback-long-frame');
      expect(stages).toContain('markdown-playback-sample');
      expect(stages.length).toBeLessThan(tokenStates(container).filter((state) => state !== 'pending').length + 1);
      const longFrameDetails = streamingDiagnostic.mock.calls
        .find(([stage]) => stage === 'markdown-playback-long-frame')?.[1]();
      expect(longFrameDetails).toMatchObject({
        frameIntervalMs: 80,
        streaming: true,
        tickDurationMs: expect.any(Number),
      });
      expect(longFrameDetails).not.toHaveProperty('canonical');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not report an idle gap as a long playback frame when new tokens arrive', async () => {
    const frames = installFrameHarness();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(<Markdown streaming>甲</Markdown>));
      while (frames.pending() > 0) await frames.flush(24);
      frames.elapse(2_000);
      streamingDiagnostic.mockClear();

      await act(async () => root.render(<Markdown streaming>甲乙</Markdown>));
      await act(async () => Promise.resolve());
      await frames.flush(16);

      expect(streamingDiagnostic.mock.calls.some(
        ([stage]) => stage === 'markdown-playback-long-frame',
      )).toBe(false);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
