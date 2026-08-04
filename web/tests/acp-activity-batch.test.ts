/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ACPMessageList, buildAcpTimelineProjection } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function event(partial: Partial<AcpUiEventVm>): AcpUiEventVm {
  return {
    id: 'event',
    seq: 1,
    timestamp: '1Z',
    kind: 'toolCall',
    sessionId: 'session',
    content: null,
    title: null,
    toolCallId: null,
    status: null,
    raw: null,
    ...partial,
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe('ACP activity batch disclosure', () => {
  it('keeps a finalized file change set immediately after the turn activity batch', () => {
    const projection = buildAcpTimelineProjection([
      event({
        id: 'write-tool',
        seq: 1,
        kind: 'toolCall',
        toolCallId: 'write-tool',
        title: 'Write src/app.ts',
        status: 'completed',
      }),
      event({
        id: 'file-change-set',
        seq: 2,
        kind: 'fileChangeSet',
        status: 'finalized',
        raw: {
          changeSetId: 'change-set-1',
          summary: { fileCount: 1, addedFiles: 1, modifiedFiles: 0, deletedFiles: 0, addedLines: 2, deletedLines: 0 },
        },
      }),
    ], 'completed');

    expect(projection.timeline.map((item) => item.kind)).toEqual([
      'activityBatch',
      'fileChangeSet',
    ]);
  });

  it('does not touch a large tool output until the individual tool is expanded', async () => {
    let outputReads = 0;
    const raw: Record<string, unknown> = {
      title: 'Read',
      rawInput: { path: 'large.log' },
    };
    Object.defineProperty(raw, 'output', {
      enumerable: true,
      configurable: true,
      get() {
        outputReads += 1;
        return 'x'.repeat(256_000);
      },
    });
    const projection = buildAcpTimelineProjection([
      event({
        id: 'large-tool',
        kind: 'toolCall',
        toolCallId: 'large-tool',
        title: 'Read large.log',
        status: 'completed',
        raw,
      }),
    ], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(React.createElement(ACPMessageList, {
          timeline: projection.timeline,
          sessionStatus: 'completed',
          sending: false,
        }));
      });
      expect(outputReads).toBe(0);

      const activityTrigger = container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      await act(async () => {
        activityTrigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(outputReads).toBe(0);

      const triggers = container.querySelectorAll<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      expect(triggers.length).toBeGreaterThanOrEqual(2);
      await act(async () => {
        triggers[1]?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(outputReads).toBeGreaterThan(0);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('hides terminal permission records and offers collapse at the detail footer', async () => {
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: scrollIntoView,
    });

    const command = 'Get-Content -Path "docs/gold-band/开发计划/acp接入/acp-first-refactor-plan.md"';
    const toolCallId = 'exec-status';
    const projection = buildAcpTimelineProjection([
      event({
        id: 'tool',
        kind: 'toolCall',
        toolCallId,
        title: command,
        status: 'completed',
        raw: { title: command, rawInput: { command } },
      }),
      event({
        id: 'permission',
        seq: 2,
        timestamp: '2Z',
        kind: 'permissionRequest',
        toolCallId,
        title: 'Permission required',
        status: 'selected',
        raw: {
          requestId: 'permission-status',
          optionId: 'allow_always',
          toolCall: { toolCallId, title: command, rawInput: { command } },
          options: [{ optionId: 'allow_always', kind: 'allow_always', name: 'Allow for Session' }],
        },
      }),
    ], 'completed');

    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            TooltipProvider,
            null,
            React.createElement(ACPMessageList, {
              timeline: projection.timeline,
              sessionStatus: 'completed',
              sending: false,
            }),
          ),
        );
      });

      const trigger = container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      expect(trigger?.getAttribute('aria-expanded')).toBe('false');

      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });

      const decision = container.querySelector<HTMLElement>('.acp-permission-decision-audit');
      const collapse = container.querySelector<HTMLButtonElement>('.acp-activity-collapse-button');
      expect(decision).toBeNull();
      expect(container.textContent).not.toContain('Allow for Session');
      expect(container.textContent).toContain(command);
      expect(collapse?.textContent).toContain('收起');

      await act(async () => {
        collapse?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await new Promise((resolve) => window.setTimeout(resolve, 1));
      });

      expect(trigger?.getAttribute('aria-expanded')).toBe('false');
      expect(container.querySelector('.acp-permission-decision-audit')).toBeNull();
      expect(scrollIntoView).toHaveBeenCalledWith({ block: 'nearest' });
    } finally {
      await act(async () => {
        root.unmount();
      });
    }
  });

  it('hands bottom-follow ownership to the activity disclosure lifecycle', async () => {
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
      window.setTimeout(() => callback(performance.now()), 0)
    ));
    vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: scrollIntoView,
    });
    const onActivityDisclosureOpen = vi.fn(() => 42);
    const onActivityDisclosureClose = vi.fn(() => true);
    const projection = buildAcpTimelineProjection([
      event({
        id: 'tool',
        kind: 'toolCall',
        toolCallId: 'tool',
        title: 'Read activity.log',
        status: 'completed',
        raw: { title: 'Read activity.log', rawInput: { path: 'activity.log' } },
      }),
    ], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(React.createElement(ACPMessageList, {
          timeline: projection.timeline,
          sessionStatus: 'completed',
          sending: false,
          onActivityDisclosureOpen,
          onActivityDisclosureClose,
        }));
      });

      const trigger = container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(onActivityDisclosureOpen).toHaveBeenCalledTimes(1);

      const collapse = container.querySelector<HTMLButtonElement>('.acp-activity-collapse-button');
      await act(async () => {
        collapse?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await new Promise((resolve) => window.setTimeout(resolve, 1));
      });
      expect(onActivityDisclosureClose).toHaveBeenCalledWith(42);
      expect(scrollIntoView).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
