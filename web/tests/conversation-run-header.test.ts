import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import '@/i18n';
import { ConversationRunHeader } from '@/components/conversation/ConversationRunHeader';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { ConversationRunVm } from '@/types';

function directRun(): ConversationRunVm {
  return {
    projectId: 'default',
    taskId: 'task-001',
    runId: 'run-001',
    title: 'Direct conversation',
    runStatus: 'running',
    runMode: 'direct',
    directConfig: {
      agentType: 'claude-acp',
      modelId: 'sonnet-hidden',
      permissionMode: 'bypass-hidden',
    },
    agentIdentity: {
      agentType: 'claude-acp',
      displayName: 'Claude hidden',
      iconKey: 'claude',
    },
    sessionTree: { rounds: [], selectedSessionKey: null },
    selectedSession: null,
    activeSessions: [],
    inputAttachments: [],
    workflowValid: true,
    workflowStatus: 'valid',
    workflowGraph: null,
  } as ConversationRunVm;
}

describe('ConversationRunHeader', () => {
  it('keeps only the folder action on the right for Direct sessions', () => {
    const html = renderToStaticMarkup(
      React.createElement(
        TooltipProvider,
        null,
        React.createElement(ConversationRunHeader, {
          run: directRun(),
          onRerun: () => undefined,
          onEditWorkflow: () => undefined,
          onViewWorkflow: () => undefined,
          onOpenInFileManager: () => undefined,
          onToggleSessionSwitcher: () => undefined,
          sessionSwitcherOpen: false,
          canViewWorkflow: false,
          canEditWorkflow: false,
        }),
      ),
    );

    expect(html).toContain('aria-label="打开目录"');
    expect(html).toContain('lucide-folder-open');
    expect(html).not.toContain('Claude hidden');
    expect(html).not.toContain('sonnet-hidden');
    expect(html).not.toContain('bypass-hidden');
    expect(html).not.toContain('title="修改标题"');
  });
});
