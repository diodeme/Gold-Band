import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { DirectAgentEmptyState } from '@/components/conversation/ConversationComposer';
import { TooltipProvider } from '@/components/ui/tooltip';

describe('DirectAgentEmptyState', () => {
  it('keeps the empty hint and exposes an accessible Agent Management action', () => {
    const html = renderToStaticMarkup(React.createElement(
      TooltipProvider,
      null,
      React.createElement(DirectAgentEmptyState, {
        onOpenAgentManagement: () => undefined,
      }),
    ));

    expect(html).toContain('请先在 Agent 管理中添加 Agent');
    expect(html).toContain('aria-label="新增 Agent"');
    expect(html).toContain('<svg');
  });
});
