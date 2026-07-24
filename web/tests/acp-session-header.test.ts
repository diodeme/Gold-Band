import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ACPSessionHeader } from '@/components/acp/ACPChatDialog';
import type { AcpSessionVm } from '@/types';

function session(): AcpSessionVm {
  return {
    provider: 'claude-acp',
    adapterDisplayName: 'Claude',
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

describe('ACPSessionHeader', () => {
  it('hides the system prompt action for Direct sessions', () => {
    const html = renderToStaticMarkup(React.createElement(ACPSessionHeader, {
      session: session(),
      rawActive: false,
      rawLoading: false,
      showSystemPromptAction: false,
      onToggleRaw: () => undefined,
      onOpenSystemPrompt: () => undefined,
    }));

    expect(html).not.toContain('系统提示');
    expect(html).toContain('原始帧');
  });
});
