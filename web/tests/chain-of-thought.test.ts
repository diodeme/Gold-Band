import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import {
  ChainOfThought,
  ChainOfThoughtContent,
  ChainOfThoughtStep,
  ChainOfThoughtText,
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
  it('renders thought content as literal plain text instead of Markdown', () => {
    const html = renderToStaticMarkup(
      createElement(ChainOfThoughtText, null, '\n\n**Inspecting**\n- files\n\n'),
    );

    expect(html).toContain('**Inspecting**\n- files');
    expect(html).not.toContain('\n\n**Inspecting**');
    expect(html).not.toContain('- files\n\n');
    expect(html).toContain('whitespace-pre-wrap');
    expect(html).not.toContain('<strong>');
    expect(html).not.toContain('<li>');
  });

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
