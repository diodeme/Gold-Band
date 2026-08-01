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
});
