/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const resizablePanelBehaviors = vi.hoisted(() => new Map<string, string[]>());
const resizablePanelHandles = vi.hoisted(() => new Map<string, { collapsed: boolean; calls: string[] }>());
const resizableGroupLayouts = vi.hoisted(() => [] as Array<Record<string, number>>);
const resizableGroupWidth = vi.hoisted(() => ({ value: 1_290 }));
const resizableGroupEvents = vi.hoisted(() => ({
  onLayoutChanged: null as null | ((layout: Record<string, number>, meta: { isUserInteraction: boolean }) => void),
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, saveConversationPreference: vi.fn().mockResolvedValue(undefined) };
});

vi.mock('@/components/AppTitleBar', () => ({
  AppTitleBar: () => <header />,
}));

vi.mock('@/components/conversation/ConversationSidebar', () => ({
  ConversationSidebar: () => <aside data-testid="conversation-sidebar" />,
}));

vi.mock('@/components/workspace/RightWorkspaceDock', () => ({
  RightWorkspaceDock: () => <aside data-testid="right-workspace-dock" />,
}));

vi.mock('@/components/ui/resizable', async () => {
  const ReactModule = await vi.importActual<typeof import('react')>('react');
  return {
    ResizablePanelGroup: ({ children, elementRef, groupRef, onLayoutChanged }: { children: React.ReactNode; elementRef?: React.Ref<HTMLDivElement>; groupRef?: React.Ref<unknown>; onLayoutChanged?: (layout: Record<string, number>, meta: { isUserInteraction: boolean }) => void }) => {
      const layoutRef = ReactModule.useRef<Record<string, number>>({});
      resizableGroupEvents.onLayoutChanged = onLayoutChanged ?? null;
      ReactModule.useImperativeHandle(groupRef, () => ({
        getLayout: () => layoutRef.current,
        setLayout: (layout: Record<string, number>) => {
          const applied = { ...layout };
          for (const [id, size] of Object.entries(layout)) {
            const state = resizablePanelHandles.get(id);
            if (state && size <= 0.01) state.collapsed = true;
          }
          for (const [id, size] of Object.entries(layout)) {
            const state = resizablePanelHandles.get(id);
            if (!state?.collapsed || size <= 0.01) continue;
            applied[id] = 0;
            applied['workspace-center'] = (applied['workspace-center'] ?? 0) + size;
          }
          layoutRef.current = applied;
          resizableGroupLayouts.push(applied);
          return applied;
        },
      }));
      return (
        <div
          ref={(node) => {
            if (node) Object.defineProperty(node, 'clientWidth', { configurable: true, value: resizableGroupWidth.value });
            if (typeof elementRef === 'function') elementRef(node);
            else if (elementRef) elementRef.current = node;
          }}
        >
          {children}
        </div>
      );
    },
    ResizablePanel: ({ children, groupResizeBehavior, id, panelRef }: { children: React.ReactNode; groupResizeBehavior: string; id: string; panelRef?: React.Ref<unknown> }) => {
      const behaviors = resizablePanelBehaviors.get(id) ?? [];
      behaviors.push(groupResizeBehavior);
      resizablePanelBehaviors.set(id, behaviors);
      const state = resizablePanelHandles.get(id) ?? { collapsed: false, calls: [] };
      resizablePanelHandles.set(id, state);
      ReactModule.useImperativeHandle(panelRef, () => ({
        collapse: () => { state.collapsed = true; state.calls.push('collapse'); },
        expand: () => { state.collapsed = false; state.calls.push('expand'); },
        getSize: () => ({ asPercentage: 0, inPixels: 0 }),
        isCollapsed: () => state.collapsed,
        resize: () => {},
      }));
      return <div data-panel="">{children}</div>;
    },
    ResizableHandle: ({ elementRef, id }: { elementRef?: React.Ref<HTMLDivElement>; id?: string }) => <div ref={elementRef} id={id} tabIndex={0} />,
  };
});

vi.mock('@/components/ui/sheet', () => ({
  Sheet: () => null,
  SheetContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SheetTitle: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { WorkspaceShell } from '@/components/workspace/WorkspaceShell';
import { FALLBACK_WORKSPACE_LAYOUT } from '@/components/workspace/workspace-layout';
import { ConversationWorkspaceStore } from '@/components/workspace/right-workspace-context';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

beforeEach(() => {
  resizablePanelBehaviors.clear();
  resizablePanelHandles.clear();
  resizableGroupLayouts.splice(0, resizableGroupLayouts.length);
  resizableGroupWidth.value = 1_290;
  resizableGroupEvents.onLayoutChanged = null;
  vi.clearAllMocks();
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('WorkspaceShell sidebar width hydration', () => {
  it('applies a persisted minimum width that arrives after the panel registers', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const commonProps = {
      appName: 'Gold Band',
      windowFrameStyle: 'native-compositor' as const,
      appConfig: {
        acpSessionTitleRefreshEnabled: false,
        acpChatEventPageSize: 360,
        turnFiles: { cardPreviewLimit: 3 },
        workspaceLayout: FALLBACK_WORKSPACE_LAYOUT,
      },
      active: { kind: 'conversation-home' } as const,
      conversationWorkspaceStore: new ConversationWorkspaceStore(),
      sidebarCollapsed: false,
      onSelect: () => {},
      onToggleSidebar: () => {},
      onNewConversation: () => {},
      onSearch: () => {},
      onSelectTask: () => {},
      onSelectRun: () => {},
      onPinTask: () => {},
      onUnpinTask: () => {},
      onRenameTask: () => {},
      onDeleteTask: () => {},
    };

    try {
      await act(async () => {
        root.render(
          <WorkspaceShell
            {...commonProps}
            vm={{ workspaces: [], pinnedTasks: [], tasksByWorkspace: {}, preferences: null }}
          >
            <div />
          </WorkspaceShell>,
        );
      });
      expect(resizableGroupLayouts.at(-1)).toMatchObject({
        'workspace-navigation': 256 / 1_290 * 100,
        'workspace-right': 0,
      });

      await act(async () => {
        root.render(
          <WorkspaceShell
            {...commonProps}
            vm={{ workspaces: [], pinnedTasks: [], tasksByWorkspace: {}, preferences: { 'sidebar.width': 176 } }}
          >
            <div />
          </WorkspaceShell>,
        );
      });

      expect(resizableGroupLayouts.at(-1)).toMatchObject({
        'workspace-navigation': 176 / 1_290 * 100,
        'workspace-right': 0,
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps both side-panel width owners stable across run-mode capability changes', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const store = new ConversationWorkspaceStore();
    store.openWorkspace({ explicit: true });
    const commonProps = {
      appName: 'Gold Band',
      windowFrameStyle: 'native-compositor' as const,
      appConfig: {
        acpSessionTitleRefreshEnabled: false,
        acpChatEventPageSize: 360,
        turnFiles: { cardPreviewLimit: 3 },
        workspaceLayout: FALLBACK_WORKSPACE_LAYOUT,
      },
      vm: {
        workspaces: [],
        pinnedTasks: [],
        tasksByWorkspace: {},
        preferences: { 'sidebar.width': 176, 'rightWorkspace.width': 690 },
      },
      conversationWorkspaceStore: store,
      sidebarCollapsed: false,
      onSelect: () => {},
      onToggleSidebar: () => {},
      onNewConversation: () => {},
      onSearch: () => {},
      onSelectTask: () => {},
      onSelectRun: () => {},
      onPinTask: () => {},
      onUnpinTask: () => {},
      onRenameTask: () => {},
      onDeleteTask: () => {},
    };

    try {
      await act(async () => {
        root.render(
          <WorkspaceShell {...commonProps} active={{ kind: 'conversation-home' }}>
            <div />
          </WorkspaceShell>,
        );
      });
      expect(container.querySelector('[data-testid="conversation-sidebar"]')).not.toBeNull();
      expect(container.querySelector('[data-testid="right-workspace-dock"]')).not.toBeNull();

      await act(async () => {
        root.render(
          <WorkspaceShell {...commonProps} active={{ kind: 'run-mode-management' }}>
            <div />
          </WorkspaceShell>,
        );
      });
      expect(container.querySelector('[data-testid="conversation-sidebar"]')).not.toBeNull();
      expect(container.querySelector('[data-testid="right-workspace-dock"]')).toBeNull();
      const leftPanelState = resizablePanelHandles.get('workspace-navigation');
      if (leftPanelState) leftPanelState.collapsed = true;

      await act(async () => {
        root.render(
          <WorkspaceShell {...commonProps} active={{ kind: 'conversation-home' }}>
            <div />
          </WorkspaceShell>,
        );
      });

      expect(container.querySelector('[data-testid="conversation-sidebar"]')).not.toBeNull();
      expect(container.querySelector('[data-testid="right-workspace-dock"]')).not.toBeNull();
      expect(resizablePanelBehaviors.get('workspace-navigation')?.every((value) => value === 'preserve-pixel-size')).toBe(true);
      expect(resizablePanelBehaviors.get('workspace-center')?.every((value) => value === 'preserve-relative-size')).toBe(true);
      expect(resizablePanelBehaviors.get('workspace-right')?.every((value) => value === 'preserve-pixel-size')).toBe(true);
      expect(resizableGroupLayouts.at(-1)).toMatchObject({
        'workspace-navigation': 176 / 1_290 * 100,
        'workspace-center': 424 / 1_290 * 100,
        'workspace-right': 690 / 1_290 * 100,
      });
      expect(resizablePanelHandles.get('workspace-navigation')?.calls).toContain('expand');
      expect(resizablePanelHandles.get('workspace-right')?.calls).toContain('expand');
      expect(store.peekShellState()).toMatchObject({ requestedOpen: true, width: 690 });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('persists a right drag from the completed layout even when the separator exposes no pointer intent', async () => {
    const { saveConversationPreference } = await import('@/api');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const store = new ConversationWorkspaceStore();
    store.openWorkspace({ explicit: true });
    resizableGroupWidth.value = 1_271;

    try {
      await act(async () => {
        root.render(
          <WorkspaceShell
            appName="Gold Band"
            windowFrameStyle="native-compositor"
            appConfig={{
              acpSessionTitleRefreshEnabled: false,
              acpChatEventPageSize: 360,
              turnFiles: { cardPreviewLimit: 3 },
              workspaceLayout: FALLBACK_WORKSPACE_LAYOUT,
            }}
            vm={{
              workspaces: [],
              pinnedTasks: [],
              tasksByWorkspace: {},
              preferences: { 'sidebar.width': 176, 'rightWorkspace.width': 772 },
            }}
            active={{ kind: 'conversation-home' }}
            conversationWorkspaceStore={store}
            sidebarCollapsed={false}
            onSelect={() => {}}
            onToggleSidebar={() => {}}
            onNewConversation={() => {}}
            onSearch={() => {}}
            onSelectTask={() => {}}
            onSelectRun={() => {}}
            onPinTask={() => {}}
            onUnpinTask={() => {}}
            onRenameTask={() => {}}
            onDeleteTask={() => {}}
          >
            <div />
          </WorkspaceShell>,
        );
      });

      await act(async () => {
        resizableGroupEvents.onLayoutChanged?.({
          'workspace-navigation': 13.869,
          'workspace-center': 28.369,
          'workspace-right': 57.762,
        }, { isUserInteraction: false });
        resizableGroupEvents.onLayoutChanged?.({
          'workspace-navigation': 13.869,
          'workspace-center': 63.436,
          'workspace-right': 22.695,
        }, { isUserInteraction: true });
      });

      expect(store.peekShellState().width).toBe(288);
      expect(saveConversationPreference).toHaveBeenCalledWith('rightWorkspace.width', 288);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
