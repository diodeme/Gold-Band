/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import '@/i18n';
import { EditableConversationTitle } from '@/components/conversation/EditableConversationTitle';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
});

describe('EditableConversationTitle', () => {
  it('keeps the header layout slot separate from the title trigger and edit field', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <EditableConversationTitle title="主题引擎-v2" className="flex-1" />
          </TooltipProvider>,
        );
      });

      const trigger = container.querySelector('button');
      expect(trigger).not.toBeNull();
      expect(trigger?.parentElement?.classList.contains('flex-1')).toBe(true);
      expect(trigger?.classList.contains('flex-1')).toBe(false);
      expect(trigger?.classList.contains('inline-flex')).toBe(true);
      expect(trigger?.classList.contains('max-w-full')).toBe(true);
      expect(trigger?.classList.contains('-ml-1')).toBe(false);

      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });

      const input = container.querySelector('input');
      expect(input).not.toBeNull();
      expect(input?.parentElement?.classList.contains('flex-1')).toBe(true);
      expect(input?.classList.contains('flex-1')).toBe(false);
      expect(input?.className).toContain('[field-sizing:content]');
      expect(input?.classList.contains('max-w-full')).toBe(true);
      expect(input?.getAttribute('aria-label')).toBe('修改标题');
    } finally {
      await act(async () => {
        root.unmount();
      });
    }
  });
});
