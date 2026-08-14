/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const resizablePanelCalls = vi.hoisted(() => new Map<string, string[]>());

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, saveConversationPreference: vi.fn().mockResolvedValue(undefined) };
});

vi.mock('@/components/AppTitleBar', () => ({
  AppTitleBar: () => <header />,
}));

vi.mock('@/components/conversation/ConversationSidebar', () => ({
  ConversationSidebar: () => <aside />,
}));

vi.mock('@/components/ui/resizable', async () => {
  const ReactModule = await vi.importActual<typeof import('react')>('react');
  return {
    ResizablePanelGroup: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
    ResizablePanel: ({ children, id, panelRef }: { children: React.ReactNode; id: string; panelRef?: React.Ref<unknown> }) => {
      const collapsed = ReactModule.useRef(false);
      ReactModule.useImperativeHandle(panelRef, () => ({
        collapse: () => { collapsed.current = true; },
        expand: () => { collapsed.current = false; },
        isCollapsed: () => collapsed.current,
        resize: (size: number | string) => {
          const calls = resizablePanelCalls.get(id) ?? [];
          calls.push(`resize:${size}`);
          resizablePanelCalls.set(id, calls);
        },
      }));
      return <div>{children}</div>;
    },
    ResizableHandle: () => <div />,
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
  resizablePanelCalls.clear();
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
      expect(resizablePanelCalls.get('workspace-navigation')).toEqual(['resize:256']);

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

      expect(resizablePanelCalls.get('workspace-navigation')).toEqual(['resize:256', 'resize:176']);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
