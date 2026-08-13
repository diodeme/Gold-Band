/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  listConversationDirectory: vi.fn(),
}));

vi.mock('@/api', () => ({
  listConversationDirectory: apiMocks.listConversationDirectory,
  openConversationDirectoryPathInFileManager: vi.fn(),
  readConversationDirectoryFile: vi.fn(),
  workspaceFilePreviewUrl: vi.fn(() => ''),
}));

import { ConversationDirectoryWorkspacePanel } from '@/components/workspace/ConversationDirectoryWorkspacePanel';
import { RightWorkspaceProvider, type ConversationDirectoryWorkspaceResource } from '@/components/workspace/right-workspace-context';
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
  preferredWidth: 760,
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
});
