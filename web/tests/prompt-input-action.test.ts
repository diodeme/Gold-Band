import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { PromptInput, PromptInputAction } from '@/components/prompt-kit/prompt-input';

describe('PromptInputAction', () => {
  it('uses the shared Tooltip trigger without emitting a native title', () => {
    const html = renderToStaticMarkup(
      React.createElement(
        PromptInput,
        null,
        React.createElement(
          PromptInputAction,
          { tooltip: 'Send' },
          React.createElement('button', { type: 'button', 'aria-label': 'Send' }, 'Send'),
        ),
      ),
    );

    expect(html).toContain('data-slot="prompt-input-action"');
    expect(html).toContain('data-state="closed"');
    expect(html).not.toContain('title=');
  });
});
