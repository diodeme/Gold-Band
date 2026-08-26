/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { GitBranchPickerSnapshotVm } from '@/types';

const getSnapshot = vi.fn<() => Promise<GitBranchPickerSnapshotVm>>();
const changeBranch = vi.fn();
const openExternalUrl = vi.fn();
const translate = (key: string, params?: Record<string, unknown>) => {
  if (key === 'conversation.branchPicker.dirtyFiles') return `未提交：${params?.count} 个文件`;
  return key;
};

vi.mock('@/api', () => ({
  getGitBranchPickerSnapshot: (...args: unknown[]) => getSnapshot(...args as []),
  changeGitBranch: (...args: unknown[]) => changeBranch(...args),
  openExternalUrl: (...args: unknown[]) => openExternalUrl(...args),
}));

vi.mock('react-i18next', () => ({
  initReactI18next: { type: '3rdParty', init: () => {} },
  useTranslation: () => ({
    t: translate,
  }),
}));

import { GitBranchSelector } from '@/components/git/GitBranchSelector';
import {
  GitBranchPickerSnapshotProvider,
  GitBranchPickerSnapshotStore,
} from '@/components/git/GitBranchPickerSnapshotContext';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function snapshot(overrides: Partial<GitBranchPickerSnapshotVm> = {}): GitBranchPickerSnapshotVm {
  return {
    workspacePath: 'D:/repo',
    currentBranch: 'main',
    headOid: 'head-main',
    revision: 'revision-main',
    dirtyFileCount: 12,
    operationInProgress: null,
    lock: { locked: false, owner: null, operation: null },
    branches: [
      { name: 'main', targetOid: 'head-main', checkedOutWorktreePaths: ['D:/repo'] },
      { name: 'feature/topic', targetOid: 'head-topic', checkedOutWorktreePaths: [] },
      { name: 'gold-band/conversation/very-long-runtime-branch', targetOid: 'head-run', checkedOutWorktreePaths: ['D:/runtime-worktree'] },
    ],
    ...overrides,
  };
}

beforeEach(() => {
  getSnapshot.mockResolvedValue(snapshot());
  changeBranch.mockResolvedValue(snapshot({
    currentBranch: 'feature/topic',
    headOid: 'head-topic',
    revision: 'revision-topic',
  }));
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  for (const method of ['hasPointerCapture', 'setPointerCapture', 'releasePointerCapture', 'scrollIntoView']) {
    if (method in HTMLElement.prototype) continue;
    Object.defineProperty(HTMLElement.prototype, method, {
      configurable: true,
      value: method === 'hasPointerCapture' ? () => false : () => {},
    });
  }
});

afterEach(() => {
  vi.clearAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

async function renderSelector(
  element: React.ReactNode,
  store = new GitBranchPickerSnapshotStore(),
) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(
    <TooltipProvider>
      <GitBranchPickerSnapshotProvider store={store}>{element}</GitBranchPickerSnapshotProvider>
    </TooltipProvider>,
  ));
  await act(async () => Promise.resolve());
  return { container, root, store };
}

function InlineBranchChangeHarness() {
  const [branch, setBranch] = React.useState<string | null>(null);
  return (
    <>
      <span data-selected-branch="true">{branch}</span>
      <GitBranchSelector projectId="project-a" onBranchChange={(next) => setBranch(next)} />
    </>
  );
}

describe('GitBranchSelector', () => {
  it('restores a cached workspace snapshot in the first render without a spinner', async () => {
    const store = new GitBranchPickerSnapshotStore();
    const cached = snapshot({ currentBranch: 'feature/cached', revision: 'revision-cached' });
    store.set('project-a', undefined, cached);
    const html = renderToStaticMarkup(
      <TooltipProvider>
        <GitBranchPickerSnapshotProvider store={store}>
          <GitBranchSelector projectId="project-a" />
        </GitBranchPickerSnapshotProvider>
      </TooltipProvider>,
    );

    expect(html).toContain('feature/cached');
    expect(html).not.toContain('animate-spin');
    expect(getSnapshot).not.toHaveBeenCalled();
  });

  it('loads the lightweight snapshot and performs a revision-checked real switch', async () => {
    const onBranchChange = vi.fn();
    const onMutationPendingChange = vi.fn();
    const { container, root } = await renderSelector(
      <GitBranchSelector
        projectId="project-a"
        onBranchChange={onBranchChange}
        onMutationPendingChange={onMutationPendingChange}
      />,
    );
    try {
      expect(getSnapshot).toHaveBeenCalledWith('project-a', undefined);
      const trigger = container.querySelector<HTMLButtonElement>('[data-git-branch-selector="editable"]')!;
      expect(trigger.textContent).toContain('main');
      expect(onBranchChange).toHaveBeenLastCalledWith('main');

      await act(async () => trigger.click());
      expect(document.body.textContent).toContain('未提交：12 个文件');
      const popover = document.body.querySelector<HTMLElement>('[data-git-branch-popover-align="start"]')!;
      const fixedAction = popover.querySelector<HTMLElement>('[data-git-branch-fixed-action="true"]')!;
      const commandList = popover.querySelector<HTMLElement>('[data-slot="command-list"]')!;
      expect(fixedAction).not.toBeNull();
      expect(commandList.contains(fixedAction)).toBe(false);
      expect(fixedAction.querySelector('button')?.classList.contains('justify-start')).toBe(true);
      const topic = [...document.body.querySelectorAll<HTMLElement>('[data-slot="command-item"]')]
        .find((item) => item.textContent?.includes('feature/topic'))!;
      await act(async () => topic.click());

      expect(changeBranch).toHaveBeenCalledWith('project-a', undefined, {
        kind: 'switch',
        name: 'feature/topic',
        expectedRevision: 'revision-main',
      });
      expect(onBranchChange).toHaveBeenLastCalledWith('feature/topic');
      expect(onMutationPendingChange).toHaveBeenCalledWith(true);
      expect(onMutationPendingChange).toHaveBeenLastCalledWith(false);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not reload the snapshot when a parent rerender replaces the notification callback', async () => {
    const { container, root } = await renderSelector(<InlineBranchChangeHarness />);
    try {
      expect(container.querySelector('[data-selected-branch="true"]')?.textContent).toBe('main');
      expect(getSnapshot).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows an explicit unsupported Git version state instead of an empty branch list', async () => {
    getSnapshot.mockRejectedValueOnce({
      code: 'git.version-unsupported',
      params: { installedVersion: '2.35.9', minimumVersion: '2.36.0' },
    });
    const store = new GitBranchPickerSnapshotStore();
    store.set('project-a', undefined, snapshot({ currentBranch: 'stale-branch' }));
    const { container, root } = await renderSelector(
      <GitBranchSelector projectId="project-a" />,
      store,
    );
    try {
      await act(async () => Promise.resolve());
      const trigger = container.querySelector<HTMLButtonElement>('[data-git-branch-selector="editable"]')!;
      expect(trigger.textContent).toContain('conversation.branchPicker.versionUnsupportedLabel');
      await act(async () => trigger.click());
      expect(document.body.querySelector('[data-git-version-capability-error="git.version-unsupported"]')).not.toBeNull();
      expect(document.body.textContent).toContain('conversation.branchPicker.versionUnsupportedTitle');
      expect(document.body.textContent).not.toContain('conversation.branchPicker.empty');
      expect(document.body.querySelector('[data-slot="command-input"]')).toBeNull();
      expect(store.get('project-a', undefined)).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps a managed Worktree branch read-only and exposes its full truncated name', async () => {
    const branch = 'gold-band/conversation/very-long-managed-worktree-branch';
    const { container, root } = await renderSelector(
      <GitBranchSelector projectId="project-a" readOnlyBranch={branch} variant="session" />,
    );
    try {
      const value = container.querySelector<HTMLElement>('[data-git-branch-selector="read-only"]')!;
      expect(value.textContent).toContain(branch);
      expect(value.querySelector('span')?.classList.contains('truncate')).toBe(true);
      expect(getSnapshot).not.toHaveBeenCalled();
      await act(async () => value.focus());
      expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent).toBe(branch);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('uses the elapsed-value foreground tone for an editable session branch', async () => {
    const { container, root } = await renderSelector(
      <GitBranchSelector projectId="project-a" variant="session" />,
    );
    try {
      const trigger = container.querySelector<HTMLButtonElement>('[data-git-branch-selector="editable"]')!;
      expect(trigger.classList.contains('text-foreground/80')).toBe(true);
      expect(trigger.classList.contains('text-muted-foreground')).toBe(false);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows the full editable branch name only when the visible value is truncated', async () => {
    const branch = 'feature/a-very-long-editable-branch-name';
    getSnapshot.mockResolvedValueOnce(snapshot({ currentBranch: branch }));
    const { container, root } = await renderSelector(<GitBranchSelector projectId="project-a" />);
    try {
      const value = container.querySelector<HTMLElement>('[data-git-branch-value="true"]')!;
      Object.defineProperty(value, 'clientWidth', { configurable: true, value: 80 });
      Object.defineProperty(value, 'scrollWidth', { configurable: true, value: 240 });
      const trigger = container.querySelector<HTMLButtonElement>('[data-git-branch-selector="editable"]')!;

      await act(async () => {
        trigger.dispatchEvent(new MouseEvent('pointerover', { bubbles: true }));
      });

      expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent).toBe(branch);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('uses an icon-only compact trigger and keeps its menu one click away', async () => {
    const { container, root } = await renderSelector(
      <GitBranchSelector projectId="project-a" responsiveContext />,
    );
    try {
      const trigger = container.querySelector<HTMLButtonElement>('[data-git-branch-selector="editable"]')!;
      const value = container.querySelector<HTMLElement>('[data-git-branch-value="true"]')!;
      expect(trigger.className.split(' ')).toContain('w-7');
      expect(trigger.className.split(' ')).toContain('@md/conversation-context:w-auto');
      expect(trigger.getAttribute('aria-label')).toBe('conversation.branchPicker.label: main');
      expect(value.className.split(' ')).toContain('hidden');
      expect(value.className.split(' ')).toContain('@md/conversation-context:inline');

      Object.defineProperties(value, {
        clientWidth: { configurable: true, value: 80 },
        scrollWidth: { configurable: true, value: 80 },
      });
      await act(async () => {
        trigger.dispatchEvent(new MouseEvent('pointerover', { bubbles: true }));
      });
      expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent).toBe('main');

      await act(async () => {
        trigger.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }));
      });
      expect(document.body.querySelector('[data-slot="tooltip-content"]')?.textContent).toBe('main');

      await act(async () => trigger.click());
      expect(document.body.querySelector('[data-git-branch-popover-align="start"]')).not.toBeNull();
      expect(document.body.querySelector('[data-slot="tooltip-content"]')).toBeNull();
      expect(trigger.dataset.gitBranchPopoverOpen).toBe('true');
      expect(trigger.className.split(' ')).toContain('data-[git-branch-popover-open=true]:bg-accent');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not restore pointer focus or reopen the compact tooltip after switching branches', async () => {
    const { container, root } = await renderSelector(
      <GitBranchSelector projectId="project-a" responsiveContext />,
    );
    try {
      const trigger = container.querySelector<HTMLButtonElement>('[data-git-branch-selector="editable"]')!;
      await act(async () => {
        trigger.dispatchEvent(new MouseEvent('pointerover', { bubbles: true }));
        trigger.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }));
        trigger.click();
      });

      const topic = [...document.body.querySelectorAll<HTMLElement>('[data-slot="command-item"]')]
        .find((item) => item.textContent?.includes('feature/topic'))!;
      await act(async () => {
        topic.dispatchEvent(new MouseEvent('pointerdown', { bubbles: true, button: 0 }));
        topic.click();
        await Promise.resolve();
      });

      expect(changeBranch).toHaveBeenCalledTimes(1);
      expect(document.body.querySelector('[data-git-branch-popover-align="start"]')).toBeNull();
      expect(document.body.querySelector('[data-slot="tooltip-content"]')).toBeNull();
      expect(document.activeElement).not.toBe(trigger);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('restores focus to the compact branch trigger after a keyboard close', async () => {
    const { container, root } = await renderSelector(
      <GitBranchSelector projectId="project-a" responsiveContext />,
    );
    try {
      const trigger = container.querySelector<HTMLButtonElement>('[data-git-branch-selector="editable"]')!;
      await act(async () => {
        trigger.focus();
        trigger.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Enter' }));
        trigger.click();
      });

      const input = document.body.querySelector<HTMLInputElement>('[data-slot="command-input"]')!;
      await act(async () => {
        input.focus();
        input.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, key: 'Escape' }));
      });

      expect(document.body.querySelector('[data-git-branch-popover-align="start"]')).toBeNull();
      expect(document.activeElement).toBe(trigger);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('disables branch mutations while Merge/Rebase or a Git lock is active', async () => {
    getSnapshot.mockResolvedValueOnce(snapshot({
      operationInProgress: { kind: 'rebase', currentOid: null, currentSubject: null },
      lock: { locked: true, owner: 'runtime', operation: 'runtime-worktree-create' },
    }));
    const { container, root } = await renderSelector(<GitBranchSelector projectId="project-a" />);
    try {
      await act(async () => container.querySelector<HTMLButtonElement>('[data-git-branch-selector="editable"]')!.click());
      const topic = [...document.body.querySelectorAll<HTMLElement>('[data-slot="command-item"]')]
        .find((item) => item.textContent?.includes('feature/topic'))!;
      expect(topic.getAttribute('data-disabled')).not.toBeNull();
      expect(document.body.textContent).toContain('conversation.branchPicker.operationInProgress');
      expect(changeBranch).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
