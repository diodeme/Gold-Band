/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { TurnFileChangesCard } from '@/components/acp/TurnFileChangesCard';
import { ACPMessageList } from '@/components/acp/ACPChatDialog';
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

const { getTurnFileChangeSetMock } = vi.hoisted(() => ({
  getTurnFileChangeSetMock: vi.fn(),
}));

vi.mock('@/api', () => ({
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

function WorkspaceProbe() {
  const workspace = useRightWorkspace();
  const active = workspace.tabs.find((tab) => tab.key === workspace.activeTabKey);
  return (
    <output
      data-active-kind={active?.kind ?? ''}
      data-active-key={active?.key ?? ''}
      data-active-branch={active && 'locator' in active && 'branchId' in active.locator ? active.locator.branchId : ''}
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
  getTurnFileChangeSetMock.mockReset();
  getTurnFileChangeSetMock.mockImplementation((_locator, id: string) => Promise.resolve(changeSet(id)));
});

afterEach(() => {
  document.body.replaceChildren();
});

describe('turn file changes card', () => {
  it('opens added and modified captures in the right workspace, while deleted files remain non-interactive', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = await renderCard(container);
    try {
      const rows = Array.from(container.querySelectorAll<HTMLElement>('[role="listitem"]'));
      expect(rows).toHaveLength(3);
      expect(rows[0]?.tagName).toBe('BUTTON');
      expect(rows[1]?.tagName).toBe('BUTTON');
      expect(rows[2]?.tagName).toBe('DIV');
      expect(rows[2]?.getAttribute('tabindex')).toBeNull();

      await act(async () => rows[0]?.click());
      const probe = container.querySelector('output');
      expect(probe?.dataset.activeKind).toBe('file-version');
      expect(probe?.dataset.activeKey).toContain('change-set-1:added-1');
      expect(probe?.dataset.activeBranch).toBe('agent-1');

      await act(async () => rows[1]?.click());
      expect(probe?.dataset.activeKind).toBe('file-diff');
      expect(probe?.dataset.activeKey).toContain('change-set-1:modified-1');
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
      const artifactButton = container.querySelector<HTMLButtonElement>('button[title="result.md"]');
      expect(artifactButton).not.toBeNull();
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
  it('uses a read-only unified CodeMirror merge view', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'web/src/components/workspace/files/TurnFileWorkspacePanel.tsx'),
      'utf8',
    );
    expect(source).toContain('unifiedMergeView({');
    expect(source).toContain('EditorState.readOnly.of(true)');
    expect(source).toContain('EditorView.editable.of(false)');
    expect(source).toContain('mergeControls: false');
    expect(source).toContain('collapseUnchanged:');
  });
});
