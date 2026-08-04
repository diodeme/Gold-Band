/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import '@/i18n';
import { EditableConversationTitle } from '@/components/conversation/EditableConversationTitle';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

describe('Tooltip streaming rerenders', () => {
  it('keeps the trigger mounted across repeated parent renders without a ref update loop', async () => {
    const consoleErrors: unknown[][] = [];
    vi.spyOn(console, 'error').mockImplementation((...args) => {
      consoleErrors.push(args);
    });

    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          React.createElement(
            TooltipProvider,
            null,
            React.createElement(EditableConversationTitle, {
              title: 'Streaming conversation',
              metadata: '0',
              showEditIcon: false,
            }),
          ),
        );
      });

      const trigger = container.querySelector('[data-slot="tooltip-trigger"]');
      expect(trigger).not.toBeNull();
      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('pointermove', { bubbles: true }));
      });

      for (let frame = 1; frame < 80; frame += 1) {
        await act(async () => {
          root.render(
            React.createElement(
              TooltipProvider,
              null,
              React.createElement(EditableConversationTitle, {
                title: 'Streaming conversation',
                metadata: `${frame}`,
                showEditIcon: false,
              }),
            ),
          );
        });
      }

      expect(container.querySelector('[data-slot="tooltip-trigger"]')?.textContent)
        .toContain('Streaming conversation');
      expect(consoleErrors.flat().join('\n')).not.toContain('Maximum update depth exceeded');
    } finally {
      await act(async () => {
        root.unmount();
      });
    }
  });
});
