/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import '@/i18n';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { FileBrowserWorkspaceResource, FileWorkspaceResource } from '@/components/workspace/right-workspace-context';
import { createUserPromptSubmission } from '@/lib/composer-context';

const mocks = vi.hoisted(() => ({ openResource: vi.fn(), reference: vi.fn() }));
vi.mock('@/api', async (original) => ({
  ...await original<typeof import('@/api')>(),
  readFileResource: vi.fn(),
  startWorkspaceFileWatch: vi.fn().mockResolvedValue(undefined),
  stopWorkspaceFileWatch: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('@/components/workspace/right-workspace-context', async (original) => ({
  ...await original<typeof import('@/components/workspace/right-workspace-context')>(),
  useRightWorkspace: () => ({ openResource: mocks.openResource }),
  useRightWorkspaceCommands: () => ({ scopeKey: 'scope', openResource: mocks.openResource }),
}));
vi.mock('@/components/workspace/workspace-file-reference-bridge', async (original) => ({
  ...await original<typeof import('@/components/workspace/workspace-file-reference-bridge')>(),
  useWorkspaceFileReferenceCommands: () => ({ available: true, addWorkspaceFileRef: mocks.reference }),
  useComposerQuoteCommand: () => null,
}));
vi.mock('@/components/workspace/use-workspace-responsive-state', () => ({
  useWorkspaceResponsiveState: () => ({ ref: () => {}, responsiveState: { split: true, widthAtTransition: 900 }, currentWidth: () => 900 }),
}));
vi.mock('@/components/ui/resizable', () => ({
  ResizablePanelGroup: ({ children }: React.PropsWithChildren) => <div>{children}</div>,
  ResizablePanel: ({ children }: React.PropsWithChildren) => <div>{children}</div>,
  ResizableHandle: () => null,
}));
vi.mock('@/components/workspace/files/WorkspaceFileEditor', () => ({
  WorkspaceFileEditor: ({ value }: { value: string }) => <pre>{value}</pre>,
}));
vi.mock('@/components/workspace/files/WorkspaceFileTree', () => ({ WorkspaceFileTree: () => <div>Receiving directory</div> }));

import { readFileResource, startWorkspaceFileWatch } from '@/api';
import { FileWorkspacePanel } from '@/components/workspace/files/FileWorkspacePanel';
import { fileContentStore } from '@/components/workspace/files/file-content-store';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const selectedFile = {
  kind: 'file' as const, key: 'file:p:D:/source/a.ts', scopeKey: 'scope', projectId: 'p', workspacePath: 'D:/source',
  source: { kind: 'git-file' as const, projectId: 'p', workspacePath: 'D:/source', relativePath: 'a.ts' },
  title: 'a.ts', attention: false, target: null, targetRevision: 0,
  locator: { projectId: 'p', canonicalPath: 'D:/source/a.ts', relativePath: 'a.ts', scope: 'workspace' as const },
};
const resource: FileBrowserWorkspaceResource = {
  kind: 'file-browser', key: 'file-browser:p', scopeKey: 'scope', projectId: 'p', title: 'Files', attention: false,
  root: { kind: 'unavailable', reason: 'released', workspacePath: 'D:/receiver' }, browseMain: false, selectedFile,
};
const layout = { splitMinWidth: 700, treeMinWidth: 200, treeMaxWidth: 400, treeDefaultWidth: 260 };
afterEach(async () => {
  await fileContentStore.release(selectedFile.key);
  vi.clearAllMocks();
  document.body.replaceChildren();
});

it('renders the fixed source file beside the unavailable receiving directory without browsing main', async () => {
  vi.mocked(readFileResource).mockResolvedValue({
    kind: 'text', locator: selectedFile.locator, name: 'a.ts', content: 'export const source = "original";',
    editable: true, language: 'typescript', revision: { contentHash: 'source', byteLength: 40, modifiedAtNs: '1' },
    externalAccessGrant: null,
  });
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<TooltipProvider><FileWorkspacePanel resource={resource} layout={layout} /></TooltipProvider>));
    expect(container.querySelector('article[aria-label="a.ts"]')?.textContent).toContain('export const source = "original";');
    expect(container.textContent).toContain('浏览主工作区');
    expect(readFileResource).toHaveBeenCalledWith(expect.objectContaining({ workspacePath: 'D:/source' }), 'D:/source/a.ts', undefined, false);
    expect(startWorkspaceFileWatch).toHaveBeenCalledWith({ projectId: 'p', workspacePath: 'D:/source' });
    expect(mocks.openResource).not.toHaveBeenCalled();
    const browse = [...container.querySelectorAll('button')].find((button) => button.textContent?.includes('浏览主工作区'))!;
    await act(async () => browse.click());
    expect(mocks.openResource).toHaveBeenCalledWith(expect.objectContaining({ browseMain: true, selectedFile }));
  } finally { await act(async () => root.unmount()); }
});

const fixedSources: NonNullable<FileWorkspaceResource['source']>[] = [
  selectedFile.source,
  { kind: 'turn-file', locator: { projectId: 'p', taskId: 'task-source', taskUuid: 'uuid-source',
    runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'main',
    outerNodeId: 'outer-node', outerAttemptId: 'outer-attempt' }, changeSetId: 'set', changeId: 'change' },
  { kind: 'turn-attachment', locator: { projectId: 'p', taskId: 'task-source', taskUuid: 'uuid-source',
    runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'main',
    outerNodeId: 'outer-node', outerAttemptId: 'outer-attempt' }, changeSetId: 'set', attachmentId: 'attachment' },
];

it.each(fixedSources)('preserves the $kind source when referencing the preview header and submitting', async (source) => {
  const file: FileWorkspaceResource = source.kind === 'turn-attachment'
    ? { ...selectedFile, source, title: 'report.md', locator: { projectId: 'p',
      canonicalPath: 'D:/runtime/attempt/attachments/report.md', relativePath: null, scope: 'external' } }
    : { ...selectedFile, source };
  vi.mocked(readFileResource).mockResolvedValue({
    kind: 'text', locator: file.locator, name: file.title, content: 'original source',
    editable: true, language: 'typescript', revision: { contentHash: 'source', byteLength: 15, modifiedAtNs: '1' },
    externalAccessGrant: null,
  });
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<TooltipProvider><FileWorkspacePanel
      resource={{ ...resource, selectedFile: file }} layout={layout}
    /></TooltipProvider>));
    const referenceButton = container.querySelector<HTMLButtonElement>('[data-file-reference-to-conversation]');
    expect(referenceButton).not.toBeNull();
    await act(async () => referenceButton!.click());
    expect(mocks.reference).toHaveBeenCalledWith(expect.objectContaining({ source }), { isDocked: false });
    const reference = mocks.reference.mock.calls[0][0];
    expect(createUserPromptSubmission('', [], null, [reference]).workspaceFiles).toEqual([source]);
  } finally { await act(async () => root.unmount()); }
});
