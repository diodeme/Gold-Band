/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { Markdown } from '@/components/prompt-kit/markdown';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const writeText = vi.fn<(value: string) => Promise<void>>();

beforeEach(() => {
  writeText.mockResolvedValue();
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  });
});

afterEach(() => {
  writeText.mockReset();
  document.body.replaceChildren();
});

describe('prompt-kit fenced code blocks', () => {
  it('copies each block body without Markdown fences or the language marker', async () => {
    const markdown = [
      '```json',
      '{',
      '  "status": "active"',
      '}',
      '```',
      '',
      '```java',
      'class AcpModule {}',
      '```',
    ].join('\n');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => root.render(<TooltipProvider><Markdown>{markdown}</Markdown></TooltipProvider>));
      const buttons = container.querySelectorAll<HTMLButtonElement>('[data-streamdown="code-block-copy-button"]');
      expect(buttons).toHaveLength(2);

      await act(async () => new Promise((resolve) => window.setTimeout(resolve, 0)));
      const jsonBody = container.querySelector<HTMLElement>('[data-streamdown="code-block-body"][data-language="json"]');
      expect(jsonBody?.querySelectorAll('[style*="--sdm-c"]').length).toBeGreaterThan(1);
      expect(jsonBody?.querySelectorAll('code > span')).toHaveLength(3);

      const settledButtons = container.querySelectorAll<HTMLButtonElement>('[data-streamdown="code-block-copy-button"]');
      expect(settledButtons[1]?.disabled).toBe(false);
      expect(navigator.clipboard.writeText).toBe(writeText);
      await act(async () => {
        settledButtons[1]?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await Promise.resolve();
      });

      expect(writeText).toHaveBeenCalledOnce();
      expect(writeText).toHaveBeenCalledWith('class AcpModule {}\n');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
