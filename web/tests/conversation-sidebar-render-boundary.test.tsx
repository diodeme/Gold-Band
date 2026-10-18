/** @vitest-environment jsdom */

import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

// i18next keeps `t` stable; each sidebar-level render calls useTranslation once.
const i18n = vi.hoisted(() => ({ calls: 0, t: (key: string) => key }));
vi.mock('react-i18next', () => ({
  useTranslation: () => {
    i18n.calls += 1;
    return { t: i18n.t };
  },
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, saveConversationPreference: vi.fn().mockResolvedValue(undefined) };
});

vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

// Every TaskRow render evaluates its activity exactly once, keyed by `projectId:taskId`.
const rowRenders = vi.hoisted(() => new Map<string, number>());
vi.mock('@/lib/conversation-sidebar-activity', async () => {
  const actual = await vi.importActual<typeof import('@/lib/conversation-sidebar-activity')>('@/lib/conversation-sidebar-activity');
  return {
    ...actual,
    useConversationTaskActive: (...args: Parameters<typeof actual.useConversationTaskActive>) => {
      rowRenders.set(args[1], (rowRenders.get(args[1]) ?? 0) + 1);
      return actual.useConversationTaskActive(...args);
    },
  };
});

import { ConversationSidebar } from '@/components/conversation/ConversationSidebar';
import { useStableCallbacks } from '@/hooks/use-stable-callbacks';
import type { ConversationSidebarVm, ConversationTaskRowVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const noop = () => {};
const callbacks = {
  onSelect: noop,
  onNewConversation: noop,
  onSearch: noop,
  onPinTask: noop,
  onUnpinTask: noop,
  onRenameTask: noop,
  onDeleteTask: noop,
  onRetryBootstrap: noop,
  onRequestWorkspaceTasks: noop,
  onRequestPinnedTasks: noop,
  onRequestTaskRuns: noop,
  onSetResident: vi.fn().mockResolvedValue(undefined),
};

function task(taskId: string): ConversationTaskRowVm {
  return {
    projectId: 'workspace-a', taskId, taskUuid: `uuid-${taskId}`, title: taskId, autoTitle: false,
    runMode: 'direct', latestRun: null, runs: [], runHistoryStatus: 'ready', runsNextCursor: null,
    pinned: false, pinnedOrder: null, lastActivityAt: '2026-10-10T00:00:00Z',
  };
}

function sidebarVm(): ConversationSidebarVm {
  return {
    loadStatus: 'ready',
    workspaces: [{ projectId: 'workspace-a', workspacePath: 'D:\\workspace-a', name: 'Workspace A' }],
    pinRefs: [],
    pinnedTasks: [],
    pinnedTaskPage: { status: 'ready', nextCursor: null },
    tasksByWorkspace: { 'workspace-a': [task('task-1'), task('task-2'), task('task-3')] },
    workspaceTaskPages: { 'workspace-a': { status: 'ready', nextCursor: null } },
    lastActiveWorkspaceId: 'workspace-a',
  };
}

const active = { kind: 'conversation-home' } as const;

afterEach(() => {
  document.body.replaceChildren();
  rowRenders.clear();
});

async function mount(node: React.ReactNode) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => root.render(node));
  return { container, root };
}

describe('conversation sidebar render boundary', () => {
  it('does not re-render when the parent re-renders with unchanged data and stable commands', async () => {
    let bump!: () => void;
    function Parent() {
      const [, setTick] = useState(0);
      bump = () => setTick((value) => value + 1);
      return (
        <ConversationSidebar {...callbacks} vm={vm} active={active} />
      );
    }
    const vm = sidebarVm();
    const { root } = await mount(<Parent />);
    expect(rowRenders.get('workspace-a:task-1')).toBeGreaterThan(0);
    const sidebarRenders = i18n.calls;
    rowRenders.clear();

    await act(async () => bump());
    await act(async () => bump());

    expect(i18n.calls).toBe(sidebarRenders);
    expect(rowRenders.size).toBe(0);
    await act(async () => root.unmount());
  });

  it('re-renders only the row whose task changed and keeps the other rows mounted', async () => {
    const vm = sidebarVm();
    const { container, root } = await mount(<ConversationSidebar {...callbacks} vm={vm} active={active} />);
    const rowOf = (title: string) => [...container.querySelectorAll('span.truncate')]
      .find((span) => span.textContent === title)
      ?.closest('[data-slot="context-menu-trigger"]') ?? null;
    const untouched = rowOf('task-2');
    rowRenders.clear();

    const [first, ...rest] = vm.tasksByWorkspace['workspace-a'];
    const next = { ...vm, tasksByWorkspace: { 'workspace-a': [{ ...first, title: 'renamed' }, ...rest] } };
    await act(async () => root.render(<ConversationSidebar {...callbacks} vm={next} active={active} />));

    expect([...rowRenders.keys()]).toEqual(['workspace-a:task-1']);
    expect(container.textContent).toContain('renamed');
    expect(rowOf('task-2')).toBe(untouched);
    await act(async () => root.unmount());
  });
});

describe('useStableCallbacks', () => {
  it('keeps one identity across renders and calls the latest handler', async () => {
    const seen: Array<{ run: () => string }> = [];
    const calls: string[] = [];
    function Probe({ label }: { label: string }) {
      const commands = useStableCallbacks({ run: () => { calls.push(label); return label; } });
      seen.push(commands);
      return null;
    }
    const { root } = await mount(<Probe label="first" />);
    await act(async () => root.render(<Probe label="second" />));

    expect(seen[0]).toBe(seen.at(-1));
    expect(seen[0].run()).toBe('second');
    expect(calls).toEqual(['second']);
    await act(async () => root.unmount());
  });
});
