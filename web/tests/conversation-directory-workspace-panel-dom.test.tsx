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
import { RightWorkspaceProvider, useRightWorkspace, type ConversationDirectoryWorkspaceResource } from '@/components/workspace/right-workspace-context';
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
  key: 'conversation-directory:project-1:task-1:run-1:round-1:node-1:attempt-1',
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
});
