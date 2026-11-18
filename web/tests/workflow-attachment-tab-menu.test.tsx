/** @vitest-environment jsdom */

import React, { act, useEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { TurnFileChangesCard } from '@/components/acp/TurnFileChangesCard';
import { TooltipProvider } from '@/components/ui/tooltip';
import { RightWorkspaceDock } from '@/components/workspace/RightWorkspaceDock';
import { createConversationWorkspaceScope, RightWorkspaceProvider } from '@/components/workspace/right-workspace-context';
import { useWorkspaceFileReferenceBridge } from '@/components/workspace/workspace-file-reference-bridge';
import { clearTurnFileChangeSetCacheForTests } from '@/lib/turn-file-change-set-cache';
import type { AcpUiEventVm, PromptWorkspaceFileRef, RightWorkspaceFileTargetVm, TurnFileChangeSetVm } from '@/types';

const mocks = vi.hoisted(() => ({ changeSet: vi.fn(), resolve: vi.fn(), reveal: vi.fn(), reference: vi.fn() }));
vi.mock('@/api', () => ({
  getTurnFileChangeSet: mocks.changeSet,
  resolveRightWorkspaceFileTarget: mocks.resolve,
  revealRightWorkspaceFile: mocks.reveal,
}));
vi.mock('@/components/workspace/AgentConversationPanel', () => ({ AgentConversationPanel: () => null }));

const scope = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', taskUuid: 'task-uuid-1', runId: 'run-1' });
const source: Extract<PromptWorkspaceFileRef, { kind: 'turn-attachment' }> = {
  kind: 'turn-attachment',
  locator: { projectId: 'project-1', taskId: 'task-1', runId: 'run-1', roundId: 'round-1', nodeId: 'node-1', attemptId: 'attempt-1', branchId: 'root' },
  changeSetId: 'set-1',
  attachmentId: 'attachment-1',
};
const manifest: TurnFileChangeSetVm = {
  id: source.changeSetId, turnId: 'turn-1', promptEventId: 'prompt-1', branchId: 'root', status: 'finalized',
  startedAt: '2026-10-28T00:00:00Z', changes: [], limitationCodes: [],
  summary: { fileCount: 0, addedFiles: 0, modifiedFiles: 0, deletedFiles: 0, addedLines: 0, deletedLines: 0 },
  attachments: [{ id: source.attachmentId, name: 'report.md', relativePath: 'reports/report.md', byteLength: 128 }],
};
const event: AcpUiEventVm = { id: 'event-1', seq: 1, timestamp: manifest.startedAt, kind: 'fileChangeSet',
  status: 'finalized', raw: { changeSetId: source.changeSetId, attachmentCount: 1, summary: manifest.summary } };
const target: RightWorkspaceFileTargetVm = { source, projectId: 'project-1', name: 'report.md',
  absolutePath: 'D:/runtime/attempt/attachments/reports/report.md', relativePath: 'reports/report.md',
  relativeBase: 'attachment', workspacePath: null, byteLength: 128, mimeType: 'text/markdown', availability: { kind: 'available' } };

function ComposerTarget() {
  const bridge = useWorkspaceFileReferenceBridge();
  useEffect(() => bridge.register({ addWorkspaceFileRef: mocks.reference, addQuote: () => ({ kind: 'added' }) }), [bridge]);
  return null;
}

let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.clearAllMocks();
  clearTurnFileChangeSetCacheForTests();
  vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
  vi.stubGlobal('ResizeObserver', class { observe() {} disconnect() {} unobserve() {} });
  mocks.changeSet.mockResolvedValue(manifest);
  mocks.resolve.mockResolvedValue(target);
  mocks.reveal.mockResolvedValue(undefined);
  mocks.reference.mockReturnValue({ kind: 'added' });
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: vi.fn().mockResolvedValue(undefined) } });
  container = document.body.appendChild(document.createElement('div'));
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  clearTurnFileChangeSetCacheForTests();
  vi.unstubAllGlobals();
});

it('opens a delivery card attachment in the real dock and executes all four menu actions against its manifest identity', async () => {
  await act(async () => root.render(
    <TooltipProvider><RightWorkspaceProvider scope={scope} workLocation={{ kind: 'main' }}>
      <ComposerTarget />
      <TurnFileChangesCard event={event} locator={source.locator} />
      <RightWorkspaceDock />
    </RightWorkspaceProvider></TooltipProvider>,
  ));
  await act(async () => container.querySelector<HTMLButtonElement>('[data-turn-attachments-card] [role="listitem"]')!.click());
  const tab = container.querySelector<HTMLElement>('[data-right-workspace-tab]')!;
  expect(tab.dataset.rightWorkspaceResourceKey).toBe('turn-attachment:set-1:attachment-1');
  expect(mocks.resolve).not.toHaveBeenCalled();
  const openMenu = async () => act(async () => {
    tab.querySelector('button')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, button: 2, clientX: 10, clientY: 10 }));
  });
  const select = async (index: number) => act(async () => document.querySelectorAll<HTMLElement>('[role="menuitem"]')[index]!.click());

  await openMenu();
  expect([...document.querySelectorAll('[role="menuitem"]')].map(item => item.textContent)).toEqual([
    '复制绝对路径', '复制相对路径', '在文件管理器中打开', '引用到对话',
  ]);
  expect(mocks.resolve).toHaveBeenCalledExactlyOnceWith(source);
  await select(0);
  expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith(target.absolutePath);
  await openMenu();
  await select(1);
  expect(navigator.clipboard.writeText).toHaveBeenLastCalledWith('reports/report.md');
  await openMenu();
  await select(2);
  expect(mocks.reveal).toHaveBeenCalledExactlyOnceWith(source);
  await openMenu();
  await select(3);
  expect(mocks.reference).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ source, name: 'report.md', canonicalPath: target.absolutePath }), { isDocked: false });
  expect(container.querySelector('[data-right-workspace-tab]')).toBe(tab);
});
