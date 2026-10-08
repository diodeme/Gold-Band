/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  TURN_FILE_HOVER_CLOSE_DELAY_MS,
  TURN_FILE_HOVER_DEBUG_STORAGE_KEY,
  TURN_FILE_HOVER_OPEN_DELAY_MS,
  TurnFileChangesCard,
} from '@/components/acp/TurnFileChangesCard';
import { ACPMessageList } from '@/components/acp/ACPChatDialog';
import {
  DIFF_VIEW_SCAN_LIMIT,
  DIFF_VIEW_TIMEOUT_MS,
  shouldShowDiffChunkNavigation,
} from '@/components/workspace/files/TurnFileWorkspacePanel';
import {
  clearTurnFileChangeSetCacheForTests,
  fileChangeSetOwnerBranch,
  loadTurnFileChangeSet,
} from '@/lib/turn-file-change-set-cache';
import {
  clearTurnFileComparisonCacheForTests,
  loadTurnFileComparison,
  TURN_FILE_COMPARISON_CACHE_LIMIT,
} from '@/lib/turn-file-comparison-cache';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  RightWorkspaceProvider,
  useRightWorkspace,
} from '@/components/workspace/right-workspace-context';
import type {
  AcpUiEventVm,
  TurnFileChangeSetVm,
  TurnFileLocatorVm,
} from '@/types';

const turnFileDiffPreviewSource = readFileSync(
  resolve(process.cwd(), 'web/src/components/acp/TurnFileDiffPreview.tsx'),
  'utf8',
);

const { getFileComparisonMock, getTurnFileChangeSetMock } = vi.hoisted(() => ({
  getFileComparisonMock: vi.fn(),
  getTurnFileChangeSetMock: vi.fn(),
}));

vi.mock('@/api', () => ({
  getFileComparison: getFileComparisonMock,
  getTurnFileChangeSet: getTurnFileChangeSetMock,
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const locator: TurnFileLocatorVm = {
  projectId: 'project-1',
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
  branchId: 'agent-1',
};

function changeSet(id = 'change-set-1'): TurnFileChangeSetVm {
  return {
    id,
    turnId: 'turn-1',
    promptEventId: 'prompt-1',
    branchId: locator.branchId,
    status: 'finalized',
    startedAt: '2026-08-04T01:00:00Z',
    finishedAt: '2026-08-04T01:00:01Z',
    summary: {
      fileCount: 3,
      addedFiles: 1,
      modifiedFiles: 1,
      deletedFiles: 1,
      addedLines: 5,
      deletedLines: 2,
    },
    changes: [
      {
        id: 'added-1',
        changeKind: 'added',
        logicalPath: 'src/new.ts',
        text: true,
        addedLines: 3,
        deletedLines: 0,
      },
      {
        id: 'modified-1',
        changeKind: 'modified',
        logicalPath: 'src/existing.ts',
        text: true,
        addedLines: 2,
        deletedLines: 1,
      },
      {
        id: 'deleted-1',
        changeKind: 'deleted',
        logicalPath: 'src/deleted.ts',
        text: true,
        addedLines: 0,
        deletedLines: 1,
      },
    ],
    attachments: [],
    limitationCodes: [],
  };
}

function pointerEvent(id = 'change-set-1'): AcpUiEventVm {
  return {
    id: `file-change-${id}`,
    seq: 4,
    timestamp: '2026-08-04T01:00:01Z',
    kind: 'fileChangeSet',
    status: 'finalized',
    raw: {
      changeSetId: id,
      summary: changeSet(id).summary,
    },
  };
}

function comparison(changeId = 'modified-1') {
  return {
    changeSetId: 'change-set-1',
    changeId,
    path: `src/${changeId}.ts`,
    stats: { addedLines: 2, deletedLines: 1 },
    before: { content: 'const value = 1;\n' },
    after: { content: 'const value = 2;\n' },
    limitationCode: null,
  };
}

function WorkspaceProbe() {
  const workspace = useRightWorkspace();
  const active = workspace.tabs.find((tab) => tab.key === workspace.activeTabKey);
  return (
    <output
      data-active-kind={active?.kind ?? ''}
      data-active-key={active?.key ?? ''}
      data-active-branch={active && 'locator' in active && 'branchId' in active.locator ? active.locator.branchId : ''}
      data-active-attention={String(active?.attention ?? false)}
      data-tab-count={workspace.tabs.length}
    />
  );
}

async function renderCard(container: HTMLElement, event = pointerEvent()) {
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <RightWorkspaceProvider>
        <TooltipProvider>
          <TurnFileChangesCard event={event} locator={locator} />
          <WorkspaceProbe />
        </TooltipProvider>
      </RightWorkspaceProvider>,
    );
  });
  await act(async () => { await Promise.resolve(); });
  return root;
}

beforeEach(() => {
  clearTurnFileChangeSetCacheForTests();
  clearTurnFileComparisonCacheForTests();
  getFileComparisonMock.mockReset();
  getFileComparisonMock.mockImplementation((_locator, _changeSetId, changeId: string) => Promise.resolve(comparison(changeId)));
  getTurnFileChangeSetMock.mockReset();
  getTurnFileChangeSetMock.mockImplementation((_locator, id: string) => Promise.resolve(changeSet(id)));
});

afterEach(() => {
  window.localStorage.removeItem(TURN_FILE_HOVER_DEBUG_STORAGE_KEY);
  document.body.replaceChildren();
});

describe('turn file changes card', () => {
  it('keeps the recorded-change explanation in a focusable header tooltip', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    try {
      const trigger = container.querySelector<HTMLButtonElement>('[data-slot="card-title"] button[data-slot="tooltip-trigger"]');
      expect(trigger).not.toBeNull();
      const explanation = trigger!.getAttribute('aria-label');
      expect(explanation).toBeTruthy();
      expect(container.textContent).not.toContain(explanation);
      expect(trigger!.hasAttribute('title')).toBe(false);
      await act(async () => trigger!.focus());
      expect(document.querySelector('[role="tooltip"]')?.textContent).toBe(explanation);
      expect(getFileComparisonMock).not.toHaveBeenCalled();
      await act(async () => trigger!.blur());
    } finally { await act(async () => root.unmount()); }
  });

  it('groups disconnected edits by file with recorded totals and loads only the selected edit', async () => {
    const set = changeSet();
    set.summary = { ...set.summary, fileCount: 2, addedLines: 7, deletedLines: 2 };
    set.changes = [set.changes[0]!, set.changes[1]!, { ...set.changes[1]!, id: 'modified-2' }];
    getTurnFileChangeSetMock.mockResolvedValue(set);
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    try {
      const rows = [...container.querySelectorAll<HTMLElement>('[role="listitem"]')];
      expect(rows).toHaveLength(2);
      expect(getFileComparisonMock).not.toHaveBeenCalled();
      const group = container.querySelector<HTMLButtonElement>('[data-recorded-file-group]');
      expect(group).not.toBeNull();
      expect(group!.textContent).toContain('+4');
      expect(group!.textContent).toContain('-2');
      expect(group!.textContent).toContain('×2');
      // One row shape: single and grouped files share height and stat slots.
      expect(group).toBe(rows[1]);
      expect(group!.className).toBe(rows[0]!.className);
      // Stats stay the last column on every row, aligned with the header total;
      // the edit count is not reserved on rows that do not have one.
      for (const row of rows) expect(row.lastElementChild?.hasAttribute('data-file-stats')).toBe(true);
      expect(rows[0]!.querySelector('[data-file-edit-count]')).toBeNull();
      expect(group!.querySelector('[data-file-edit-count]')?.textContent).toBe('×2');
      expect(container.querySelector('[data-slot="card-header"]')?.textContent).toContain('+7');
      await act(async () => group!.click());
      expect(container.querySelectorAll('[data-recorded-edit]')).toHaveLength(2);
      expect(getFileComparisonMock).not.toHaveBeenCalled();
    } finally { await act(async () => root.unmount()); }
  });

  it('shows workspace files relative to the recorded root and keeps outside paths absolute', async () => {
    const set = changeSet();
    set.workspaceRoot = 'E:/Repo';
    set.summary.fileCount = 2;
    set.changes = [
      { ...set.changes[0]!, logicalPath: 'e:/repo/src/deep/new.ts' },
      { ...set.changes[1]!, logicalPath: 'C:/Temp/outside.rs' },
    ];
    getTurnFileChangeSetMock.mockResolvedValue(set);
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    try {
      const rows = [...container.querySelectorAll<HTMLElement>('[role="listitem"]')];
      expect(rows[0]!.querySelector('[data-file-name]')?.textContent).toBe('new.ts');
      expect(rows[0]!.querySelector('[data-file-directory]')?.textContent).toBe('src/deep');
      expect(rows[1]!.querySelector('[data-file-name]')?.textContent).toBe('outside.rs');
      expect(rows[1]!.querySelector('[data-file-directory]')?.textContent).toBe('C:/Temp');
    } finally { await act(async () => root.unmount()); }
  });

  it('does not present incomplete net statistics as a complete total', async () => {
    const partial = changeSet();
    partial.status = 'partial';
    partial.limitationCodes = ['turn-files.capture-limit-exceeded'];
    partial.changes[1]!.addedLines = null;
    partial.changes[1]!.deletedLines = null;
    partial.changes[1]!.limitationCode = 'turn-files.capture-limit-exceeded';
    getTurnFileChangeSetMock.mockResolvedValue(partial);
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    try {
      const header = container.querySelector('[data-slot="card-header"]');
      expect(header?.textContent).not.toContain('+5');
      expect(header?.textContent).not.toContain('-2');
      expect(container.querySelectorAll('[role="listitem"]')[1]?.textContent).not.toContain('+0');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not render a nested-agent change set on the parent conversation', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const rootLocator: TurnFileLocatorVm = { ...locator, branchId: 'root' };
    const event = pointerEvent('turn-files-agent');
    event.raw = {
      changeSetId: 'turn-files-agent',
      summary: { fileCount: 1, addedFiles: 1, modifiedFiles: 0, deletedFiles: 0, addedLines: 22, deletedLines: 0 },
      attachmentCount: 0,
      _meta: {
        conversation: { branchId: 'agent-04fcd2a222955ab5ac81c4683a403f28' },
        goldBandConversation: { branchId: 'root' },
      },
    };
    const root = createRoot(container);
    await act(async () => {
      root.render(
        <RightWorkspaceProvider>
          <TooltipProvider>
            <TurnFileChangesCard event={event} locator={rootLocator} />
          </TooltipProvider>
        </RightWorkspaceProvider>,
      );
    });
    await act(async () => { await Promise.resolve(); });
    try {
      expect(container.querySelector('[data-turn-file-changes-card]')).toBeNull();
      expect(container.textContent).not.toContain('无法加载本轮文件变化');
      expect(getTurnFileChangeSetMock).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('renders new attachments once, defaults to one row, and opens the attachment tab immediately', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const set = changeSet('attachments');
    set.summary = {
      fileCount: 1,
      addedFiles: 0,
      modifiedFiles: 1,
      deletedFiles: 0,
      addedLines: 1,
      deletedLines: 1,
    };
    set.changes = [{
      id: 'existing-attachment-modified',
      changeKind: 'modified',
      logicalPath: 'C:/attempt/attachments/existing.md',
      text: true,
      addedLines: 1,
      deletedLines: 1,
    }];
    set.attachments = [
      { id: 'attachment-report', relativePath: 'report.md', name: 'report.md', byteLength: 2048 },
      { id: 'attachment-summary', relativePath: 'summary.txt', name: 'summary.txt', byteLength: 24 },
    ];
    getTurnFileChangeSetMock.mockResolvedValue(set);
    const event = pointerEvent(set.id);
    event.raw = { changeSetId: set.id, summary: set.summary, attachmentCount: 2 };
    const root = await renderCard(container, event);
    try {
      const attachmentCard = container.querySelector<HTMLElement>('[data-turn-attachments-card]');
      const changeCard = container.querySelector<HTMLElement>('[data-turn-file-changes-card]');
      expect(attachmentCard).not.toBeNull();
      expect(changeCard).not.toBeNull();
      expect(attachmentCard?.textContent).toContain('report.md');
      expect(attachmentCard?.textContent).not.toContain('summary.txt');
      expect(changeCard?.textContent).toContain('existing.md');
      expect(changeCard?.textContent).not.toContain('report.md');
      expect(getTurnFileChangeSetMock).toHaveBeenCalledTimes(1);

      const firstAttachment = attachmentCard?.querySelector<HTMLButtonElement>('[role="listitem"]');
      await act(async () => firstAttachment?.click());
      const probe = container.querySelector('output');
      expect(probe?.dataset.activeKind).toBe('turn-attachment');
      expect(probe?.dataset.activeKey).toContain('attachments:attachment-report');
      expect(probe?.dataset.tabCount).toBe('1');

      const expand = attachmentCard?.querySelector<HTMLButtonElement>('button[aria-expanded="false"]');
      await act(async () => expand?.click());
      expect(attachmentCard?.textContent).toContain('summary.txt');
      expect(attachmentCard?.querySelectorAll('[role="listitem"]')).toHaveLength(2);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('updates one compaction row in place and stops its clock on completion', async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-09-09T06:16:20Z'));
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const first: AcpUiEventVm = {
      id: 'context-compaction-10', seq: 10, kind: 'contextCompaction',
      timestamp: '2026-09-09T06:16:20Z', startedAt: '2026-09-09T06:16:20Z',
      status: 'running',
    };
    const render = async (event: AcpUiEventVm) => {
      await act(async () => root.render(
        <TooltipProvider><ACPMessageList timeline={[event]} sessionStatus="running" sending={false} /></TooltipProvider>,
      ));
    };
    try {
      await render(first);
      const row = container.querySelector('[role="status"]');
      expect(row).not.toBeNull();
      for (const seq of [11, 12, 13]) {
        await act(async () => vi.advanceTimersByTime(30_000));
        await render({ ...first, seq });
        expect(container.querySelectorAll('[role="status"]')).toHaveLength(1);
        expect(container.querySelector('[role="status"]')).toBe(row);
      }
      await act(async () => vi.advanceTimersByTime(17_000));
      await render({ ...first, seq: 14, status: 'completed', endedAt: '2026-09-09T06:18:07Z' });
      const completedText = row?.textContent;
      expect(completedText).toContain('1m 47s');
      expect(row?.querySelector('.animate-spin')).toBeNull();
      await act(async () => vi.advanceTimersByTime(120_000));
      expect(row?.textContent).toBe(completedText);
      expect(container.querySelector('[role="status"]')).toBe(row);
    } finally {
      await act(async () => root.unmount());
      container.remove();
      vi.useRealTimers();
    }
  });

  it('keeps future ACP compaction statuses opaque instead of showing success', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(
        <TooltipProvider><ACPMessageList timeline={[{
          id: 'context-compaction-id-future', seq: 1, kind: 'contextCompaction',
          timestamp: '2026-10-08T00:00:00Z', status: '_provider_waiting',
        }]} sessionStatus="running" sending={false} /></TooltipProvider>,
      ));
      const row = container.querySelector('[role="status"]');
      expect(row?.textContent).not.toContain('✓');
      expect(row?.textContent).not.toContain('上下文压缩完成');
      expect(row?.querySelector('.animate-spin')).toBeNull();
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });

  it('uses the shared assistant content rail for compaction and file-change timeline items', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const compactionEvent: AcpUiEventVm = {
      id: 'compaction-1',
      seq: 3,
      timestamp: '2026-08-04T01:00:00Z',
      startedAt: '2026-08-04T01:00:00Z',
      endedAt: '2026-08-04T01:00:01Z',
      kind: 'contextCompaction',
      status: 'completed',
    };
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <TooltipProvider>
              <ACPMessageList
                timeline={[compactionEvent, pointerEvent()]}
                sessionStatus="completed"
                sending={false}
                branchLocator={locator}
              />
            </TooltipProvider>
          </RightWorkspaceProvider>,
        );
      });

      const compaction = container.querySelector<HTMLElement>('[role="status"]');
      const fileCard = container.querySelector<HTMLElement>('[data-turn-file-changes-card]');
      expect(compaction?.parentElement?.className).toContain('max-w-[82%]');
      expect(fileCard?.parentElement?.className).toContain('max-w-[82%]');
      expect(fileCard?.getAttribute('data-theme-role')).toBe('card');
      expect(fileCard?.className).toContain('mb-3');
      expect(compaction?.className).not.toContain('pl-10');
      expect(fileCard?.className).not.toContain('ml-10');
      expect(fileCard?.className).not.toContain('calc(100%');
      expect(fileCard?.className).not.toContain('bg-muted/10');
      expect(fileCard?.className).not.toContain('border-border/60');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('renders a prefetched change set on the first committed frame without a loading row', async () => {
    const prefetched = changeSet('prefetched');
    getTurnFileChangeSetMock.mockResolvedValue(prefetched);
    await loadTurnFileChangeSet(locator, prefetched.id);
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container, pointerEvent(prefetched.id));
    try {
      expect(container.textContent).not.toContain('正在加载文件变化');
      expect(container.querySelectorAll('[role="listitem"]')).toHaveLength(3);
      expect(container.querySelectorAll('[data-slot="hover-card-trigger"]')).toHaveLength(3);
      expect(container.querySelector('[role="list"] [data-slot="tooltip-trigger"]')).toBeNull();
      expect(container.querySelector('[title]')).toBeNull();
      expect(getTurnFileChangeSetMock).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps added and modified click navigation while deleted files expose preview focus without becoming clickable', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const limitedChangeSet = changeSet();
    limitedChangeSet.changes[1]!.limitationCode = 'turn-files.capture-limit-exceeded';
    getTurnFileChangeSetMock.mockResolvedValue(limitedChangeSet);
    const root = await renderCard(container);
    try {
      const rows = Array.from(container.querySelectorAll<HTMLElement>('[role="listitem"]'));
      expect(rows).toHaveLength(3);
      expect(rows[0]?.tagName).toBe('BUTTON');
      expect(rows[1]?.tagName).toBe('BUTTON');
      expect(rows[2]?.tagName).toBe('DIV');
      expect(rows[2]?.getAttribute('tabindex')).toBe('0');
      expect(rows[2]?.getAttribute('aria-label')).toContain('预览已删除文件');

      await act(async () => rows[0]?.click());
      const probe = container.querySelector('output');
      expect(probe?.dataset.activeKind).toBe('file-version');
      expect(probe?.dataset.activeKey).toContain('change-set-1:added-1');
      expect(probe?.dataset.activeBranch).toBe('agent-1');

      await act(async () => rows[1]?.click());
      expect(probe?.dataset.activeKind).toBe('file-diff');
      expect(probe?.dataset.activeKey).toContain('change-set-1:modified-1');
      expect(probe?.dataset.tabCount).toBe('2');
      expect(probe?.dataset.activeAttention).toBe('false');
      expect(rows[1]?.querySelector('svg')?.className.baseVal).toContain('text-gold-running');
      await act(async () => rows[2]?.click());
      expect(probe?.dataset.tabCount).toBe('2');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not render a card when the pointer summary contains no file changes', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const event = pointerEvent();
    event.raw = {
      changeSetId: 'empty',
      summary: { fileCount: 0, addedFiles: 0, modifiedFiles: 0, deletedFiles: 0, addedLines: 0, deletedLines: 0 },
    };
    getTurnFileChangeSetMock.mockResolvedValue({ ...changeSet('empty'), changes: [], summary: event.raw.summary });
    const root = await renderCard(container, event);
    try {
      expect(container.querySelector('[data-turn-file-changes-card]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('scrolls the complete file list after expansion instead of only the additional rows', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const longSet = changeSet('long-set');
    longSet.changes = Array.from({ length: 15 }, (_, index) => ({
      id: `added-${index + 1}`,
      changeKind: 'added' as const,
      logicalPath: `docs/${index + 1}.txt`,
      text: true,
      addedLines: 1,
      deletedLines: 0,
    }));
    longSet.summary = {
      fileCount: 15,
      addedFiles: 15,
      modifiedFiles: 0,
      deletedFiles: 0,
      addedLines: 15,
      deletedLines: 0,
    };
    getTurnFileChangeSetMock.mockResolvedValue(longSet);
    const event = pointerEvent('long-set');
    event.raw = { changeSetId: longSet.id, summary: longSet.summary };
    const root = await renderCard(container, event);
    try {
      expect(container.querySelectorAll('[role="listitem"]')).toHaveLength(3);
      const trigger = container.querySelector<HTMLButtonElement>('button[aria-expanded="false"]');
      expect(trigger).not.toBeNull();
      const initialContent = container.querySelector<HTMLElement>('[data-slot="collapsible-content"]');
      expect(initialContent?.hidden).toBe(true);
      expect(initialContent?.className).not.toContain('animate-collapsible-up');
      await act(async () => trigger?.click());

      const viewport = container.querySelector('[data-radix-scroll-area-viewport]');
      expect(viewport).not.toBeNull();
      expect(viewport?.querySelectorAll('[role="listitem"]')).toHaveLength(15);
      expect(container.querySelectorAll('[role="list"]')).toHaveLength(1);
      expect(container.querySelector('[data-slot="collapsible-content"]')?.className).toContain('animate-collapsible-down');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('uses a deliberate hover delay and a bounded cache that deduplicates comparison reads', async () => {
    expect(TURN_FILE_HOVER_OPEN_DELAY_MS).toBeGreaterThanOrEqual(300);
    expect(TURN_FILE_HOVER_CLOSE_DELAY_MS).toBeGreaterThan(0);
    expect(TURN_FILE_COMPARISON_CACHE_LIMIT).toBe(2);

    let resolveComparison!: (value: ReturnType<typeof comparison>) => void;
    getFileComparisonMock.mockImplementationOnce(() => new Promise((resolvePromise) => {
      resolveComparison = resolvePromise;
    }));
    const first = loadTurnFileComparison(locator, 'change-set-1', 'modified-1');
    const second = loadTurnFileComparison(locator, 'change-set-1', 'modified-1');
    expect(first).toBe(second);
    expect(getFileComparisonMock).toHaveBeenCalledTimes(1);

    resolveComparison(comparison());
    await expect(first).resolves.toMatchObject({ changeId: 'modified-1' });
    await expect(loadTurnFileComparison(locator, 'change-set-1', 'modified-1')).resolves.toMatchObject({ changeId: 'modified-1' });
    expect(getFileComparisonMock).toHaveBeenCalledTimes(1);

    await loadTurnFileComparison({ ...locator, branchId: 'agent-2' }, 'change-set-1', 'modified-1');
    expect(getFileComparisonMock).toHaveBeenCalledTimes(2);
  });

  it('keeps the hover diff preview compact while retaining viewport bounds', () => {
    expect(turnFileDiffPreviewSource).toContain('h-[clamp(10rem,44vh,24rem)]');
    expect(turnFileDiffPreviewSource).toContain('w-[min(40rem,calc(100vw-2rem))]');
    expect(turnFileDiffPreviewSource).toContain('h-9 shrink-0');
    expect(turnFileDiffPreviewSource).not.toContain('h-[clamp(12rem,52vh,30rem)]');
    expect(turnFileDiffPreviewSource).not.toContain('w-[min(46rem,calc(100vw-2rem))]');
  });

  it('opens the same lazy diff preview from keyboard focus without preloading other rows', async () => {
    getFileComparisonMock.mockImplementation(() => new Promise(() => undefined));
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    try {
      const rows = Array.from(container.querySelectorAll<HTMLElement>('[role="listitem"]'));
      expect(getFileComparisonMock).not.toHaveBeenCalled();
      await act(async () => rows[1]?.focus());
      await act(async () => { await Promise.resolve(); });

      const preview = document.body.querySelector<HTMLElement>('[data-turn-file-diff-preview="modified-1"]');
      expect(preview).not.toBeNull();
      expect(preview?.textContent).toContain('正在加载文件差异');
      expect(getFileComparisonMock).toHaveBeenCalledTimes(1);
      expect(getFileComparisonMock).toHaveBeenCalledWith(locator, 'change-set-1', 'modified-1');
      await act(async () => rows[1]?.click());
      await act(async () => { await Promise.resolve(); });
      expect(document.body.querySelector('[data-turn-file-diff-preview="modified-1"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not reopen the hover preview when pointer down moves focus into the row', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    const pointerEvent = (type: 'pointerdown' | 'pointerup') => {
      const event = new MouseEvent(type, { bubbles: true, clientX: 80, clientY: 40 });
      Object.defineProperty(event, 'pointerType', { value: 'mouse' });
      return event;
    };
    try {
      const row = container.querySelectorAll<HTMLElement>('[role="listitem"]')[1];
      await act(async () => {
        row?.dispatchEvent(pointerEvent('pointerdown'));
        row?.focus();
        await Promise.resolve();
      });
      expect(document.body.querySelector('[data-turn-file-diff-preview="modified-1"]')).toBeNull();

      await act(async () => {
        row?.dispatchEvent(pointerEvent('pointerup'));
        row?.blur();
        row?.focus();
        await Promise.resolve();
      });
      expect(document.body.querySelector('[data-turn-file-diff-preview="modified-1"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps a clicked hover preview closed until the pointer leaves and enters the row again', async () => {
    vi.useFakeTimers();
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    const pointerEvent = (
      type: 'pointerover' | 'pointerout' | 'pointermove',
      x: number,
      y: number,
      relatedTarget: EventTarget | null = null,
      movementX = 0,
      movementY = 0,
    ) => {
      const event = new MouseEvent(type, { bubbles: true, clientX: x, clientY: y, relatedTarget });
      Object.defineProperty(event, 'pointerType', { value: 'mouse' });
      Object.defineProperty(event, 'movementX', { value: movementX });
      Object.defineProperty(event, 'movementY', { value: movementY });
      return event;
    };
    try {
      const row = container.querySelectorAll<HTMLElement>('[role="listitem"]')[1];
      expect(row).not.toBeNull();

      await act(async () => {
        row?.dispatchEvent(pointerEvent('pointerover', 80, 40));
        await vi.advanceTimersByTimeAsync(TURN_FILE_HOVER_OPEN_DELAY_MS);
      });
      expect(document.body.querySelector('[data-turn-file-diff-preview="modified-1"]')).not.toBeNull();

      await act(async () => {
        row?.dispatchEvent(new MouseEvent('click', {
          bubbles: true,
          clientX: 80,
          clientY: 40,
          detail: 1,
        }));
        row?.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: document.body }));
        row?.dispatchEvent(pointerEvent('pointerout', 80, 40, document.body));
        row?.dispatchEvent(pointerEvent('pointerover', 120, 60, document.body));
        row?.dispatchEvent(pointerEvent('pointermove', 80, 40, document.body, 40, 20));
        await vi.advanceTimersByTimeAsync(TURN_FILE_HOVER_OPEN_DELAY_MS + TURN_FILE_HOVER_CLOSE_DELAY_MS);
      });
      expect(document.body.querySelector('[data-turn-file-diff-preview="modified-1"]')).toBeNull();

      await act(async () => {
        row?.dispatchEvent(pointerEvent('pointerout', 140, 70, document.body));
        row?.dispatchEvent(pointerEvent('pointerover', 150, 80, document.body));
        row?.dispatchEvent(pointerEvent('pointermove', 150, 80, document.body, 10, 10));
        await vi.advanceTimersByTimeAsync(TURN_FILE_HOVER_OPEN_DELAY_MS);
      });
      expect(document.body.querySelector('[data-turn-file-diff-preview="modified-1"]')).not.toBeNull();
    } finally {
      await act(async () => root.unmount());
      vi.useRealTimers();
    }
  });

  it('emits copyable opt-in hover diagnostics without file paths', async () => {
    window.localStorage.setItem(TURN_FILE_HOVER_DEBUG_STORAGE_KEY, '1');
    const info = vi.spyOn(console, 'info').mockImplementation(() => undefined);
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    try {
      const row = container.querySelectorAll<HTMLElement>('[role="listitem"]')[1];
      await act(async () => row?.focus());
      await act(async () => row?.click());
      await act(async () => { await Promise.resolve(); });
      await act(async () => root.unmount());

      const lines = info.mock.calls.map(([line]) => String(line));
      expect(lines.some((line) => line.includes('[GoldBand][Turn file hover]'))).toBe(true);
      expect(lines.some((line) => line.includes('"event":"row-mount"'))).toBe(true);
      expect(lines.some((line) => line.includes('"event":"open-request"'))).toBe(true);
      expect(lines.some((line) => line.includes('"event":"click"'))).toBe(true);
      expect(lines.some((line) => line.includes('"event":"row-unmount"'))).toBe(true);
      expect(lines.join('\n')).not.toContain('src/existing.ts');
    } finally {
      info.mockRestore();
    }
  });
});

describe('conversation artifact workspace entry', () => {
  it('keeps the artifact in its message card and opens it in the right workspace', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const artifactEvent: AcpUiEventVm = {
      id: 'artifact-message',
      seq: 8,
      timestamp: '2026-08-04T01:00:02Z',
      kind: 'textDelta',
      content: 'Completed\n```json\n{}\n```',
      status: 'completed',
      raw: {
        runtimeControlOutputDisplay: {
          artifactName: 'result.md',
          kind: 'workflow-completion',
          jsonText: '{}',
          start: 10,
          end: 25,
          parseStatus: 'valid',
        },
      },
    };
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <TooltipProvider>
              <ACPMessageList timeline={[artifactEvent]} sessionStatus="completed" sending={false} branchLocator={locator} />
              <WorkspaceProbe />
            </TooltipProvider>
          </RightWorkspaceProvider>,
        );
      });
      const artifactButton = Array.from(container.querySelectorAll<HTMLButtonElement>('button'))
        .find((button) => button.textContent?.includes('result.md')) ?? null;
      expect(artifactButton).not.toBeNull();
      expect(artifactButton?.dataset.slot).toBe('tooltip-trigger');
      expect(artifactButton?.hasAttribute('title')).toBe(false);
      await act(async () => artifactButton?.click());
      const probe = container.querySelector('output');
      expect(probe?.dataset.activeKind).toBe('conversation-asset');
      expect(probe?.dataset.activeBranch).toBe('agent-1');
    } finally {
      await act(async () => root.unmount());
    }
  });
});

describe('turn file viewer contract', () => {
  it('shows change navigation only when the unified diff has multiple chunks', () => {
    expect(shouldShowDiffChunkNavigation(0)).toBe(false);
    expect(shouldShowDiffChunkNavigation(1)).toBe(false);
    expect(shouldShowDiffChunkNavigation(2)).toBe(true);
  });

  it('uses a read-only unified CodeMirror merge view', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'web/src/components/workspace/files/ReadonlyUnifiedDiff.tsx'),
      'utf8',
    );
    expect(source).toContain('unifiedMergeView({');
    expect(source).toContain('EditorState.readOnly.of(true)');
    expect(source).toContain('EditorView.editable.of(false)');
    expect(source).toContain('EditorView.lineWrapping');
    expect(source).toContain('drawSelection: false');
    expect(source).toContain('width="100%"');
    expect(source).toContain('[&_.cm-scroller]:overflow-x-hidden');
    expect(source).toContain('mergeControls: false');
    expect(source).toContain('highlightChanges: true');
    expect(source).toContain('collapseUnchanged:');
    expect(source).toContain('diffConfig: { scanLimit: DIFF_VIEW_SCAN_LIMIT, timeout: DIFF_VIEW_TIMEOUT_MS }');
    const workspaceSource = readFileSync(
      resolve(process.cwd(), 'web/src/components/workspace/files/TurnFileWorkspacePanel.tsx'),
      'utf8',
    );
    expect(workspaceSource).toContain('getChunks(view.state)?.chunks.length');
    expect(workspaceSource).toContain('<ReadonlyUnifiedDiff');
    expect(workspaceSource).toContain('const range = EditorSelection.range(to, from)');
    expect(workspaceSource).toContain('EditorView.scrollIntoView(range)');
    expect(workspaceSource).toContain('goToNextChunk');
    expect(workspaceSource).toContain('goToPreviousChunk');
  });

  it('keeps large source diffs precise within a bounded main-thread budget', () => {
    expect(DIFF_VIEW_SCAN_LIMIT).toBeGreaterThanOrEqual(5_000);
    expect(DIFF_VIEW_TIMEOUT_MS).toBeGreaterThan(0);
    expect(DIFF_VIEW_TIMEOUT_MS).toBeLessThanOrEqual(300);
  });

  it('opens review files at the top and only focuses chunks for cross-file change navigation', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'web/src/components/workspace/files/TurnFileWorkspacePanel.tsx'),
      'utf8',
    );
    expect(source).toContain("navigateReviewFile(-1, 'top')");
    expect(source).toContain("navigateReviewFile(1, 'top')");
    expect(source).toContain("resource.reviewLanding === 'first-change' || resource.reviewLanding === 'last-change'");
    expect(source).not.toContain("reviewLanding === 'last'");
  });

  it('does not enable a closing animation when an asynchronously loaded card first mounts collapsed', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'web/src/components/acp/TurnFileChangesCard.tsx'),
      'utf8',
    );
    expect(source).toContain("hasUserToggled && 'data-[state=closed]:animate-collapsible-up");
    expect(source).not.toContain('<CollapsibleContent className="data-[state=closed]:animate-collapsible-up');
  });

  it('uses the shared read-only Markdown viewer for fully added Markdown files', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'web/src/components/workspace/files/TurnFileWorkspacePanel.tsx'),
      'utf8',
    );
    expect(source).toContain("resource.kind === 'file-version'");
    expect(source).toContain('isMarkdownDocumentPath(');
    expect(source).toContain('<WorkspaceFileEditor');
    expect(source).toContain('editable={false}');
  });
});

describe('file change set owner branch', () => {
  it('prefers the finalize owner over a leaked parent-timeline goldBandConversation branch', () => {
    expect(fileChangeSetOwnerBranch({
      changeSetId: 'turn-files-agent',
      _meta: {
        conversation: { branchId: 'agent-04fcd2a222955ab5ac81c4683a403f28' },
        goldBandConversation: { branchId: 'root' },
      },
    })).toBe('agent-04fcd2a222955ab5ac81c4683a403f28');
  });

  it('reads canonical goldBandConversation ownership after persist routing is corrected', () => {
    expect(fileChangeSetOwnerBranch({
      changeSetId: 'turn-files-agent',
      _meta: {
        goldBandConversation: { branchId: 'agent-04fcd2a222955ab5ac81c4683a403f28' },
      },
    })).toBe('agent-04fcd2a222955ab5ac81c4683a403f28');
  });
});
