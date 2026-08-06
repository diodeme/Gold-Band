/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const { streamdownRender } = vi.hoisted(() => ({ streamdownRender: vi.fn() }));

vi.mock('streamdown', () => ({
  defaultUrlTransform: (url: string) => url,
  parseMarkdownIntoBlocks: (markdown: string) => [markdown],
  Streamdown: React.memo(({ children, isAnimating }: {
    children: React.ReactNode;
    isAnimating?: boolean;
  }) => {
    streamdownRender({ children, isAnimating });
    return <div>{children}</div>;
  }),
}));

vi.mock('@/api', () => ({
  openExternalUrl: vi.fn(),
}));

import { Markdown } from '@/components/prompt-kit/markdown';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
  streamdownRender.mockClear();
  vi.restoreAllMocks();
});

describe('streaming Markdown render budget', () => {
  it('keeps exactly one replaceable typewriter frame pending', async () => {
    let nextFrameId = 1;
    const pendingFrames = new Map<number, FrameRequestCallback>();
    vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      const frameId = nextFrameId;
      nextFrameId += 1;
      pendingFrames.set(frameId, callback);
      return frameId;
    });
    vi.spyOn(window, 'cancelAnimationFrame').mockImplementation((frameId) => {
      pendingFrames.delete(frameId);
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(
        <Markdown streaming>第一段</Markdown>,
      ));
      expect(streamdownRender).toHaveBeenCalledTimes(1);
      expect(streamdownRender).toHaveBeenLastCalledWith({
        children: '第',
        isAnimating: true,
      });
      expect(pendingFrames.size).toBe(1);

      await act(async () => root.render(
        <Markdown streaming>{'第一段\n\n第二段'}</Markdown>,
      ));
      expect(pendingFrames.size).toBe(1);

      const [frameId, frame] = pendingFrames.entries().next().value!;
      pendingFrames.delete(frameId);
      await act(async () => {
        frame(32);
      });
      expect(pendingFrames.size).toBe(1);
      expect(streamdownRender.mock.calls.at(-1)?.[0].children.length).toBeGreaterThan(1);
    } finally {
      await act(async () => root.unmount());
      expect(pendingFrames.size).toBe(0);
    }
  });
});
