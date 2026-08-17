/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/components/workspace/files/WorkspaceFileEditor', () => ({
  WorkspaceFileEditor: (props: {
    value: string;
    editable: boolean;
    markdownMode: string | null;
  }) => (
    <output
      data-testid="markdown-source-viewer"
      data-editable={String(props.editable)}
      data-markdown-mode={String(props.markdownMode)}
    >
      {props.value}
    </output>
  ),
}));

import '@/i18n';
import { SystemPromptPanel } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  window.localStorage.clear();
  document.body.replaceChildren();
});

describe('system prompt Markdown document viewer', () => {
  it('uses the real Markdown renderer by default and a read-only source viewer on demand', async () => {
    const content = '# Runtime\n\nThis is **rendered** content.';
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <SystemPromptPanel
              prompt={content}
              documentKey="hidden-prompt:1"
              resourceKind="hidden-prompt-section"
            />
          </TooltipProvider>,
        );
      });

      const rendered = container.querySelector('[data-readonly-markdown-mode="rendered"]');
      expect(rendered?.querySelector('h1')?.textContent).toContain('Runtime');
      expect(rendered?.querySelector('strong')?.textContent).toBe('rendered');
      expect(container.querySelector('[data-testid="markdown-source-viewer"]')).toBeNull();

      await act(async () => {
        container.querySelector<HTMLButtonElement>('[aria-label="切换到源码模式"]')?.click();
      });

      const source = container.querySelector('[data-testid="markdown-source-viewer"]');
      expect(container.querySelector('[data-readonly-markdown-mode="raw"]')).not.toBeNull();
      expect(source?.textContent).toBe(content);
      expect(source?.getAttribute('data-editable')).toBe('false');
      expect(source?.getAttribute('data-markdown-mode')).toBe('null');

      await act(async () => {
        container.querySelector<HTMLButtonElement>('[aria-label="渲染 Markdown"]')?.click();
      });

      expect(container.querySelector('[data-readonly-markdown-mode="rendered"] strong')?.textContent).toBe('rendered');
      expect(container.querySelector('[data-testid="markdown-source-viewer"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
