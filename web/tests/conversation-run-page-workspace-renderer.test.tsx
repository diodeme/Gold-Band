/** @vitest-environment jsdom */

import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const workspace = vi.hoisted(() => ({
  scopeKey: 'conversation:project-1:task-1:run-1',
  openResource: vi.fn(),
  registerResourceRenderer: vi.fn(() => vi.fn()),
  registerResourceCloseResolver: vi.fn(() => vi.fn()),
  setConversationDirectoryEntry: vi.fn(),
  refreshResourceRenderers: vi.fn(),
  tabs: [{ key: 'workflow-view', kind: 'workflow-view' }],
  activeTabKey: 'workflow-view',
}));

vi.mock('@/components/ReadOnlyExperience', () => ({
  useReadOnlyExperience: () => false,
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/components/ui/tooltip', () => ({
  TooltipProvider: ({ children }: { children: unknown }) => children,
}));

vi.mock('@/components/ui/alert-dialog', () => ({
  AlertDialog: ({ open, children }: { open?: boolean; children: unknown }) =>
    open ? <div data-dialog="alert">{children}</div> : null,
  AlertDialogAction: ({ children }: { children: unknown }) => children,
  AlertDialogCancel: ({ children }: { children: unknown }) => children,
  AlertDialogContent: ({ children }: { children: unknown }) => children,
  AlertDialogDescription: ({ children }: { children: unknown }) => children,
  AlertDialogFooter: ({ children }: { children: unknown }) => children,
  AlertDialogHeader: ({ children }: { children: unknown }) => children,
  AlertDialogTitle: ({ children }: { children: unknown }) => children,
}));

vi.mock('@/components/BrandLoadingState', () => ({
  BrandLoadingState: ({ label }: { label: string }) => <div>{label}</div>,
}));

vi.mock('@/components/conversation/ConversationRunHeader', () => ({
  ConversationRunHeader: ({ onConfigureAuto }: { onConfigureAuto?: () => void }) => (
    <button type="button" onClick={onConfigureAuto}>configure-auto</button>
  ),
}));

vi.mock('@/components/conversation/ConversationSessionSwitcher', () => ({
  ConversationSessionSwitcher: () => null,
}));

vi.mock('@/components/acp/ACPChatDialog', () => ({
  ACPChatDialog: ({ readOnly, showDisabledComposer, showBranchControl, manualCheckPending, runtimeComposerContext }: {
    readOnly?: boolean;
    showDisabledComposer?: boolean;
    showBranchControl?: boolean;
    manualCheckPending?: boolean;
    runtimeComposerContext?: { runtimeError?: string | null; runtimeErrorFallback?: string | null };
  }) => (
    <div data-testid="acp-chat" data-runtime-error={runtimeComposerContext?.runtimeError ?? ''} data-runtime-fallback={runtimeComposerContext?.runtimeErrorFallback ?? ''}>
      {showDisabledComposer ? <textarea aria-label="composer" disabled={readOnly} /> : null}
      {showBranchControl ? <button type="button">branch</button> : null}
      {manualCheckPending ? <button type="button">manual-check</button> : null}
    </div>
  ),
  createAcpEventWindowCacheKey: () => 'window',
  hasHydratedAcpSessionContent: () => true,
  shouldShowConversationContentLoadingState: () => false,
}));

vi.mock('@/components/theme/ThemeAssetsContext', () => ({
  useThemeWallpaperSurface: () => undefined,
}));

vi.mock('@/components/workspace/ConversationRunWorkspaceResourcePanel', () => ({
  ConversationRunWorkspaceResourcePanel: ({
    onNodeRetry,
  }: {
    onNodeRetry?: (node: {
      roundId: string;
      nodeId: string;
      attemptId: string;
      status: string;
      retryable: boolean;
    }) => Promise<void>;
  }) => (
    <div data-testid="workflow-panel">
      <button
        type="button"
        data-testid="node-retry-trigger"
        onClick={() => {
          void onNodeRetry?.({
            roundId: 'round-1',
            nodeId: 'dev',
            attemptId: 'attempt-1',
            status: 'completed',
            retryable: true,
          });
        }}
      >
        retry node
      </button>
    </div>
  ),
  confirmCloseConversationRunWorkspaceResource: () => true,
}));

vi.mock('@/components/workspace/right-workspace-context', () => ({
  useRightWorkspace: () => workspace,
  conversationRunWorkspaceResourceKey: (kind: string, locator: { projectId: string; taskUuid?: string; runId: string }) => (
    `${kind}:${locator.projectId}:${locator.taskUuid ?? 'missing-task-uuid'}:${locator.runId}`
  ),
}));

vi.mock('@/api', () => ({
  submitManualCheck: vi.fn(),
}));

vi.mock('@/lib/conversation-runtime-workflow', () => ({
  canViewConversationRuntimeWorkflow: () => false,
  conversationSessionLeafForGraphNode: () => null,
}));

vi.mock('@/lib/conversation-navigation', () => ({
  conversationPageForSession: () => null,
}));

vi.mock('@/lib/conversation-run-snapshot', () => ({
  findConversationLeafByKey: () => null,
}));

vi.mock('@/lib/acp-runtime-error', () => ({
  acpRuntimeErrorBannerCopy: () => null,
}));

vi.mock('@/lib/conversation-run-cache', () => ({
  conversationRunCacheKey: (run: { projectId: string; taskUuid?: string; runId: string }) =>
    run.taskUuid?.trim() ? JSON.stringify([run.projectId, run.taskUuid, run.runId]) : null,
}));

vi.mock('@/lib/conversation-session-follow', () => ({
  isRuntimeControlledConversationLifecycle: () => false,
  isRuntimeTerminalConversationLifecycle: () => false,
  isTerminalConversationSessionStatus: () => false,
}));

vi.mock('@/routes', () => ({
  pathFromRoute: () => '/conversation',
  taskListPage: {},
}));

import { ConversationRunPage } from '@/pages/ConversationRunPage';
import type { ConversationRunVm, ConversationSessionLeafVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function runSnapshot(sequence: number) {
  return {
    projectId: 'project-1',
    taskId: 'task-1',
    taskUuid: 'uuid-1',
    runId: 'run-1',
    runMode: 'direct' as const,
    runStatus: 'running',
    workflowValid: true,
    workflowGraph: { nodes: [], edges: [] },
    activeSessions: [],
    sessionTree: { rounds: [], selectedSessionKey: null },
    runtimeError: null,
    runtimeErrorMessage: null,
    pauseReason: null,
    sequence,
  } as never;
}

describe('ConversationRunPage workspace renderer lifecycle', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.clearAllMocks();
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
  });

  it.each(['sealed-source', 'inherited-attempt'] as const)('renders %s as read-only history', async (mode) => {
    const display = { code: 'failure', tone: 'danger', icon: 'x', terminal: true, resumable: false, blockingError: false } as const;
    const leaf: ConversationSessionLeafVm = {
      roundId: 'round-1', nodeId: 'dev', attemptId: 'attempt-1', pathLabel: 'round-1/dev/attempt-1',
      status: 'completed', outcome: 'failure', current: true, manualCheckPending: true,
      inherited: mode === 'inherited-attempt', sessionId: 'session-1', sessionEstablished: true,
      workLocation: { kind: 'main' }, artifactCount: 0, attachmentCount: 0, runtimeDisplay: display,
    };
    const run: ConversationRunVm = {
      ...(runSnapshot(1) as unknown as ConversationRunVm),
      runMode: 'workflow', runStatus: 'completed', sealed: mode === 'sealed-source',
      sessionTree: {
        selectedSessionKey: leaf.pathLabel,
        rounds: [{ roundId: 'round-1', index: 1, label: 'Round', status: 'completed', runtimeDisplay: display, nodes: [{
          nodeId: 'dev', label: 'Dev', nodeType: 'worker', status: 'completed', runtimeDisplay: display, attempts: [leaf],
        }] }],
      },
    };
    await act(async () => root.render(<ConversationRunPage
      run={run} taskTitle="Task" appConfig={{
        acpChatEventPageSize: 20, acpChatEventWindowPageCount: 3, conversationInlineContentMaxBytes: 1000,
        turnFiles: { cardPreviewLimit: 3, attachmentCardPreviewLimit: 3 },
      } as never} agentRegistry={null}
      onRerun={vi.fn()} onEditWorkflow={vi.fn()} onSelectSession={vi.fn()} followMode="manual"
      initialSessionTreeExpansion={{}} onSessionTreeExpansionChange={vi.fn()}
    />));
    expect(container.querySelector('textarea[aria-label="composer"]')).toBeNull();
    if (mode === 'inherited-attempt') {
      expect(container.querySelector('[data-testid="acp-chat"]')).toBeNull();
      expect(container.textContent).toContain('conversation.runtime.rerunHistoryHint');
    }
    expect(container.textContent).not.toContain('manual-check');
    expect(container.textContent).not.toContain('branch');
    if (mode === 'sealed-source') expect(container.textContent).toContain('conversation.runtime.sourceSealed');
  });

  it.each(['runtime-controlled', 'non-runtime-controlled'] as const)(
    'projects a Workflow pause error by current control ownership: %s',
    async (mode) => {
      const display = { code: 'running', tone: 'running', icon: 'dot', terminal: false, resumable: false, blockingError: false };
      const leaf: ConversationSessionLeafVm = {
        roundId: 'round-1', nodeId: 'dev', attemptId: 'attempt-1', pathLabel: 'round-1/dev/attempt-1',
        status: 'running', current: true, manualCheckPending: false, sessionId: 'session-1', sessionEstablished: true,
        workLocation: { kind: 'main' }, artifactCount: 0, attachmentCount: 0, runtimeDisplay: display,
        lifecycle: {
          runtime: { status: 'paused', pauseReason: 'runtime-abnormal', resumable: true, current: true, active: false, continuable: true, phase: 'paused' },
          control: { mode },
          acp: { sessionAvailability: 'established', liveTurnActivity: 'running', latestTurnStatus: 'none', stopping: false },
          displayStatus: 'running', runtimeDisplay: display,
          composer: { mode: 'normal', submitTarget: 'acp-prompt', canStop: true, lockInput: false },
        },
      };
      const run: ConversationRunVm = {
        ...(runSnapshot(1) as unknown as ConversationRunVm),
        runMode: 'workflow', runStatus: 'paused', pauseReason: 'runtime-abnormal', runtimeErrorMessage: 'Old run failure',
        sessionTree: {
          selectedSessionKey: leaf.pathLabel,
          rounds: [{ roundId: 'round-1', index: 1, label: 'Round', status: 'paused', runtimeDisplay: display, nodes: [{
            nodeId: 'dev', label: 'Dev', nodeType: 'ai', status: 'paused', runtimeDisplay: display, attempts: [leaf],
          }] }],
        },
      };
      await act(async () => root.render(<ConversationRunPage
        run={run} taskTitle="Task" appConfig={{
          acpChatEventPageSize: 20, acpChatEventWindowPageCount: 3, conversationInlineContentMaxBytes: 1000,
          turnFiles: { cardPreviewLimit: 3, attachmentCardPreviewLimit: 3 },
        } as never} agentRegistry={null}
        onRerun={vi.fn()} onEditWorkflow={vi.fn()} onSelectSession={vi.fn()} followMode="manual"
        initialSessionTreeExpansion={{}} onSessionTreeExpansionChange={vi.fn()}
      />));
      const chat = container.querySelector('[data-testid="acp-chat"]');
      expect(chat).not.toBeNull();
      expect(chat?.getAttribute('data-runtime-error')).toBe(mode === 'runtime-controlled' ? 'Old run failure' : '');
      expect(chat?.getAttribute('data-runtime-fallback')).toBe(mode === 'non-runtime-controlled' ? 'Old run failure' : '');
    },
  );

  it('keeps the registered workflow renderer identity across live run updates', async () => {
    const props = {
      taskTitle: 'Task',
      appConfig: {
        acpChatEventPageSize: 20,
        acpChatEventWindowPageCount: 3,
        conversationInlineContentMaxBytes: 1000,
        turnFiles: { cardPreviewLimit: 3, attachmentCardPreviewLimit: 3 },
      },
      agentRegistry: null,
      onRerun: vi.fn(),
      onEditWorkflow: vi.fn(),
      onSelectSession: vi.fn(),
      followMode: 'auto' as const,
      initialSessionTreeExpansion: {},
      onSessionTreeExpansionChange: vi.fn(),
    };

    await act(async () => {
      root.render(<ConversationRunPage {...props} run={runSnapshot(1)} />);
    });
    const initialRegistrations = [...workspace.registerResourceRenderer.mock.calls];
    expect(initialRegistrations.length).toBeGreaterThan(0);
    const initialDisposers = workspace.registerResourceRenderer.mock.results.map((result) => result.value);
    await act(async () => {
      root.render(<ConversationRunPage {...props} run={runSnapshot(2)} />);
    });

    expect(workspace.registerResourceRenderer.mock.calls).toEqual(initialRegistrations);
    initialDisposers.forEach((dispose) => expect(dispose).not.toHaveBeenCalled());
  });

  it.each([
    { label: 'with taskUuid', taskUuid: 'uuid-1' as string | undefined },
    { label: 'without taskUuid', taskUuid: undefined },
  ])('discards a pending node rerun confirmation when the run changes $label', async ({ taskUuid }) => {
    const onNodeRetry = vi.fn(() => new Promise<void>(() => {}));
    const props = {
      taskTitle: 'Task',
      appConfig: {
        acpChatEventPageSize: 20,
        acpChatEventWindowPageCount: 3,
        conversationInlineContentMaxBytes: 1000,
        turnFiles: { cardPreviewLimit: 3, attachmentPreviewLimit: 3 },
      },
      agentRegistry: null,
      onRerun: vi.fn(),
      onEditWorkflow: vi.fn(),
      onSelectSession: vi.fn(),
      followMode: 'auto' as const,
      initialSessionTreeExpansion: {},
      onSessionTreeExpansionChange: vi.fn(),
      onNodeRetry,
    };
    const panelContainer = document.createElement('div');
    document.body.appendChild(panelContainer);
    const panelRoot = createRoot(panelContainer);
    const originalRegister = workspace.registerResourceRenderer.getMockImplementation();
    workspace.registerResourceRenderer.mockImplementation((
      kind: string,
      renderer: (resource: unknown) => ReactNode,
    ) => {
      if (kind === 'workflow-view') {
        panelRoot.render(renderer({ kind, key: 'verify' }));
      }
      return vi.fn();
    });

    try {
      const run = {
        ...(runSnapshot(1) as unknown as ConversationRunVm),
        taskUuid,
      };
      await act(async () => {
        root.render(<ConversationRunPage {...props} run={run} />);
      });
      await act(async () => {
        panelContainer
          .querySelector<HTMLButtonElement>('[data-testid="node-retry-trigger"]')
          ?.click();
      });
      expect(container.textContent).toContain('conversation.runtime.nodeRerunConfirmTitle');

      const otherRun = { ...run, runId: 'run-2' } as ConversationRunVm;
      await act(async () => {
        root.render(<ConversationRunPage {...props} run={otherRun} />);
      });
      expect(container.textContent).not.toContain(
        'conversation.runtime.nodeRerunConfirmTitle',
      );

      await act(async () => {
        root.render(<ConversationRunPage {...props} run={run} />);
      });
      expect(container.textContent).not.toContain(
        'conversation.runtime.nodeRerunConfirmTitle',
      );
      expect(onNodeRetry).not.toHaveBeenCalled();
    } finally {
      workspace.registerResourceRenderer.mockImplementation(originalRegister);
      await act(async () => panelRoot.unmount());
      panelContainer.remove();
    }
  });

  it('refreshes an open workflow resource when the graph projection changes', async () => {
    const graph = { nodes: [], edges: [] };
    const first = { ...runSnapshot(1), workflowGraph: graph } as ConversationRunVm;
    const second = {
      ...first,
      workflowGraph: { ...graph, nodes: [{ id: 'dev', nodeId: 'dev', status: 'running' }] },
    } as ConversationRunVm;

    await act(async () => root.render(<ConversationRunPage
      {...({
        taskTitle: 'Task',
        appConfig: { turnFiles: { cardPreviewLimit: 3, attachmentCardPreviewLimit: 3 } },
        agentRegistry: null,
        onRerun: vi.fn(), onEditWorkflow: vi.fn(), onSelectSession: vi.fn(), followMode: 'auto',
        initialSessionTreeExpansion: {}, onSessionTreeExpansionChange: vi.fn(),
      } as never)}
      run={first}
    />));
    const initialRefreshes = workspace.refreshResourceRenderers.mock.calls.length;

    await act(async () => root.render(<ConversationRunPage
      {...({
        taskTitle: 'Task',
        appConfig: { turnFiles: { cardPreviewLimit: 3, attachmentCardPreviewLimit: 3 } },
        agentRegistry: null,
        onRerun: vi.fn(), onEditWorkflow: vi.fn(), onSelectSession: vi.fn(), followMode: 'auto',
        initialSessionTreeExpansion: {}, onSessionTreeExpansionChange: vi.fn(),
      } as never)}
      run={second}
    />));

    expect(workspace.refreshResourceRenderers.mock.calls.length).toBeGreaterThan(initialRefreshes);
  });

  it('opens AUTO configuration as a right-workspace tab on the current run', async () => {
    const props = {
      taskTitle: 'Task',
      appConfig: {
        acpChatEventPageSize: 20,
        acpChatEventWindowPageCount: 3,
        conversationInlineContentMaxBytes: 1000,
        turnFiles: { cardPreviewLimit: 3, attachmentCardPreviewLimit: 3 },
      },
      agentRegistry: null,
      onRerun: vi.fn(),
      onEditWorkflow: vi.fn(),
      onSelectSession: vi.fn(),
      followMode: 'auto' as const,
      initialSessionTreeExpansion: {},
      onSessionTreeExpansionChange: vi.fn(),
    };
    const run = runSnapshot(1);
    run.runMode = 'auto';

    await act(async () => {
      root.render(<ConversationRunPage {...props} run={run} />);
    });
    await act(async () => {
      container.querySelector('button')?.click();
    });

    expect(workspace.openResource).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'auto-config',
      key: 'auto-config:project-1:uuid-1:run-1',
      title: 'executionPlan.configureAuto',
      locator: expect.objectContaining({
        projectId: 'project-1',
        taskId: 'task-1',
        taskUuid: 'uuid-1',
        runId: 'run-1',
      }),
    }));
  });
});
