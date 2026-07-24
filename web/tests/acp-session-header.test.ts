import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ACPSessionHeader } from '@/components/acp/ACPChatDialog';
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
    expect(html).toContain('min-w-0 max-w-[40%] shrink');
    expect(html).not.toContain('lucide-pencil');
    expect(html).not.toContain('mr-2');
    expect(html).toContain('data-slot="tooltip-trigger"');
    expect(html).not.toContain('title="修改标题"');
  });
});
