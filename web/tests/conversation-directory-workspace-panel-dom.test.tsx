/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listConversationDirectory: vi.fn(),
  readConversationDirectoryFile: vi.fn(),
}));

vi.mock('@/api', () => ({
  listConversationDirectory: apiMocks.listConversationDirectory,
  openConversationDirectoryPathInFileManager: vi.fn(),
  readConversationDirectoryFile: apiMocks.readConversationDirectoryFile,
  workspaceFilePreviewUrl: vi.fn(() => ''),
}));

vi.mock('@/components/workspace/files/WorkspaceFileEditor', () => ({
  WorkspaceFileEditor: (props: {
    editable: boolean;
    markdownMode?: string | null;
    onMarkdownModeChange?: (mode: 'live-preview' | 'source') => void;
  }) => (
    <output
      data-testid="run-directory-file-editor"
      data-editable={String(props.editable)}
      data-markdown-mode={String(props.markdownMode)}
    >
      <button type="button" onClick={() => props.onMarkdownModeChange?.('source')}>source</button>
    </output>
  ),
}));

import { ConversationDirectoryWorkspacePanel } from '@/components/workspace/ConversationDirectoryWorkspacePanel';
import { TooltipProvider } from '@/components/ui/tooltip';
import { conversationDirectoryWorkspaceResourceKey, RightWorkspaceProvider, useRightWorkspace, type ConversationDirectoryWorkspaceResource } from '@/components/workspace/right-workspace-context';
import type { FileWorkspaceLayoutVm, WorkspaceDirectoryEntryVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class ControlledResizeObserver implements ResizeObserver {
  static instances: ControlledResizeObserver[] = [];

  readonly callback: ResizeObserverCallback;
  readonly targets = new Set<Element>();

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    ControlledResizeObserver.instances.push(this);
  }

  observe(target: Element) {
    this.targets.add(target);
  }

  unobserve(target: Element) {
    this.targets.delete(target);
  }

  disconnect() {
    this.targets.clear();
  }

  static flushWhere(predicate: (target: Element) => boolean) {
    for (const observer of ControlledResizeObserver.instances) {
      const targets = [...observer.targets].filter(predicate);
      if (targets.length === 0) continue;
      observer.callback(targets.map((target) => ({ target }) as ResizeObserverEntry), observer);
    }
  }
}

const layout: FileWorkspaceLayoutVm = {
  splitMinWidth: 500,
  treeDefaultWidth: 280,
  treeMinWidth: 200,
  treeMaxWidth: 420,
};

const resource: ConversationDirectoryWorkspaceResource = {
  kind: 'conversation-directory',
  key: conversationDirectoryWorkspaceResourceKey({
    projectId: 'project-1',
    taskId: 'task-1',
    runId: 'run-1',
    roundId: 'round-1',
    nodeId: 'node-1',
    attemptId: 'attempt-1',
  }),
  scopeKey: 'draft:default',
  title: '运行目录',
  attention: false,
  locator: {
    projectId: 'project-1',
    taskId: 'task-1',
    runId: 'run-1',
    roundId: 'round-1',
    nodeId: 'node-1',
    attemptId: 'attempt-1',
  },
};

const artifact: WorkspaceDirectoryEntryVm = {
  name: 'artifact.md',
  relativePath: 'artifact.md',
  canonicalPath: 'D:\\attempt\\artifact.md',
  kind: 'file',
  hasChildren: false,
  byteLength: 42,
  modifiedAtNs: '1',
};

function WorkspaceWidthProbe({ onWidth }: { onWidth: (width: number) => void }) {
  const workspace = useRightWorkspace();
  React.useEffect(() => {
    onWidth(workspace.width);
  }, [onWidth, workspace.width]);
  return null;
}

describe('conversation directory responsive tree lifecycle', () => {
  let panelWidth = 0;
  let treeHeight = 0;
  let animationFrameId = 0;
  let animationFrames = new Map<number, FrameRequestCallback>();
  let clientWidthDescriptor: PropertyDescriptor | undefined;
  let clientHeightDescriptor: PropertyDescriptor | undefined;

  beforeEach(() => {
    panelWidth = 0;
    treeHeight = 0;
    animationFrameId = 0;
    animationFrames = new Map();
    ControlledResizeObserver.instances = [];
    apiMocks.listConversationDirectory.mockResolvedValue([artifact]);
    apiMocks.readConversationDirectoryFile.mockResolvedValue({
      kind: 'text',
      name: artifact.name,
      content: '# Artifact',
      language: 'markdown',
      locator: { projectId: resource.locator.projectId, canonicalPath: artifact.canonicalPath, relativePath: artifact.relativePath, scope: 'workspace' },
      revision: { contentHash: 'artifact', byteLength: artifact.byteLength, modifiedAtNs: '1' },
      externalAccessGrant: null, encoding: 'utf-8', lineEnding: 'lf', editable: false, limitationCode: null,
    });

    clientWidthDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth');
    clientHeightDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientHeight');
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get() {
        if (this === document.documentElement) return 1440;
        if ((this as HTMLElement).dataset.fileWorkspacePanel === 'true') return panelWidth;
        return 0;
      },
    });
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get: () => treeHeight,
    });

    vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      const id = ++animationFrameId;
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
    if (clientWidthDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientWidth', clientWidthDescriptor);
    if (clientHeightDescriptor) Object.defineProperty(HTMLElement.prototype, 'clientHeight', clientHeightDescriptor);
  });

  it('remeasures the tree when restored width moves it from compact to split layout', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider initialWidth={760}>
            <ConversationDirectoryWorkspacePanel resource={resource} layout={layout} />
          </RightWorkspaceProvider>,
        );
      });
      await act(async () => { await Promise.resolve(); });

      const compactTree = container.querySelector<HTMLElement>('[role="tree"]');
      expect(compactTree).not.toBeNull();
      expect(compactTree?.style.height).toBe('1px');
      expect(container.textContent).toContain('artifact.md');

      panelWidth = 760;
      treeHeight = 800;
      await act(async () => {
        ControlledResizeObserver.flushWhere(
          (target) => target instanceof HTMLElement && target.dataset.fileWorkspacePanel === 'true',
        );
        const callbacks = [...animationFrames.values()];
        animationFrames.clear();
        callbacks.forEach((callback) => callback(performance.now()));
      });

      const splitTree = container.querySelector<HTMLElement>('[role="tree"]');
      expect(container.querySelector('[data-slot="resizable-panel-group"]')).not.toBeNull();
      expect(splitTree).not.toBe(compactTree);
      expect(splitTree?.style.height).toBe('800px');
      expect(container.textContent).toContain('artifact.md');
      expect(apiMocks.listConversationDirectory).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps the canonical right workspace width when the run directory opens', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const widths: number[] = [];

    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider initialWidth={397}>
            <WorkspaceWidthProbe onWidth={(width) => widths.push(width)} />
            <ConversationDirectoryWorkspacePanel resource={resource} layout={layout} />
          </RightWorkspaceProvider>,
        );
      });
      await act(async () => { await Promise.resolve(); });

      expect(widths).toEqual([397]);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it.each(['success', 'failure'])('ignores an obsolete child-directory %s', async outcome => {
    panelWidth = 760; treeHeight = 800;
    const directory = { ...artifact, name: 'reports', relativePath: 'reports', kind: 'directory' as const, hasChildren: true };
    let resolveOld!: (entries: WorkspaceDirectoryEntryVm[]) => void;
    let rejectOld!: (reason: unknown) => void;
    const old = new Promise<WorkspaceDirectoryEntryVm[]>((resolve, reject) => { resolveOld = resolve; rejectOld = reject; });
    apiMocks.listConversationDirectory.mockImplementation((input: { relativePath: string; attemptId: string }) => {
      if (!input.relativePath) return Promise.resolve([directory]);
      return input.attemptId === 'attempt-1' ? old : Promise.resolve([{ ...artifact, name: 'current.md', relativePath: 'reports/current.md' }]);
    });
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    const render = (value: ConversationDirectoryWorkspaceResource) => root.render(<TooltipProvider><RightWorkspaceProvider><ConversationDirectoryWorkspacePanel resource={value} layout={layout} /></RightWorkspaceProvider></TooltipProvider>);
    const folder = () => [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'reports')!;
    try {
      await act(async () => render(resource));
      await act(async () => folder().click());
      const next = { ...resource, locator: { ...resource.locator, attemptId: 'attempt-2' } };
      await act(async () => render(next));
      await act(async () => folder().click());
      expect(container.textContent).toContain('current.md');
      await act(async () => {
        if (outcome === 'success') resolveOld([{ ...artifact, name: 'obsolete.md', relativePath: 'reports/obsolete.md' }]);
        else rejectOld({ code: 'demo.resource-not-found', params: {} });
        await old.catch(() => undefined);
      });
      expect(container.textContent).toContain('current.md');
      expect(container.textContent).not.toContain('obsolete.md');
      expect(container.querySelector('[role="alert"]')).toBeNull();
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it('coalesces repeated child-directory clicks before React commits loading', async () => {
    panelWidth = 760; treeHeight = 800;
    const directory = { ...artifact, name: 'reports', relativePath: 'reports', kind: 'directory' as const, hasChildren: true };
    let finish!: (entries: WorkspaceDirectoryEntryVm[]) => void;
    const pending = new Promise<WorkspaceDirectoryEntryVm[]>(resolve => { finish = resolve; });
    apiMocks.listConversationDirectory.mockResolvedValueOnce([directory]).mockReturnValue(pending);
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<RightWorkspaceProvider><ConversationDirectoryWorkspacePanel resource={resource} layout={layout} /></RightWorkspaceProvider>));
      const folder = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'reports')!;
      await act(async () => { folder.click(); folder.click(); folder.click(); });
      expect(folder.getAttribute('aria-busy')).toBe('true');
      expect(apiMocks.listConversationDirectory).toHaveBeenCalledTimes(2);
      await act(async () => { finish([{ ...artifact, relativePath: 'reports/artifact.md' }]); await pending; });
      expect(folder.hasAttribute('aria-busy')).toBe(false);
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it('shows a child-directory failure with an explicit local retry', async () => {
    panelWidth = 760; treeHeight = 800;
    const directory = { ...artifact, name: 'reports', relativePath: 'reports', kind: 'directory' as const, hasChildren: true };
    apiMocks.listConversationDirectory.mockResolvedValueOnce([directory])
      .mockRejectedValueOnce({ code: 'demo.resource-not-found', params: {} })
      .mockResolvedValueOnce([{ ...artifact, relativePath: 'reports/artifact.md' }]);
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<TooltipProvider><RightWorkspaceProvider><ConversationDirectoryWorkspacePanel resource={resource} layout={layout} /></RightWorkspaceProvider></TooltipProvider>));
      const folder = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'reports')!;
      await act(async () => folder.click());
      expect(container.querySelector('[role="alert"]')).not.toBeNull();
      const retry = container.querySelector<HTMLButtonElement>('button[aria-label*="重试"]');
      expect(retry).not.toBeNull();
      await act(async () => retry!.click());
      expect(container.querySelector('[role="alert"]')).toBeNull();
      expect(container.textContent).toContain('artifact.md');
      expect(apiMocks.listConversationDirectory.mock.calls.map(([input]) => input.relativePath)).toEqual(['', 'reports', 'reports']);
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it('restores the selected file after the dock presentation remounts', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    let workspace!: ReturnType<typeof useRightWorkspace>;
    function ActivePanel({ visible }: { visible: boolean }) {
      workspace = useRightWorkspace();
      const active = workspace.tabs[0] as ConversationDirectoryWorkspaceResource | undefined;
      return visible && active ? <ConversationDirectoryWorkspacePanel resource={active} layout={layout} /> : null;
    }
    const render = (visible: boolean) => root.render(<RightWorkspaceProvider><ActivePanel visible={visible} /></RightWorkspaceProvider>);
    try {
      await act(async () => render(true));
      await act(async () => workspace.openResource(resource));
      const row = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === artifact.name);
      expect(row).toBeDefined();
      await act(async () => row!.click());
      expect(container.querySelector('[data-testid="run-directory-file-editor"]')).not.toBeNull();
      await act(async () => render(false));
      await act(async () => render(true));
      expect(container.querySelector('[data-testid="run-directory-file-editor"]')).not.toBeNull();
      expect(apiMocks.readConversationDirectoryFile).toHaveBeenCalledTimes(2);
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it('restores expanded directory paths after dock remount without reading collapsed siblings', async () => {
    panelWidth = 760; treeHeight = 800;
    const directory = { ...artifact, name: 'reports', relativePath: 'reports', kind: 'directory' as const, hasChildren: true };
    apiMocks.listConversationDirectory.mockImplementation(({ relativePath }: { relativePath: string }) => Promise.resolve(relativePath
      ? [{ ...artifact, relativePath: 'reports/artifact.md' }]
      : [directory, { ...directory, name: 'closed', relativePath: 'closed' }]));
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    let workspace!: ReturnType<typeof useRightWorkspace>;
    function Panel({ visible }: { visible: boolean }) {
      workspace = useRightWorkspace();
      const active = workspace.tabs[0] as ConversationDirectoryWorkspaceResource | undefined;
      return visible && active ? <ConversationDirectoryWorkspacePanel resource={active} layout={layout} /> : null;
    }
    const render = (visible: boolean) => root.render(<RightWorkspaceProvider><Panel visible={visible} /></RightWorkspaceProvider>);
    try {
      await act(async () => render(true));
      await act(async () => workspace.openResource(resource));
      const folder = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === 'reports')!;
      await act(async () => folder.click());
      expect(container.textContent).toContain('artifact.md');
      await act(async () => render(false));
      await act(async () => render(true));
      expect(container.textContent).toContain('artifact.md');
      expect(apiMocks.listConversationDirectory.mock.calls.map(([input]) => input.relativePath)).toEqual(['', 'reports', '', 'reports']);
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it('waits for the new root before restoring expansion after an attempt change', async () => {
    panelWidth = 760; treeHeight = 800;
    const directory = { ...artifact, name: 'reports', relativePath: 'reports', kind: 'directory' as const, hasChildren: true };
    let finish!: (entries: WorkspaceDirectoryEntryVm[]) => void;
    const pending = new Promise<WorkspaceDirectoryEntryVm[]>(resolve => { finish = resolve; });
    apiMocks.listConversationDirectory.mockImplementation(({ attemptId, relativePath }: { attemptId: string; relativePath: string }) => {
      if (relativePath) return Promise.resolve([artifact]);
      return attemptId === 'attempt-1' ? Promise.resolve([directory]) : pending;
    });
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    const render = (value: ConversationDirectoryWorkspaceResource) => root.render(<RightWorkspaceProvider><ConversationDirectoryWorkspacePanel resource={value} layout={layout} /></RightWorkspaceProvider>);
    try {
      await act(async () => render(resource));
      await act(async () => render({ ...resource, locator: { ...resource.locator, attemptId: 'attempt-2' }, expandedPaths: ['reports'] }));
      expect(apiMocks.listConversationDirectory.mock.calls.map(([input]) => [input.attemptId, input.relativePath])).toEqual([
        ['attempt-1', ''], ['attempt-2', ''],
      ]);
      await act(async () => { finish([]); await pending; });
      expect(container.textContent).not.toContain('artifact.md');
      expect(apiMocks.listConversationDirectory).toHaveBeenCalledTimes(2);
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it.each(['expanded', 'collapsed', 'failure'])('restores nested paths with an %s ancestor', async mode => {
    panelWidth = 760; treeHeight = 800;
    const directory = { ...artifact, name: 'reports', relativePath: 'reports', kind: 'directory' as const, hasChildren: true };
    apiMocks.listConversationDirectory.mockImplementation(({ relativePath }: { relativePath: string }) => {
      if (!relativePath) return Promise.resolve([directory]);
      if (mode === 'failure') return Promise.reject({ code: 'demo.resource-not-found', params: {} });
      return Promise.resolve(relativePath === 'reports'
        ? [{ ...directory, name: 'nested', relativePath: 'reports/nested' }]
        : [{ ...artifact, relativePath: 'reports/nested/artifact.md' }]);
    });
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<TooltipProvider><RightWorkspaceProvider><ConversationDirectoryWorkspacePanel
        resource={{ ...resource, expandedPaths: mode === 'collapsed' ? ['reports/nested'] : ['reports', 'reports/nested'] }} layout={layout}
      /></RightWorkspaceProvider></TooltipProvider>));
      expect(apiMocks.listConversationDirectory.mock.calls.map(([input]) => input.relativePath)).toEqual(
        mode === 'collapsed' ? [''] : mode === 'failure' ? ['', 'reports'] : ['', 'reports', 'reports/nested'],
      );
      expect(Boolean(container.querySelector('[role="alert"]'))).toBe(mode === 'failure');
      expect(container.textContent?.includes('artifact.md')).toBe(mode === 'expanded');
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it('loads an unopened directory through keyboard expansion', async () => {
    panelWidth = 760; treeHeight = 800;
    const directory = { ...artifact, name: 'reports', relativePath: 'reports', kind: 'directory' as const, hasChildren: true };
    apiMocks.listConversationDirectory.mockResolvedValueOnce([directory]).mockResolvedValue([{ ...artifact, relativePath: 'reports/artifact.md' }]);
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => root.render(<RightWorkspaceProvider><ConversationDirectoryWorkspacePanel resource={resource} layout={layout} /></RightWorkspaceProvider>));
      const tree = container.querySelector<HTMLElement>('[role="tree"]')!;
      await act(async () => tree.focus());
      await act(async () => tree.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })));
      expect(apiMocks.listConversationDirectory.mock.calls.map(([input]) => input.relativePath)).toEqual(['', 'reports']);
      expect(container.textContent).toContain('artifact.md');
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it.each(['directory', 'file'])('shows a recoverable %s read failure', async target => {
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    if (target === 'directory') apiMocks.listConversationDirectory.mockRejectedValueOnce({ code: 'demo.resource-not-found', params: {} });
    else apiMocks.readConversationDirectoryFile.mockRejectedValueOnce({ code: 'demo.resource-not-found', params: {} });
    try {
      await act(async () => root.render(<RightWorkspaceProvider><ConversationDirectoryWorkspacePanel resource={resource} layout={layout} /></RightWorkspaceProvider>));
      if (target === 'file') {
        const row = [...container.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent === artifact.name);
        await act(async () => row!.click());
      }
      const alert = container.querySelector('[role="alert"]');
      expect(alert).not.toBeNull();
      const retry = alert!.querySelector<HTMLButtonElement>('button');
      expect(retry).not.toBeNull();
      await act(async () => retry!.click());
      expect(container.querySelector('[role="alert"]')).toBeNull();
      if (target === 'file') expect(container.querySelector('[data-testid="run-directory-file-editor"]')).not.toBeNull();
      else expect(container.textContent).toContain(artifact.name);
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it('returns to file content when the same file is selected again in compact mode', async () => {
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container);
    const button = (name: string) => [...container.querySelectorAll<HTMLButtonElement>('button')].find(item => item.textContent === name)!;
    try {
      await act(async () => root.render(<RightWorkspaceProvider><ConversationDirectoryWorkspacePanel resource={resource} layout={layout} /></RightWorkspaceProvider>));
      await act(async () => button(artifact.name).click());
      expect(container.querySelector('[data-testid="run-directory-file-editor"]')).not.toBeNull();
      await act(async () => button('目录').click());
      await act(async () => button(artifact.name).click());
      expect(container.querySelector('[data-testid="run-directory-file-editor"]')).not.toBeNull();
    } finally { await act(async () => root.unmount()); container.remove(); }
  });

  it('opens Markdown run artifacts in the shared read-only preview and source viewer', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider initialWidth={397}>
            <ConversationDirectoryWorkspacePanel resource={resource} layout={layout} />
          </RightWorkspaceProvider>,
        );
      });
      await act(async () => { await Promise.resolve(); });

      const row = [...container.querySelectorAll<HTMLButtonElement>('button')]
        .find((button) => button.textContent?.includes(artifact.name));
      await act(async () => {
        row?.click();
        await Promise.resolve();
      });

      let editor = container.querySelector<HTMLElement>('[data-testid="run-directory-file-editor"]');
      expect(apiMocks.readConversationDirectoryFile).toHaveBeenCalledWith({
        ...resource.locator,
        relativePath: artifact.relativePath,
      });
      expect(editor?.dataset.editable).toBe('false');
      expect(editor?.dataset.markdownMode).toBe('live-preview');

      await act(async () => {
        editor?.querySelector<HTMLButtonElement>('button')?.click();
      });
      editor = container.querySelector<HTMLElement>('[data-testid="run-directory-file-editor"]');
      expect(editor?.dataset.markdownMode).toBe('source');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps an open directory context menu mounted across unrelated parent updates', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const Harness = ({ revision }: { revision: number }) => (
      <div data-parent-revision={revision}>
        <RightWorkspaceProvider initialWidth={397}>
          <ConversationDirectoryWorkspacePanel resource={resource} layout={layout} />
        </RightWorkspaceProvider>
      </div>
    );

    try {
      await act(async () => {
        root.render(<Harness revision={1} />);
      });
      await act(async () => { await Promise.resolve(); });

      const rowBefore = [...container.querySelectorAll('button')]
        .find((button) => button.textContent?.includes(artifact.name));
      expect(rowBefore).toBeDefined();
      await act(async () => {
        rowBefore?.dispatchEvent(new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          clientX: 12,
          clientY: 12,
        }));
      });
      const menuBefore = document.querySelector('[data-slot="context-menu-content"]');
      expect(menuBefore).not.toBeNull();

      await act(async () => {
        root.render(<Harness revision={2} />);
      });

      const rowAfter = [...container.querySelectorAll('button')]
        .find((button) => button.textContent?.includes(artifact.name));
      expect(rowAfter).toBe(rowBefore);
      expect(document.querySelector('[data-slot="context-menu-content"]')).toBe(menuBefore);
      expect(apiMocks.listConversationDirectory).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('reloads for the selected attempt and ignores a late response from the previous attempt', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    let resolveFirst: ((entries: WorkspaceDirectoryEntryVm[]) => void) | null = null;
    let resolveSecond: ((entries: WorkspaceDirectoryEntryVm[]) => void) | null = null;
    const first = new Promise<WorkspaceDirectoryEntryVm[]>((resolve) => { resolveFirst = resolve; });
    const second = new Promise<WorkspaceDirectoryEntryVm[]>((resolve) => { resolveSecond = resolve; });
    apiMocks.listConversationDirectory.mockReset();
    apiMocks.listConversationDirectory
      .mockReturnValueOnce(first)
      .mockReturnValueOnce(second);
    const nextArtifact = { ...artifact, name: 'attempt-2.md', relativePath: 'attempt-2.md', canonicalPath: 'D:\\attempt-2\\attempt-2.md' };
    const nextResource: ConversationDirectoryWorkspaceResource = {
      ...resource,
      locator: { ...resource.locator, nodeId: 'node-2', attemptId: 'attempt-2' },
    };

    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider initialWidth={397}>
            <ConversationDirectoryWorkspacePanel resource={resource} layout={layout} />
          </RightWorkspaceProvider>,
        );
      });
      await act(async () => {
        root.render(
          <RightWorkspaceProvider initialWidth={397}>
            <ConversationDirectoryWorkspacePanel resource={nextResource} layout={layout} />
          </RightWorkspaceProvider>,
        );
      });
      await act(async () => { resolveSecond?.([nextArtifact]); await second; });

      expect(container.textContent).toContain(nextArtifact.name);
      expect(container.textContent).not.toContain(artifact.name);
      await act(async () => { resolveFirst?.([artifact]); await first; });
      expect(container.textContent).toContain(nextArtifact.name);
      expect(container.textContent).not.toContain(artifact.name);
      expect(apiMocks.listConversationDirectory).toHaveBeenNthCalledWith(1, { ...resource.locator, relativePath: '' });
      expect(apiMocks.listConversationDirectory).toHaveBeenNthCalledWith(2, { ...nextResource.locator, relativePath: '' });
    } finally {
      await act(async () => root.unmount());
    }
  });
});
