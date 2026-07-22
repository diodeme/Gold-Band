import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import {
  ChainOfThought,
  ChainOfThoughtContent,
  ChainOfThoughtStep,
} from '@/components/prompt-kit/chain-of-thought';

function renderClosedContent(preserveMount: boolean) {
  return renderToStaticMarkup(
    createElement(
      ChainOfThought,
      null,
      createElement(
        ChainOfThoughtStep,
        { open: false },
        createElement(
          ChainOfThoughtContent,
          { animated: false, preserveMount },
          createElement('span', null, 'streaming thought'),
        ),
      ),
    ),
  );
}

describe('prompt-kit ChainOfThoughtContent', () => {
  it('keeps active streaming content mounted while closed without taking layout space', () => {
    const html = renderClosedContent(true);

    expect(html).toContain('streaming thought');
    expect(html).toContain('data-state="closed"');
    expect(html).toContain('data-[state=closed]:hidden');
  });

  it('uses normal unmount behavior for completed closed content', () => {
    expect(renderClosedContent(false)).not.toContain('streaming thought');
  });
});
