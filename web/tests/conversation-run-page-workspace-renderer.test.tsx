/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const workspace = vi.hoisted(() => ({
  scopeKey: 'conversation:project-1:task-1:run-1',
  openResource: vi.fn(),
  registerResourceRenderer: vi.fn(() => vi.fn()),
  registerResourceCloseResolver: vi.fn(() => vi.fn()),
  setConversationDirectoryEntry: vi.fn(),
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
  AlertDialog: ({ children }: { children: unknown }) => children,
  AlertDialogAction: ({ children }: { children: unknown }) => children,
  AlertDialogCancel: ({ children }: { children: unknown }) => children,
  AlertDialogContent: ({ children }: { children: unknown }) => children,
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
  ACPChatDialog: () => <div data-testid="acp-chat" />,
  createAcpEventWindowCacheKey: () => 'window',
  hasHydratedAcpSessionContent: () => true,
  shouldShowConversationContentLoadingState: () => false,
}));

vi.mock('@/components/theme/ThemeAssetsContext', () => ({
  useThemeWallpaperSurface: () => undefined,
}));

vi.mock('@/components/workspace/ConversationRunWorkspaceResourcePanel', () => ({
  ConversationRunWorkspaceResourcePanel: () => <div data-testid="workflow-panel" />,
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

vi.mock('@/lib/acp-runtime-composer-state', () => ({
  shouldTreatAcpRuntimeErrorAsFallback: () => false,
}));

vi.mock('@/lib/conversation-run-cache', () => ({
  conversationRunCacheKey: () => 'run-key',
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
    selectedSession: null,
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
    await act(async () => {
      root.render(<ConversationRunPage {...props} run={runSnapshot(2)} />);
    });

    expect(workspace.registerResourceRenderer).toHaveBeenCalledTimes(6);
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
