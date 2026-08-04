import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import {
  ACPSessionHeader,
  formatAcpSessionIdForDisplay,
  resolveRawFramesActionActive,
  reduceAcpSessionIdTooltipState,
} from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpSessionVm } from '@/types';

function session(): AcpSessionVm {
  return {
    provider: 'claude-acp',
    adapterDisplayName: 'Claude',
    adapterIconKey: 'claude',
    sessionId: 'session-1',
    status: 'completed',
    restored: false,
    systemPromptAppend: null,
    events: [],
    eventPage: {
      loadedCount: 0,
      total: 0,
      oldestSeq: null,
      newestSeq: null,
      hasOlder: false,
      hasNewer: false,
    },
    pendingPermissions: [],
    pendingElicitations: [],
  } as AcpSessionVm;
}

function renderHeader(props: React.ComponentProps<typeof ACPSessionHeader>) {
  return renderToStaticMarkup(
    React.createElement(
      TooltipProvider,
      null,
      React.createElement(ACPSessionHeader, props),
    ),
  );
}

describe('ACPSessionHeader', () => {
  it('uses an active raw action only when the button actually switches the current canvas', () => {
    expect(resolveRawFramesActionActive(false, true)).toBe(true);
    expect(resolveRawFramesActionActive(true, true)).toBe(false);
    expect(resolveRawFramesActionActive(true, false)).toBe(false);
  });

  it('shortens long session ids while preserving compact ids', () => {
    expect(formatAcpSessionIdForDisplay('019f9417-0b0f-75c2-a79a-739cd4c94238'))
      .toBe('019f9417…4238');
    expect(formatAcpSessionIdForDisplay('session-1')).toBe('session-1');
  });

  it('keeps copied feedback during tooltip close before returning to the full id', () => {
    const copied = reduceAcpSessionIdTooltipState(
      { open: false, phase: 'idle', reopenBlocked: false },
      { type: 'copy-succeeded' },
    );
    const closing = reduceAcpSessionIdTooltipState(copied, { type: 'feedback-elapsed' });

    expect(copied).toEqual({ open: true, phase: 'copied', reopenBlocked: true });
    expect(closing).toEqual({ open: false, phase: 'closing', reopenBlocked: true });
    expect(reduceAcpSessionIdTooltipState(closing, { type: 'open-changed', open: true }))
      .toBe(closing);
    expect(reduceAcpSessionIdTooltipState(closing, { type: 'close-settled' }))
      .toEqual({ open: false, phase: 'idle', reopenBlocked: true });
  });

  it('does not reopen the session id tooltip when the app regains focus', () => {
    const deactivated = reduceAcpSessionIdTooltipState(
      { open: true, phase: 'idle', reopenBlocked: false },
      { type: 'app-deactivated' },
    );

    expect(deactivated).toEqual({ open: false, phase: 'idle', reopenBlocked: true });
    expect(reduceAcpSessionIdTooltipState(deactivated, { type: 'open-changed', open: true }))
      .toBe(deactivated);

    const disengaged = reduceAcpSessionIdTooltipState(deactivated, { type: 'trigger-disengaged' });
    expect(disengaged).toEqual({ open: false, phase: 'idle', reopenBlocked: false });
    expect(reduceAcpSessionIdTooltipState(disengaged, { type: 'open-changed', open: true }))
      .toEqual({ open: true, phase: 'idle', reopenBlocked: false });
  });

  it('hides the system prompt action for Direct sessions', () => {
    const html = renderHeader({
      session: session(),
      rawActive: false,
      rawLoading: false,
      showSystemPromptAction: false,
      onToggleRaw: () => undefined,
      onOpenSystemPrompt: () => undefined,
    });

    expect(html).not.toContain('系统提示');
    expect(html).toContain('原始帧');
  });

  it('shows agent identity and a copyable session id without stale permission metadata', () => {
    const value = session();
    value.config = {
      currentModeId: 'bypassPermissions',
      currentModeName: 'Bypass Permissions',
    } as AcpSessionVm['config'];

    const html = renderHeader({
      session: value,
      rawActive: false,
      rawLoading: false,
      onToggleRaw: () => undefined,
      onOpenSystemPrompt: () => undefined,
    });

    expect(html).toContain('/agent-icons/claude.svg');
    expect(html).toContain('Claude');
    expect(html).toContain('session-1');
    expect(html).toContain('aria-label="复制 session ID"');
    expect(html).toContain('items-baseline');
    expect(html).toContain('gap-1.5');
    expect(html).toContain('text-[10px] leading-5');
    expect(html).not.toContain('px-1 py-0.5 text-[10px]');
    expect(html).not.toContain('Bypass Permissions');
    expect(html).not.toContain('权限');
  });

  it('combines the Direct title, session identity, diagnostics and folder action in one header row', () => {
    const html = renderHeader({
      session: session(),
      rawActive: false,
      rawLoading: false,
      showSystemPromptAction: false,
      directSessionHeader: {
        title: 'Direct title',
        onOpenInFileManager: () => undefined,
      },
      onToggleRaw: () => undefined,
      onOpenSystemPrompt: () => undefined,
    });

    expect(html).toContain('Direct title');
    expect(html).toContain('Claude');
    expect(html).toContain('session-1');
    expect(html).toContain('原始帧');
    expect(html).toContain('aria-label="打开目录"');
    expect(html).toContain('py-0.5');
    expect(html).toContain('gap-1');
    expect(html).toContain('mr-2 min-w-0 max-w-[40%] shrink');
    expect(html).not.toContain('lucide-pencil');
    expect(html).toContain('data-slot="tooltip-trigger"');
    expect(html).not.toContain('title="修改标题"');
  });

  it('shows a compact session id in the header', () => {
    const value = session();
    value.sessionId = '019f9417-0b0f-75c2-a79a-739cd4c94238';

    const html = renderHeader({
      session: value,
      rawActive: false,
      rawLoading: false,
      onToggleRaw: () => undefined,
      onOpenSystemPrompt: () => undefined,
    });

    expect(html).toContain('019f9417…4238');
    expect(html).not.toContain('>019f9417-0b0f-75c2-a79a-739cd4c94238</button>');
  });
});
