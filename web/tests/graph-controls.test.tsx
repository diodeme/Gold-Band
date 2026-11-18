/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ReactFlowProvider } from '@xyflow/react';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { GraphControls } from '@/components/GraphControls';
import { TooltipProvider } from '@/components/ui/tooltip';

function renderControls(props: Partial<Parameters<typeof GraphControls>[0]> = {}) {
  const container = document.createElement('div');
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(
      <ReactFlowProvider>
        <TooltipProvider>
          <GraphControls
            onZoomIn={vi.fn()}
            onZoomOut={vi.fn()}
            onFitView={vi.fn()}
            {...props}
          />
        </TooltipProvider>
      </ReactFlowProvider>,
    );
  });
  return {
    container,
    cleanup: () => {
      act(() => root.unmount());
      container.remove();
    },
  };
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('GraphControls direction and fullscreen entries', () => {
  it('renders only the zoom trio without optional controls', () => {
    const { container, cleanup } = renderControls();
    try {
      const buttons = container.querySelectorAll('button');
      expect(buttons.length).toBe(3);
      expect(container.querySelector('[aria-label="graph.direction"]')).toBeNull();
      expect(container.querySelector('[aria-label="graph.fullscreen"]')).toBeNull();
    } finally {
      cleanup();
    }
  });

  it('renders the direction switcher and forwards preference changes', async () => {
    const onChange = vi.fn();
    const { container, cleanup } = renderControls({ direction: { preference: 'auto', onChange } });
    try {
      const trigger = container.querySelector('[aria-label="graph.direction"]') as HTMLElement;
      expect(trigger).toBeTruthy();
      // Radix DropdownMenu opens on pointerdown.
      await act(async () => {
        trigger.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }));
      });
      const item = document.querySelector('[aria-label="graph.directionVertical"]') as HTMLElement;
      expect(item).toBeTruthy();
      await act(async () => {
        item.click();
      });
      expect(onChange).toHaveBeenCalledWith('vertical');
    } finally {
      cleanup();
    }
  });

  it('renders the fullscreen button and invokes the handler', () => {
    const onFullscreen = vi.fn();
    const { container, cleanup } = renderControls({ onFullscreen });
    try {
      const button = container.querySelector('[aria-label="graph.fullscreen"]') as HTMLElement;
      expect(button).toBeTruthy();
      act(() => {
        button.click();
      });
      expect(onFullscreen).toHaveBeenCalledTimes(1);
    } finally {
      cleanup();
    }
  });

  it('flips the fullscreen toggle into its exit affordance while active', () => {
    const onFullscreen = vi.fn();
    const { container, cleanup } = renderControls({ onFullscreen, fullscreenActive: true });
    try {
      // The active state relabels the button to the exit action so screen
      // readers and tooltips both announce what clicking will do.
      expect(container.querySelector('[aria-label="graph.exitFullscreen"]')).toBeTruthy();
      expect(container.querySelector('[aria-label="graph.fullscreen"]')).toBeNull();
      const button = container.querySelector('[aria-label="graph.exitFullscreen"]') as HTMLElement;
      act(() => {
        button.click();
      });
      expect(onFullscreen).toHaveBeenCalledTimes(1);
    } finally {
      cleanup();
    }
  });
});
