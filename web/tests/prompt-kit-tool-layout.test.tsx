/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';

import { Tool } from '@/components/prompt-kit/tool';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const labels = {
  input: 'Input',
  output: 'Output',
  error: 'Error',
  processing: 'Processing',
  pending: 'Pending',
  ready: 'Ready',
  completed: 'Completed',
};

afterEach(() => {
  document.body.replaceChildren();
});

describe('prompt-kit Tool layout', () => {
  it('keeps a long command title inside the shrinkable content column', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <Tool
            variant="audit"
            labels={labels}
            toolPart={{
              type: "$test='C:\\Users\\unlik\\.gold-band\\projects\\Gold-Band'; Get-Content -LiteralPath $test",
              state: 'output-available',
            }}
          />,
        );
      });

      const trigger = container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      const primary = container.querySelector<HTMLElement>('[data-tool-primary="true"]');
      const name = container.querySelector<HTMLElement>('[data-tool-name="true"]');
      const actions = container.querySelector<HTMLElement>('[data-tool-actions="true"]');

      expect(trigger?.className).toContain('grid-cols-[minmax(0,1fr)_auto]');
      expect(primary?.className).toContain('grid-cols-[auto_minmax(0,1fr)]');
      expect(primary?.className).toContain('overflow-hidden');
      expect(name?.className).toContain('min-w-0');
      expect(name?.className).toContain('truncate');
      expect(name?.className).not.toContain('shrink-0');
      expect(actions?.className).toContain('shrink-0');
      expect(actions?.textContent).toContain('Completed');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shares the remaining content width between the tool name and summary', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <Tool
            variant="audit"
            labels={labels}
            toolPart={{
              type: 'PowerShell',
              summary: 'Get-Content -LiteralPath C:\\Users\\unlik\\.gold-band\\projects\\Gold-Band\\very-long-file.txt',
              state: 'output-available',
            }}
          />,
        );
      });

      const name = container.querySelector<HTMLElement>('[data-tool-name="true"]');
      const summary = container.querySelector<HTMLElement>('[data-tool-summary="true"]');

      expect(name?.className).toContain('shrink');
      expect(summary?.className).toContain('min-w-0');
      expect(summary?.className).toContain('flex-1');
      expect(summary?.className).toContain('truncate');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
