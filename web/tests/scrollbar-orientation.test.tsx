import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';

vi.mock('radix-ui', () => ({
  ScrollArea: {
    ScrollAreaScrollbar: ({ children, ...props }: React.HTMLAttributes<HTMLDivElement>) => <div {...props}>{children}</div>,
    ScrollAreaThumb: (props: React.HTMLAttributes<HTMLDivElement>) => <div {...props} />,
  },
}));

import { ScrollBar } from '../src/components/ui/scroll-area';

describe('ScrollBar orientation contract', () => {
  it.each(['horizontal', 'vertical'] as const)('applies minimum thumb length only along the %s scroll axis', (orientation) => {
    const markup = renderToStaticMarkup(<ScrollBar orientation={orientation} />);
    const thumb = markup.match(/data-slot="scroll-area-thumb" class="([^"]+)"/)![1];
    const axis = orientation === 'horizontal' ? 'w' : 'h';
    const crossAxis = orientation === 'horizontal' ? 'h' : 'w';
    expect(thumb).toContain(`min-${axis}-[var(--gb-scrollbar-min-length)]`);
    expect(thumb).not.toContain(`min-${crossAxis}-[var(--gb-scrollbar-min-length)]`);
  });
});
