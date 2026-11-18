/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import type { ConversationSidebarVm, ConversationTaskRowVm } from '@/types';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ children, viewportRef, ...props }: React.PropsWithChildren<{ viewportRef?: React.Ref<HTMLDivElement> }>) => (
    <div {...props}><div data-testid="viewport" ref={viewportRef}>{children}</div></div>
  ),
}));
import { ConversationSidebar } from '@/components/conversation/ConversationSidebar';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => { vi.restoreAllMocks(); document.body.replaceChildren(); });

const task: ConversationTaskRowVm = {
  projectId: 'b', taskId: 'task-old', taskUuid: 'uuid-old', title: 'Old search target',
  autoTitle: false, runMode: 'workflow', pinned: false, runs: [], runHistoryStatus: 'not-loaded',
  latestRun: { runId: 'run-001', status: 'completed', startedAt: '', updatedAt: '', resumable: false },
};
const vm: ConversationSidebarVm = {
  loadStatus: 'ready', workspaces: [
    { projectId: 'a', name: 'A', workspacePath: 'D:/A' },
    { projectId: 'b', name: 'B', workspacePath: 'D:/B' },
  ], pinRefs: [], pinnedTasks: [], pinnedTaskPage: { status: 'ready' },
  recentTasks: [], recentTaskPage: { status: 'not-loaded' },
  tasksByWorkspace: { a: [], b: [task] }, workspaceTaskPages: { a: { status: 'ready' }, b: { status: 'ready' } },
};
const callbacks = {
  onSelect: vi.fn(), onNewConversation: vi.fn(), onSearch: vi.fn(), onPinTask: vi.fn(),
  onUnpinTask: vi.fn(), onRenameTask: vi.fn(), onDeleteTask: vi.fn(), onRetryBootstrap: vi.fn(),
  onRequestWorkspaceTasks: vi.fn(), onRequestPinnedTasks: vi.fn(), onRequestRecentTasks: vi.fn(), onRequestTaskRuns: vi.fn(),
};

it('reveals a search conversation once, without expanding or loading run history', async () => {
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function () {
    const top = this.textContent === task.title ? 600 : 0;
    return { top, bottom: top + (top ? 30 : 200), height: top ? 30 : 200, left: 0, right: 200, width: 200, x: 0, y: top, toJSON: () => ({}) };
  });
  const request = { projectId: 'b', taskId: task.taskId, taskUuid: task.taskUuid, requestId: 1 };
  const active = { kind: 'conversation-run' as const, projectId: 'b', taskId: task.taskId, taskUuid: task.taskUuid, runId: 'run-001' };
  try {
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={vm} active={{ kind: 'conversation-home' }} defaultExpandedWorkspaceId="a" />));
    callbacks.onRequestTaskRuns.mockClear();
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={vm} active={active} workspaceRevealRequest={request} />));
    expect(container.querySelector('[data-conversation-workspace-id="b"]')?.getAttribute('aria-expanded')).toBe('true');
    expect(callbacks.onRequestTaskRuns).not.toHaveBeenCalled();
    const row = [...container.querySelectorAll('div')].find((el) => el.classList.contains('group') && el.textContent === task.title)!;
    expect(row.classList.contains('bg-sidebar-accent/70')).toBe(true);
    const viewport = container.querySelector<HTMLDivElement>('[data-testid="viewport"]')!;
    expect(viewport.scrollTop).toBeGreaterThan(0);
    viewport.scrollTop = 0;
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={{ ...vm }} active={active} workspaceRevealRequest={{ ...request }} />));
    expect(viewport.scrollTop).toBe(0);
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={vm} active={active} workspaceRevealRequest={{ ...request, requestId: 2 }} />));
    expect(viewport.scrollTop).toBeGreaterThan(0);
  } finally { await act(async () => root.unmount()); }
});

it('keeps user scroll intent when a missing search row arrives late, and allows explicit retry', async () => {
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  const active = { kind: 'conversation-run' as const, projectId: 'b', taskId: task.taskId, taskUuid: task.taskUuid, runId: 'run-001' };
  const request = { projectId: 'b', taskId: task.taskId, taskUuid: task.taskUuid, requestId: 10, status: 'loading' as const };
  const onRetryReveal = vi.fn();
  try {
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={{ ...vm, tasksByWorkspace: { a: [], b: [] } }} active={active} workspaceRevealRequest={request} onRetryReveal={onRetryReveal} />));
    expect(container.querySelector('[role="status"]')?.textContent).toContain('conversation.sidebar.loadingConversations');
    const viewport = container.querySelector<HTMLDivElement>('[data-testid="viewport"]')!;
    await act(async () => viewport.dispatchEvent(new WheelEvent('wheel', { bubbles: true, deltaY: -100 })));
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function () {
      const isRow = this.dataset.conversationTaskRow === 'workspace';
      return { top: isRow ? 600 : 0, bottom: isRow ? 632 : 200, height: isRow ? 32 : 200, left: 0, right: 200, width: 200, x: 0, y: 0, toJSON: () => ({}) };
    });
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={vm} active={active} workspaceRevealRequest={{ ...request, status: undefined }} />));
    expect(container.querySelector('[data-conversation-task-row="workspace"]')?.textContent).toContain(task.title);
    expect(viewport.scrollTop).toBe(0);
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={vm} active={active} workspaceRevealRequest={{ ...request, requestId: 11, status: 'error' }} onRetryReveal={onRetryReveal} />));
    const retry = [...container.querySelectorAll('button')].find((button) => button.textContent === 'conversation.sidebar.retryConversations')!;
    await act(async () => retry.click());
    expect(onRetryReveal).toHaveBeenCalledOnce();
  } finally { await act(async () => root.unmount()); }
});

it('does not replay a consumed reveal when the sidebar remounts', async () => {
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function () {
    const isRow = this.dataset.conversationTaskRow === 'workspace';
    return { top: isRow ? 600 : 0, bottom: isRow ? 632 : 200, height: isRow ? 32 : 200, left: 0, right: 200, width: 200, x: 0, y: 0, toJSON: () => ({}) };
  });
  const request = { projectId: 'b', taskId: task.taskId, requestId: 20, scrollHandled: true };
  try {
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={vm} active={{ kind: 'conversation-run', projectId: 'b', taskId: task.taskId, taskUuid: task.taskUuid, runId: 'run-001' }} workspaceRevealRequest={request} />));
    expect(container.querySelector('[data-testid="viewport"]')?.scrollTop).toBe(0);
  } finally { await act(async () => root.unmount()); }
});
