import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { PermissionRequestCard } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';

function renderPermissionCard() {
  return renderToStaticMarkup(
    React.createElement(
      TooltipProvider,
      null,
      React.createElement(PermissionRequestCard, {
        request: {
          requestId: 'permission-1',
          title: 'Permission required',
          raw: {},
          options: [
            {
              optionId: 'allow-command-prefix',
              kind: 'allow_always',
              name: 'Allow Commands Starting With `rg --files -g !*node_modules*`',
            },
            {
              optionId: 'reject-once',
              kind: 'reject_once',
              name: 'Reject',
            },
          ],
        },
        onSelect: () => undefined,
      }),
    ),
  );
}

describe('PermissionRequestCard', () => {
  it('uses a compact low-emphasis approval surface', () => {
    const html = renderPermissionCard();

    expect(html).toContain('max-w-2xl');
    expect(html).toContain('bg-card/65');
    expect(html).toContain('bg-accent/65');
    expect(html).toContain('bg-background/45');
    expect(html).not.toContain('bg-primary text-primary-foreground');
  });

  it('exposes the full option label from the truncated button trigger', () => {
    const html = renderPermissionCard();
    const longLabel = 'Allow Commands Starting With `rg --files -g !*node_modules*`';

    expect(html).toContain('data-slot="tooltip-trigger"');
    expect(html).toContain(`aria-label="${longLabel}"`);
    expect(html).toContain('min-w-0 truncate');
  });
});
