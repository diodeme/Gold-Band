/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RightWorkspaceResource } from '@/components/workspace/right-workspace-context';
import type { RightWorkspaceFileTargetVm } from '@/types';

const mocks = vi.hoisted(() => ({ resolve: vi.fn(), reveal: vi.fn(), add: vi.fn(), get: vi.fn(), capture: vi.fn(), available: true }));
vi.mock('@/api', () => ({ resolveRightWorkspaceFileTarget: mocks.resolve, revealRightWorkspaceFile: mocks.reveal }));
vi.mock('@/components/workspace/right-workspace-context', () => ({ useRightWorkspaceCommands: () => ({ getResource: mocks.get }) }));
vi.mock('@/components/workspace/workspace-file-reference-bridge', () => ({
  useWorkspaceFileReferenceCommands: () => ({ available: mocks.available, captureFileReferenceTarget: mocks.capture }),
  useWorkspaceFileReferencePresentation: () => ({ isDocked: true }),
}));
import { RightWorkspaceFileMenu } from '@/components/workspace/RightWorkspaceFileMenu';
import { rightWorkspaceFileSource } from '@/lib/file-reference-source';
import '@/i18n';
import { TooltipProvider } from '@/components/ui/tooltip';

const artifact: RightWorkspaceResource = { kind: 'conversation-asset', key: 'artifact', scopeKey: 'scope',
  assetKind: 'artifact', name: 'review.json', title: 'review', attention: false,
  locator: { projectId: 'p', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'root' } };
const source = rightWorkspaceFileSource(artifact)!;
const target: RightWorkspaceFileTargetVm = { source, projectId: 'p', name: 'review.json', absolutePath: 'D:/runtime/artifacts/review.json',
  relativePath: 'artifacts/review.json', relativeBase: 'attempt', workspacePath: null, byteLength: 2, mimeType: 'application/json', availability: { kind: 'available' } };
let root: Root;
let container: HTMLDivElement;
const click = async (index: number) => { await act(async () => { document.querySelectorAll<HTMLElement>('[role="menuitem"]')[index].click(); }); };
async function render(tab = artifact) {
  const activate = vi.fn();
  await act(async () => { root.render(<TooltipProvider><RightWorkspaceFileMenu tab={tab}><div data-tab="true"><button onClick={activate}>Tab</button></div></RightWorkspaceFileMenu></TooltipProvider>); });
  return activate;
}
async function open() { await act(async () => { container.querySelector('[data-tab]')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, button: 2, clientX: 10, clientY: 10 })); }); }

describe('right workspace file actions', () => {
  beforeEach(() => {
    container = document.body.appendChild(document.createElement('div')); root = createRoot(container);
    vi.clearAllMocks(); mocks.resolve.mockResolvedValue(target); mocks.reveal.mockResolvedValue(undefined);
    mocks.available = true;
    mocks.get.mockReturnValue(artifact); mocks.add.mockReturnValue({ kind: 'added' }); mocks.capture.mockReturnValue(mocks.add);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: vi.fn().mockResolvedValue(undefined) } });
  });
  afterEach(() => { act(() => root.unmount()); container.remove(); });

  it('loads only after right click and copies the background tab path without activating it', async () => {
    const activate = await render(); expect(mocks.resolve).not.toHaveBeenCalled();
    const dom = container.querySelector('[data-tab]'); await open();
    expect(mocks.resolve).toHaveBeenCalledExactlyOnceWith(source);
    expect(document.querySelectorAll('[role="menuitem"]')).toHaveLength(4);
    await render(); expect(container.querySelector('[data-tab]')).toBe(dom);
    expect(document.querySelector('[data-right-workspace-file-menu]')).not.toBeNull();
    await click(1);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('artifacts/review.json');
    expect(activate).not.toHaveBeenCalled();
  });

  it('revalidates and adds structured current-file context rather than a Diff quote', async () => {
    await render(); await open(); await click(3);
    expect(mocks.resolve).toHaveBeenCalledTimes(2);
    expect(mocks.add).toHaveBeenCalledWith(expect.objectContaining({ source, name: 'review.json' }), { isDocked: true });
  });

  it('keeps composer focus when validation finishes before menu close autofocus', async () => {
    const input = document.body.appendChild(document.createElement('textarea'));
    mocks.add.mockImplementationOnce(() => { input.focus(); return { kind: 'added' }; });
    try {
      await render(); await open(); await click(3);
      expect(document.activeElement).toBe(input);
    } finally { input.remove(); }
  });

  it('reveals the exact controlled source', async () => {
    await render(); await open(); await click(2);
    expect(mocks.reveal).toHaveBeenCalledExactlyOnceWith(source);
  });

  it('keeps record paths copyable when the file is unavailable', async () => {
    mocks.resolve.mockResolvedValue({ ...target, availability: { kind: 'unavailable', code: 'conversation.workspace-file-not-found', params: {} } });
    await render(); await open();
    const items = document.querySelectorAll('[role="menuitem"]');
    expect(items[0].hasAttribute('data-disabled')).toBe(false);
    expect(items[1].hasAttribute('data-disabled')).toBe(false);
    expect(items[2].hasAttribute('data-disabled')).toBe(true);
    expect(items[3].hasAttribute('data-disabled')).toBe(true);
  });

  it('disables reference without a receiving composer while retaining copy and reveal', async () => {
    mocks.available = false;
    await render(); await open();
    const items = document.querySelectorAll('[role="menuitem"]');
    expect(items[2].hasAttribute('data-disabled')).toBe(false);
    expect(items[3].hasAttribute('data-disabled')).toBe(true);
  });

  it('shows a metadata error with all file operations disabled', async () => {
    mocks.resolve.mockRejectedValue({ code: 'conversation.workspace-file-not-found', params: {} });
    await render(); await open();
    expect(document.querySelector('[role="status"]')?.textContent).toBeTruthy();
    for (const item of document.querySelectorAll('[role="menuitem"]')) expect(item.hasAttribute('data-disabled')).toBe(true);
  });

  it('reports an opener failure in a floating alert after the menu closes', async () => {
    mocks.reveal.mockRejectedValue({ code: 'workspace-file.file-manager-open-failed', params: {} });
    await render(); await open(); await click(2);
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(document.querySelector('[role="alert"]')?.textContent).toBeTruthy();
  });

  it('discards late metadata after closing and reopening the menu', async () => {
    let finish!: (target: RightWorkspaceFileTargetVm) => void;
    mocks.resolve.mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
    await render(); await open();
    expect(document.querySelector('[role="status"]')?.textContent).toBeTruthy();
    await act(async () => { document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); });
    await open();
    await act(async () => finish({ ...target, absolutePath: 'D:/wrong-file' }));
    await click(0);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(target.absolutePath);
  });

  it('does not add a file if the tab changes source during reference validation', async () => {
    await render(); await open();
    let finish!: (target: RightWorkspaceFileTargetVm) => void;
    mocks.resolve.mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
    await click(3); mocks.get.mockReturnValue(null);
    await act(async () => finish(target));
    expect(mocks.add).not.toHaveBeenCalled();
  });

  it('discards validation when a Git review keeps its tab key but switches file', async () => {
    const git = { kind: 'file-diff' as const, key: 'review', scopeKey: 'scope', title: 'git', attention: false,
      projectId: 'p', gitSource: { kind: 'workspace' as const, workspacePath: 'D:/worktree', path: 'src/a.ts', area: 'unstaged' as const } };
    mocks.get.mockReturnValue(git);
    await render(git); await open();
    let finish!: (target: RightWorkspaceFileTargetVm) => void;
    mocks.resolve.mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
    await click(3);
    mocks.get.mockReturnValue({ ...git, gitSource: { ...git.gitSource, path: 'src/b.ts' } });
    await act(async () => finish(target));
    expect(mocks.add).not.toHaveBeenCalled();
  });

  it('shows all four file actions for a workflow-generated attachment tab', async () => {
    const locator = artifact.locator;
    const attachment = {
      kind: 'turn-attachment' as const,
      key: 'turn-attachment:set-1:attachment-1',
      scopeKey: 'scope',
      title: 'report.md',
      description: 'report.md',
      attention: false,
      locator,
      changeSetId: 'set-1',
      attachmentId: 'attachment-1',
    };
    const attachmentSource = {
      kind: 'turn-attachment' as const,
      locator,
      changeSetId: 'set-1',
      attachmentId: 'attachment-1',
    };
    mocks.get.mockReturnValue(attachment);
    mocks.resolve.mockResolvedValue({ ...target, source: attachmentSource, name: 'report.md',
      absolutePath: 'D:/runtime/attempt/attachments/report.md', relativePath: 'attachments/report.md' });

    expect(rightWorkspaceFileSource(attachment)).toEqual(attachmentSource);
    await render(attachment);
    await open();
    expect(mocks.resolve).toHaveBeenCalledExactlyOnceWith(attachmentSource);
    expect(document.querySelectorAll('[role="menuitem"]')).toHaveLength(4);
  });

  it('maps both conversation changes and Git comparisons to their own current-file source', () => {
    const locator = artifact.locator;
    expect(rightWorkspaceFileSource({ kind: 'file-version', key: 'v', scopeKey: 'scope', title: 'version', attention: false,
      locator, changeSetId: 'set', changeId: 'change' })).toEqual({ kind: 'turn-file', locator, changeSetId: 'set', changeId: 'change' });
    expect(rightWorkspaceFileSource({ kind: 'file-diff', key: 'g', scopeKey: 'scope', title: 'git', attention: false,
      projectId: 'p', gitSource: { kind: 'workspace', workspacePath: 'D:/worktree', path: 'src/a.ts', area: 'unstaged' } }))
      .toEqual({ kind: 'git-file', projectId: 'p', workspacePath: 'D:/worktree', relativePath: 'src/a.ts' });
  });
});
