/** @vitest-environment jsdom */

import React, { act, useEffect } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/lib/conversation-event-router', async () => {
  const actual = await vi.importActual<typeof import('@/lib/conversation-event-router')>('@/lib/conversation-event-router');
  return {
    ...actual,
    useConversationBranchLiveSnapshot: () => ({ revision: 0, contentRevision: 0, status: null, attention: false }),
  };
});

vi.mock('@/components/workspace/AgentConversationPanel', () => ({
  AgentConversationPanel: ({ resource }: { resource: { locator: { branchId: string } } }) => (
    <div data-conversation-viewport="true" data-rendered-agent-branch={resource.locator.branchId} />
  ),
}));

import { ACPMessageList, buildAcpTimelineProjection } from '@/components/acp/ACPChatDialog';
import { AvatarPreferencesProvider } from '@/components/avatar/AvatarPreferencesContext';
import { RightWorkspaceDock } from '@/components/workspace/RightWorkspaceDock';
import {
  agentTranscriptResourceKey,
  ConversationWorkspaceStore,
  createConversationWorkspaceScope,
  RightWorkspaceProvider,
  useRightWorkspace,
  type AgentTranscriptLocator,
} from '@/components/workspace/right-workspace-context';
import type { AcpSessionVm, AcpUiEventVm } from '@/types';
import { createDefaultAvatarPreferences } from '@/lib/avatar';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

class ControlledResizeObserver {
  static instances: ControlledResizeObserver[] = [];
  private readonly callback: ResizeObserverCallback;

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    ControlledResizeObserver.instances.push(this);
  }

  observe() {}
  unobserve() {}
  disconnect() {}

  flush(target: Element) {
    this.callback([{ target } as ResizeObserverEntry], this as unknown as ResizeObserver);
  }
}

const locator = (branchId: string): AgentTranscriptLocator => ({
  projectId: 'project-1',
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
  branchId,
});

function resource(branchId: string) {
  const branchLocator = locator(branchId);
  return {
    kind: 'agent-transcript' as const,
    key: agentTranscriptResourceKey(branchLocator),
    scopeKey: 'draft:default',
    title: branchId,
    status: 'running',
    attention: false,
    locator: branchLocator,
  };
}

function SeedTabs({ branches }: { branches: string[] }) {
  const workspace = useRightWorkspace();
  useEffect(() => {
    for (const branch of branches) workspace.openResource(resource(branch));
  }, [branches, workspace.openResource]);
  return null;
}

function OpenEmptyWorkspace() {
  const workspace = useRightWorkspace();
  useEffect(() => workspace.openWorkspace(), [workspace.openWorkspace]);
  return null;
}

function SeedWorkflowResource({ guarded = false }: { guarded?: boolean }) {
  const workspace = useRightWorkspace();
  useEffect(() => {
    const resource = {
      kind: 'workflow-view' as const,
      key: 'workflow-view:project-1:task-1:run-1',
      scopeKey: 'draft:default',
      title: 'Workflow',
      attention: false,
      locator: { projectId: 'project-1', taskId: 'task-1', runId: 'run-1' },
    };
    workspace.openResource(resource);
    const unregisterRenderer = workspace.registerResourceRenderer('workflow-view', (active) => (
      <div data-rendered-resource={active.kind}>{active.title}</div>
    ));
    const unregisterGuard = guarded
      ? workspace.registerResourceCloseResolver('workflow-view', () => false)
      : () => {};
    return () => {
      unregisterGuard();
      unregisterRenderer();
    };
  }, [guarded, workspace.openResource, workspace.registerResourceCloseResolver, workspace.registerResourceRenderer]);
  return null;
}

function WorkspaceProbe() {
  const workspace = useRightWorkspace();
  return (
    <output
      data-workspace-tab-count={workspace.tabs.length}
      data-workspace-active-tab={workspace.activeTabKey ?? ''}
      data-workspace-width={workspace.width}
    >
      {workspace.tabs.map((tab) => tab.kind === 'agent-transcript' ? tab.locator.branchId : tab.key).join(',')}
    </output>
  );
}

function TransitionGuardProbe({ resolve }: { resolve: ReturnType<typeof vi.fn> }) {
  const workspace = useRightWorkspace();
  const makeResource = (key: string) => ({
    kind: 'workflow-view' as const,
    key,
    scopeKey: 'draft:default',
    title: key,
    attention: false,
    locator: { projectId: 'default', taskId: key, runId: 'run-1' },
  });
  useEffect(() => workspace.registerResourceCloseResolver('workflow-view', resolve), [resolve, workspace.registerResourceCloseResolver]);
  useEffect(() => {
    void workspace.openResource(makeResource('first'));
  }, [workspace.openResource]);
  return <button type="button" data-open-second onClick={() => void workspace.openResource(makeResource('second'))}>open second</button>;
}

beforeEach(() => {
  ControlledResizeObserver.instances = [];
  vi.stubGlobal('ResizeObserver', ControlledResizeObserver);
});

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe('right workspace DOM lifecycle', () => {
  it('honors transition guards and reports deactivation separately from actual close', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const resolve = vi.fn().mockResolvedValue(false);
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <TransitionGuardProbe resolve={resolve} />
            <WorkspaceProbe />
          </RightWorkspaceProvider>,
        );
      });
      await act(async () => container.querySelector<HTMLButtonElement>('[data-open-second]')?.click());
      expect(resolve).toHaveBeenCalledWith(expect.objectContaining({ key: 'first' }), 'deactivate');
      expect(container.querySelector('output')?.getAttribute('data-workspace-tab-count')).toBe('1');
      expect(container.querySelector('output')?.getAttribute('data-workspace-active-tab')).toBe('first');

      resolve.mockResolvedValue(true);
      await act(async () => container.querySelector<HTMLButtonElement>('[data-open-second]')?.click());
      expect(container.querySelector('output')?.getAttribute('data-workspace-tab-count')).toBe('2');
      expect(container.querySelector('output')?.getAttribute('data-workspace-active-tab')).toBe('second');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('restores the active conversation snapshot without rendering another scope tabs', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const store = new ConversationWorkspaceStore();
    const first = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', runId: 'run-1' });
    const second = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-2', runId: 'run-1' });
    store.save(first, {
      tabs: [{ ...resource('agent-a'), scopeKey: first.key }],
      activeTabKey: resource('agent-a').key,
      requestedOpen: true,
      openRevision: 1,
    });
    store.save(second, {
      tabs: [{ ...resource('agent-b'), scopeKey: second.key }],
      activeTabKey: resource('agent-b').key,
      requestedOpen: true,
      openRevision: 1,
    });
    try {
      await act(async () => {
        root.render(<RightWorkspaceProvider scope={first} store={store}><WorkspaceProbe /></RightWorkspaceProvider>);
      });
      expect(container.querySelector('output')?.textContent).toBe('agent-a');
      await act(async () => {
        root.render(<RightWorkspaceProvider scope={second} store={store}><WorkspaceProbe /></RightWorkspaceProvider>);
      });
      expect(container.querySelector('output')?.textContent).toBe('agent-b');
      await act(async () => {
        root.render(<RightWorkspaceProvider scope={first} store={store}><WorkspaceProbe /></RightWorkspaceProvider>);
      });
      expect(container.querySelector('output')?.textContent).toBe('agent-a');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('rejects a resource descriptor owned by another conversation scope', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const scope = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', runId: 'run-1' });
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider scope={scope}>
            <SeedTabs branches={['agent-from-default-draft']} />
            <WorkspaceProbe />
          </RightWorkspaceProvider>,
        );
      });
      expect(container.querySelector('output')?.getAttribute('data-workspace-tab-count')).toBe('0');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('renders a blank entry surface when the workspace is open without resources', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <OpenEmptyWorkspace />
            <RightWorkspaceDock />
          </RightWorkspaceProvider>,
        );
      });
      expect(container.querySelector('[data-right-workspace-empty="true"]')).not.toBeNull();
      expect(container.querySelector('[data-right-workspace-tab-strip="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('renders only the active locator resource through the conversation host and honors its close guard', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <SeedWorkflowResource guarded />
            <RightWorkspaceDock />
            <WorkspaceProbe />
          </RightWorkspaceProvider>,
        );
      });
      expect(container.querySelector('[data-rendered-resource="workflow-view"]')?.textContent).toBe('Workflow');
      const closeButton = container.querySelector<HTMLButtonElement>('[aria-label="Close tab"], [aria-label="关闭标签页"]');
      await act(async () => closeButton?.dispatchEvent(new MouseEvent('click', { bubbles: true })));
      expect(container.querySelector('output')?.getAttribute('data-workspace-tab-count')).toBe('1');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('hydrates the persisted width after the asynchronous sidebar VM arrives', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<RightWorkspaceProvider initialWidth={440}><WorkspaceProbe /></RightWorkspaceProvider>);
      });
      expect(container.querySelector('output')?.getAttribute('data-workspace-width')).toBe('440');
      await act(async () => {
        root.render(<RightWorkspaceProvider initialWidth={612}><WorkspaceProbe /></RightWorkspaceProvider>);
      });
      expect(container.querySelector('output')?.getAttribute('data-workspace-width')).toBe('612');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows the compact Tab list only when the native Tab strip overflows', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <SeedTabs branches={['agent-a', 'agent-b']} />
            <RightWorkspaceDock />
          </RightWorkspaceProvider>,
        );
      });
      const tabStrip = container.querySelector<HTMLElement>('[data-right-workspace-tab-strip="true"]');
      expect(tabStrip).not.toBeNull();
      expect(container.querySelector('[data-right-workspace-overflow-menu="true"]')).toBeNull();

      Object.defineProperties(tabStrip!, {
        clientWidth: { configurable: true, value: 180 },
        scrollWidth: { configurable: true, value: 320 },
      });
      await act(async () => ControlledResizeObserver.instances.at(-1)?.flush(tabStrip!));
      expect(container.querySelector('[data-right-workspace-overflow-menu="true"]')).not.toBeNull();

      Object.defineProperty(tabStrip!, 'scrollWidth', { configurable: true, value: 160 });
      await act(async () => ControlledResizeObserver.instances.at(-1)?.flush(tabStrip!));
      expect(container.querySelector('[data-right-workspace-overflow-menu="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('mounts a ConversationViewport only for the active Tab', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const branches = ['agent-a', 'agent-b'];
    try {
      await act(async () => {
        root.render(
          <RightWorkspaceProvider>
            <SeedTabs branches={branches} />
            <RightWorkspaceDock />
          </RightWorkspaceProvider>,
        );
      });
      expect(container.querySelectorAll('[data-conversation-viewport="true"]')).toHaveLength(1);
      expect(container.querySelector('[data-rendered-agent-branch="agent-b"]')).not.toBeNull();
      const activeTab = container.querySelector('[data-right-workspace-tab][data-state="active"]');
      expect(activeTab?.textContent).toContain('agent-b');
      expect(activeTab?.className).toContain('rounded-xl');
      expect(activeTab?.className).toContain('bg-muted/70');
      expect(activeTab?.className).not.toContain('border-r');
      expect(activeTab?.className).not.toContain('after:');

      const agentATab = Array.from(container.querySelectorAll('button'))
        .find((button) => button.textContent?.includes('agent-a'));
      await act(async () => {
        agentATab?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(container.querySelectorAll('[data-conversation-viewport="true"]')).toHaveLength(1);
      expect(container.querySelector('[data-rendered-agent-branch="agent-a"]')).not.toBeNull();
      expect(container.querySelector('[data-rendered-agent-branch="agent-b"]')).toBeNull();
      expect(container.querySelector('[data-right-workspace-tab][data-state="active"]')?.textContent).toContain('agent-a');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('opens a nested Agent link as another workspace Tab', async () => {
    const childId = 'agent-child';
    const launch: AcpUiEventVm = {
      id: 'launch-child',
      seq: 2,
      timestamp: '2Z',
      kind: 'toolCall',
      sessionId: 'session-1',
      content: null,
      title: 'Agent child',
      toolCallId: 'provider-child',
      status: 'running',
      raw: {
        rawInput: { description: 'Nested child' },
        _meta: {
          goldBandConversation: {
            branchId: 'agent-parent',
            launchedAgentExecutionId: childId,
            toolName: 'Agent',
          },
        },
      },
    };
    const projectionVm: NonNullable<AcpSessionVm['timelineProjection']> = {
      todoEntries: [],
      agents: [{
        agentExecutionId: childId,
        parentAgentExecutionId: 'agent-parent',
        executionStatus: 'running',
        eventCount: 1,
        toolCallCount: 0,
        readFileCount: 0,
        writtenFileCount: 0,
        hasAttention: false,
        description: 'Nested child',
        todoEntries: [],
      }],
    };
    const projection = buildAcpTimelineProjection([launch], 'running', projectionVm);
    const avatarPreferences = createDefaultAvatarPreferences();
    avatarPreferences.agent = {
      shape: 'square',
      selectedAvatarId: 'agent-avatar',
      recentAvatars: [{
        id: 'agent-avatar',
        dataUrl: 'data:image/png;base64,AQ==',
        createdAt: '2026-08-02T00:00:00Z',
      }],
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const parentBranches = ['agent-parent'];
    try {
      await act(async () => {
        root.render(
          <AvatarPreferencesProvider preferences={avatarPreferences}>
            <RightWorkspaceProvider>
              <SeedTabs branches={parentBranches} />
              <ACPMessageList
                timeline={projection.timeline}
                sessionStatus="running"
                sending={false}
                branchLocator={locator('agent-parent')}
              />
              <WorkspaceProbe />
            </RightWorkspaceProvider>
          </AvatarPreferencesProvider>,
        );
      });
      const link = container.querySelector<HTMLButtonElement>(`[data-agent-link-branch-id="${childId}"]`);
      expect(link).not.toBeNull();
      const agentAvatar = link?.querySelector('[data-slot="avatar"]');
      expect(agentAvatar).not.toBeNull();
      expect(agentAvatar?.className).toContain('rounded-md');
      await act(async () => {
        link?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      const probe = container.querySelector('output');
      expect(probe?.getAttribute('data-workspace-tab-count')).toBe('2');
      expect(probe?.textContent).toBe('agent-parent,agent-child');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
