/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import {
  CollisionBoundaryContext,
  conversationOverlayCollisionBoundary,
  overlayCollisionPadding,
  useOverlayPositioning,
} from '@/lib/portal-container';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function Probe({ explicit }: { explicit?: HTMLElement | null }) {
  const positioning = useOverlayPositioning(explicit);
  return (
    <div
      data-constrain={String(positioning.constrainWidth)}
      data-has-boundary={String(Boolean(positioning.collisionBoundary))}
      data-has-collision-prop={String('collisionBoundary' in positioning.collisionProps)}
      data-constraint={String(positioning.constraintBoundary ?? '')}
      data-padding={String(positioning.collisionPadding ?? '')}
    />
  );
}

describe('overlay collision positioning', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    document.body.replaceChildren();
  });

  it('does not pass collision props without a boundary', async () => {
    await act(async () => {
      root.render(<Probe />);
    });
    const probe = host.querySelector('div');
    expect(probe?.getAttribute('data-constrain')).toBe('false');
    expect(probe?.getAttribute('data-has-boundary')).toBe('false');
    expect(probe?.getAttribute('data-has-collision-prop')).toBe('false');
    expect(probe?.getAttribute('data-constraint')).toBe('');
  });

  it('inherits the conversation pane boundary from context', async () => {
    const boundary = document.createElement('main');
    await act(async () => {
      root.render(
        <CollisionBoundaryContext.Provider value={boundary}>
          <Probe />
        </CollisionBoundaryContext.Provider>,
      );
    });
    const probe = host.querySelector('div');
    expect(probe?.getAttribute('data-constrain')).toBe('true');
    expect(probe?.getAttribute('data-has-boundary')).toBe('true');
    expect(probe?.getAttribute('data-has-collision-prop')).toBe('true');
    expect(probe?.getAttribute('data-constraint')).toBe(conversationOverlayCollisionBoundary);
    expect(probe?.getAttribute('data-padding')).toBe(String(overlayCollisionPadding));
  });

  it('lets an explicit null opt out of the context boundary', async () => {
    const boundary = document.createElement('main');
    await act(async () => {
      root.render(
        <CollisionBoundaryContext.Provider value={boundary}>
          <Probe explicit={null} />
        </CollisionBoundaryContext.Provider>,
      );
    });
    const probe = host.querySelector('div');
    expect(probe?.getAttribute('data-constrain')).toBe('false');
    expect(probe?.getAttribute('data-has-collision-prop')).toBe('false');
    expect(probe?.getAttribute('data-constraint')).toBe('');
  });
});
