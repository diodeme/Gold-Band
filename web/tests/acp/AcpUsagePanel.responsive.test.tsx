/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  initReactI18next: { type: '3rdParty', init: () => {} },
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/ui/tooltip', async () => {
  const ReactModule = await import('react');
  const TooltipContext = ReactModule.createContext<{
    open: boolean;
    setOpen: (open: boolean) => void;
  } | null>(null);
  return {
    Tooltip: ({ children }: { children: React.ReactNode }) => {
      const [open, setOpen] = ReactModule.useState(false);
      return ReactModule.createElement(TooltipContext.Provider, { value: { open, setOpen } }, children);
    },
    TooltipTrigger: ({ children }: { children: React.ReactElement<{ onFocus?: React.FocusEventHandler }> }) => {
      const context = ReactModule.useContext(TooltipContext);
      return ReactModule.cloneElement(children, {
        onFocus: (event: React.FocusEvent) => {
          children.props.onFocus?.(event);
          context?.setOpen(true);
        },
      });
    },
    TooltipContent: ({ children }: { children: React.ReactNode }) => {
      const context = ReactModule.useContext(TooltipContext);
      return context?.open ? ReactModule.createElement('div', { 'data-test-tooltip-content': 'true' }, children) : null;
    },
  };
});

vi.mock('@/components/git/GitBranchSelector', async () => {
  const { Popover, PopoverContent, PopoverTrigger } = await import('@/components/ui/popover');
  return {
    GitBranchSelector: () => (
      <Popover>
        <PopoverTrigger asChild>
          <button type="button" data-test-branch-selector="true">gold-band/codex-responsive</button>
        </PopoverTrigger>
        <PopoverContent data-test-branch-options="true">branch options</PopoverContent>
      </Popover>
    ),
  };
});

import { AcpUsagePanel } from '@/components/acp/AcpUsagePanel';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class ControlledResizeObserver implements ResizeObserver {
  static instances: ControlledResizeObserver[] = [];
  readonly callback: ResizeObserverCallback;
  target: Element | null = null;

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    ControlledResizeObserver.instances.push(this);
  }

  observe(target: Element) { this.target = target; }
  unobserve() {}
  disconnect() { this.target = null; }

  flush(width: number) {
    if (!this.target) throw new Error('ResizeObserver has no observed target');
    Object.defineProperty(this.target, 'clientWidth', { configurable: true, value: width });
    this.callback([{ target: this.target } as ResizeObserverEntry], this);
  }
}

let animationFrameSequence = 0;
let animationFrames = new Map<number, FrameRequestCallback>();

function flushAnimationFrames() {
  const pending = [...animationFrames.entries()];
  animationFrames = new Map();
  for (const [, callback] of pending) callback(performance.now());
}

beforeEach(() => {
  ControlledResizeObserver.instances = [];
  animationFrames = new Map();
  animationFrameSequence = 0;
  vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    const id = ++animationFrameSequence;
    animationFrames.set(id, callback);
    return id;
  });
  vi.stubGlobal('cancelAnimationFrame', (id: number) => {
    animationFrames.delete(id);
  });
});

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe('AcpUsagePanel responsive overflow', () => {
  it('moves each rightmost item once into More and restores the full inline layout after re-expansion', async () => {
    const rail = document.createElement('div');
    Object.defineProperty(rail, 'clientWidth', { configurable: true, value: 600 });
    document.body.append(rail);
    const root = createRoot(rail);

    try {
      await act(async () => {
        root.render(
          <AcpUsagePanel
            usage={{ used: 32_000, size: 100_000 }}
            processingLabel="Agent 调起中"
            sessionSeconds={141}
            worktreePath="D:/repo/.gold-band/worktrees/worker"
            branchProjectId="project-1"
            managedWorktreeBranch="gold-band/codex-responsive"
          />,
        );
      });

      const panel = rail.querySelector<HTMLElement>('[data-acp-session-info="true"]');
      const observer = ControlledResizeObserver.instances.at(-1);
      expect(panel?.dataset.acpSessionInfoLayout).toBe('full');
      expect(rail.querySelector('[data-acp-session-info-more="true"]')).toBeNull();

      await act(async () => {
        observer?.flush(500);
        flushAnimationFrames();
      });
      expect(panel?.dataset.acpSessionInfoLayout).toBe('branch-overflow');
      expect(rail.querySelector('[data-acp-session-info-item="worktree"]')).not.toBeNull();
      expect(rail.querySelector('[data-acp-session-info-item="branch"]')).toBeNull();

      await act(async () => {
        observer?.flush(420);
        flushAnimationFrames();
      });
      expect(panel?.dataset.acpSessionInfoLayout).toBe('workspace-overflow');
      expect(rail.querySelector('[data-acp-session-info-item="worktree"]')).toBeNull();

      const more = rail.querySelector<HTMLButtonElement>('[data-acp-session-info-more="true"]');
      await act(async () => more?.click());
      expect(document.querySelector('[data-test-tooltip-content="true"]')).toBeNull();
      const overflow = document.querySelector<HTMLElement>('[data-acp-session-info-overflow="true"]');
      expect(overflow?.getAttribute('data-align')).toBe('start');
      expect(overflow?.querySelector('[data-acp-session-info-item="worktree"]')).not.toBeNull();
      expect(overflow?.querySelector('[data-acp-session-info-item="branch"]')).not.toBeNull();
      expect(document.querySelectorAll('[data-acp-session-info-item="branch"]')).toHaveLength(1);

      const overflowBranch = overflow?.querySelector<HTMLElement>('[data-acp-session-info-item="branch"]');
      expect(overflowBranch?.className).toContain('[&_[data-git-branch-selector]]:w-full');
      expect(overflowBranch?.className).toContain('[&_[data-git-branch-selector]]:max-w-none');
      expect(overflowBranch?.className).toContain('[&_[data-git-branch-selector]]:justify-start');
      const branchSelector = document.querySelector<HTMLButtonElement>('[data-test-branch-selector="true"]');
      await act(async () => branchSelector?.click());
      expect(document.querySelector('[data-test-branch-options="true"]')).not.toBeNull();
      expect(document.querySelector('[data-acp-session-info-overflow="true"]')).not.toBeNull();

      await act(async () => {
        observer?.flush(320);
        flushAnimationFrames();
      });
      expect(panel?.dataset.acpSessionInfoLayout).toBe('context-overflow');
      expect(rail.querySelector('[data-acp-session-info-item="context"]')).toBeNull();
      expect(document.querySelector('[data-acp-session-info-overflow="true"] [data-acp-session-info-item="context"]')).not.toBeNull();

      await act(async () => {
        observer?.flush(600);
        flushAnimationFrames();
      });
      expect(panel?.dataset.acpSessionInfoLayout).toBe('full');
      expect(rail.querySelector('[data-acp-session-info-more="true"]')).toBeNull();
      expect(rail.querySelector('[data-acp-session-info-item="context"]')).not.toBeNull();
      expect(rail.querySelector('[data-acp-session-info-item="worktree"]')).not.toBeNull();
      expect(rail.querySelector('[data-acp-session-info-item="branch"]')).not.toBeNull();
      expect(document.querySelectorAll('[data-acp-session-info-item="branch"]')).toHaveLength(1);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
