/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/ui/tabs', async () => {
  const React = await import('react');
  const Context = React.createContext('branches');
  return {
    Tabs: ({ value, children }: { value: string; children: React.ReactNode }) => <Context.Provider value={value}><div>{children}</div></Context.Provider>,
    TabsList: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
    TabsTrigger: ({ children }: { children: React.ReactNode }) => <button type="button">{children}</button>,
    TabsContent: ({ value, className, children }: { value: string; className?: string; children: React.ReactNode }) => {
      const active = React.useContext(Context);
      return active === value ? <div className={className}>{children}</div> : null;
    },
  };
});

vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ className, children }: { className?: string; children: React.ReactNode }) => <div className={className}>{children}</div>,
}));

vi.mock('@/components/ui/dropdown-menu', () => ({
  DropdownMenu: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  DropdownMenuContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DropdownMenuItem: ({ children, disabled, onSelect }: { children: React.ReactNode; disabled?: boolean; onSelect?: () => void }) => <button type="button" disabled={disabled} onClick={onSelect}>{children}</button>,
  DropdownMenuSeparator: () => null,
}));

vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogContent: ({ children }: { children: React.ReactNode }) => <div role="dialog">{children}</div>,
  DialogDescription: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogFooter: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogHeader: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  DialogTitle: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { SourceControlRepositoryView } from '@/components/workspace/source-control/SourceControlRepositoryView';
import type { GitSourceControlSnapshotVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
  vi.clearAllMocks();
});

describe('source control repository view', () => {
  it('keeps long stash rows inside the narrow repository pane', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const longMessage = 'WIP on main: a very long stash message that must not widen the client repository pane';
    await act(async () => {
      root.render(<SourceControlRepositoryView {...props({
        stashes: [{ refName: 'stash@{123}', oid: 'stash-oid', baseOid: 'base', message: longMessage, author: { name: 'A', timestamp: '' }, createdAt: '' }],
      })} activeTab="stashes" />);
    });

    const message = Array.from(container.querySelectorAll('span')).find((item) => item.textContent === longMessage)!;
    const row = message.parentElement!;
    expect(row.className).toContain('w-full');
    expect(row.className).toContain('min-w-0');
    expect(row.className).toContain('max-w-full');
    expect(row.className).toContain('overflow-hidden');
    expect(message.className).toContain('truncate');
    expect(Array.from(row.querySelectorAll('span')).find((item) => item.textContent === 'stash@{123}')?.className).toContain('max-w-[32%]');
    await act(async () => root.unmount());
  });

  it('disables current-worktree deletion and confirms safe linked-worktree removal', async () => {
    const onMutation = vi.fn();
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(<SourceControlRepositoryView {...props({
        worktrees: [worktree('D:/repo', 'refs/heads/main', true), worktree('D:/repo-linked', 'refs/heads/topic', false)],
      }, onMutation)} activeTab="worktrees" />);
    });

    const removeButtons = Array.from(container.querySelectorAll<HTMLButtonElement>('button')).filter((button) => button.textContent === 'sourceControl.removeWorktree');
    expect(removeButtons).toHaveLength(2);
    expect(removeButtons[0].disabled).toBe(true);
    expect(removeButtons[1].disabled).toBe(false);
    await act(async () => removeButtons[1].click());
    expect(container.querySelector('[role="dialog"]')?.textContent).toContain('D:/repo-linked');
    const confirm = Array.from(container.querySelectorAll<HTMLButtonElement>('[role="dialog"] button')).find((button) => button.textContent === 'common.delete')!;
    await act(async () => confirm.click());
    expect(onMutation).toHaveBeenCalledWith({ kind: 'worktree-remove', path: 'D:/repo-linked' });
    await act(async () => root.unmount());
  });
});

function props(overrides: Partial<GitSourceControlSnapshotVm>, onMutation = vi.fn()) {
  const snapshot = {
    repository: {
      projectId: 'project', commonDir: 'D:/repo/.git', workspacePath: 'D:/repo', repoRoot: 'D:/repo',
      revision: 'revision', currentBranch: 'main', headOid: 'head', remotes: [], upstream: null,
    },
    status: { conflicts: [], staged: [], unstaged: [], untracked: [], branch: { head: 'main', ahead: 0, behind: 0 }, snapshotRevision: 'revision' },
    refs: [], worktrees: [], stashes: [], ...overrides,
  } as unknown as GitSourceControlSnapshotVm;
  return {
    snapshot, busyActionKind: null, busyActionPath: null, locked: false, onMutation, onOperation: vi.fn(),
    activeTab: 'branches' as const, onTabChange: vi.fn(),
  };
}

function worktree(path: string, branch: string, main: boolean) {
  return { path, headOid: 'head', branch, main, detached: false, locked: false, prunable: false, ownership: 'user' as const };
}
