/** @vitest-environment jsdom */

import React from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const contentProps: Array<Record<string, unknown>> = [];
vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DialogContent: (props: Record<string, unknown>) => {
    contentProps.push(props);
    return <div {...(props as React.HTMLAttributes<HTMLDivElement>)}>{props.children as React.ReactNode}</div>;
  },
  DialogDescription: () => null,
  DialogHeader: () => null,
  DialogTitle: () => null,
}));

import { GraphFullscreenDialog } from '@/components/GraphFullscreenDialog';

afterEach(() => {
  contentProps.length = 0;
  document.body.replaceChildren();
});

describe('GraphFullscreenDialog surface', () => {
  it('overrides the dialog material recipe with an opaque inline background', () => {
    // Regression: the fullscreen layer painted the theme's translucent
    // dialog material (texture + backdrop blur) over the dark overlay —
    // a dark, grid-textured backdrop even in light mode. The class-based
    // `!bg-none` override never shipped (the utility was not generated),
    // so the fix must be inline styles, asserted here verbatim.
    const container = document.createElement('div');
    document.body.append(container);
    const root: Root = createRoot(container);
    React.act(() => {
      root.render(
        <GraphFullscreenDialog open onOpenChange={() => {}} title="t">
          <p>graph</p>
        </GraphFullscreenDialog>,
      );
    });
    const props = contentProps.at(-1)! as { style?: React.CSSProperties };
    expect(props.style?.backgroundColor).toBe('var(--background)');
    expect(props.style?.backgroundImage).toBe('none');
    expect(props.style?.backdropFilter).toBe('none');
    // Fade-only open/close: the default zoom-in-95 animation frames this
    // viewport-sized layer against the dark overlay, flashing black edges.
    const cls = String(props.className);
    expect(cls).toContain('data-[state=open]:zoom-in-100');
    expect(cls).not.toContain('zoom-in-95');
    React.act(() => root.unmount());
  });
});
