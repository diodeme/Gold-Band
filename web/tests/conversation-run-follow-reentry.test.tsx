/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const workspaceMocks = vi.hoisted(() => ({
  registerResourceRenderer: vi.fn(() => vi.fn()),
  registerResourceCloseResolver: vi.fn(() => vi.fn()),
  setConversationDirectoryEntry: vi.fn(),
  openResource: vi.fn(),
}));
const headerMocks = vi.hoisted(() => ({
  render: vi.fn(() => null),
}));
const chatMocks = vi.hoisted(() => ({
  render: vi.fn(() => null),
}));
const manualCheckMocks = vi.hoisted(() => ({ submit: vi.fn() }));
vi.mock('@/api', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/api')>(),
  submitManualCheck: manualCheckMocks.submit,
}));

vi.mock('@/components/acp/ACPChatDialog', () => ({
  ACPChatDialog: chatMocks.render,
  createAcpEventWindowCacheKey: vi.fn(() => 'event-window'),
  hasHydratedAcpSessionContent: vi.fn(() => false),
}));
vi.mock('@/components/conversation/ConversationRunHeader', () => ({
  ConversationRunHeader: headerMocks.render,
}));
vi.mock('@/components/conversation/ConversationSessionSwitcher', () => ({
  ConversationSessionSwitcher: () => null,
}));
vi.mock('@/components/theme/ThemeAssetsContext', () => ({
  useThemeWallpaperSurface: vi.fn(),
}));
vi.mock('@/components/workspace/ConversationRunWorkspaceResourcePanel', () => ({
  ConversationRunWorkspaceResourcePanel: () => null,
  confirmCloseConversationRunWorkspaceResource: vi.fn(() => true),
}));
vi.mock('@/components/workspace/right-workspace-context', () => ({
  conversationRunWorkspaceResourceKey: vi.fn(() => 'resource-key'),
  useRightWorkspace: () => ({
    scopeKey: null,
    registerResourceRenderer: workspaceMocks.registerResourceRenderer,
    registerResourceCloseResolver: workspaceMocks.registerResourceCloseResolver,
    setConversationDirectoryEntry: workspaceMocks.setConversationDirectoryEntry,
    openResource: workspaceMocks.openResource,
  }),
}));

import { ConversationRunPage } from '@/pages/ConversationRunPage';
import type { AgentRegistryVm, AppConfigVm, ConversationRunVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.innerHTML = '';
  vi.clearAllMocks();
});

describe('ConversationRunPage follow mode reentry', () => {
  it.each(['no-successor', 'rejected', 'changed-session', 'changed-run', 'unmounted', 'live-refresh'])(
    'settles manual-check navigation safely for %s', async (scenario) => {
      const container = document.createElement('div');
      document.body.append(container);
      const root = createRoot(container);
      const onSelectSession = vi.fn();
      let resolve!: (value: unknown) => void;
      let reject!: (error: unknown) => void;
      manualCheckMocks.submit.mockReturnValue(new Promise((yes, no) => { resolve = yes; reject = no; }));
      const run = manualCheckRun();
      const render = (value: ConversationRunVm) => root.render(
        <ConversationRunPage
          run={value} taskTitle="Manual check"
          appConfig={{ turnFiles: { cardPreviewLimit: 10, attachmentCardPreviewLimit: 1 } } as AppConfigVm}
          agentRegistry={null} followMode="manual" onRerun={vi.fn()} onEditWorkflow={vi.fn()}
          onSelectSession={onSelectSession} onAutoFollowChange={vi.fn()}
          initialSessionTreeExpansion={{}} onSessionTreeExpansionChange={vi.fn()}
        />,
      );
      await act(async () => render(run));
      const submit = chatMocks.render.mock.lastCall?.[0].onSubmitManualCheck;
      const result = submit('success').catch((error: unknown) => error);
      if (scenario === 'unmounted') await act(async () => root.unmount());
      if (scenario === 'changed-session') {
        await act(async () => render({ ...run, sessionTree: { ...run.sessionTree, selectedSessionKey: 'history' } }));
      }
      if (scenario === 'changed-run') await act(async () => render({ ...run, runId: 'other-run' }));
      if (scenario === 'live-refresh') await act(async () => render(structuredClone(run)));
      const error = { code: 'manual-check.rejected' };
      await act(async () => {
        if (scenario === 'rejected') reject(error);
        else resolve({
          taskId: run.taskId, id: run.runId, status: scenario === 'no-successor' ? 'completed' : 'running',
          currentRound: 'round-001', currentNode: scenario === 'no-successor' ? 'check-worker' : 'next-worker',
          currentAttempt: 'attempt-001',
        });
        await result;
      });
      if (scenario === 'live-refresh') expect(onSelectSession).toHaveBeenCalledTimes(1);
      else expect(onSelectSession).not.toHaveBeenCalled();
      if (scenario === 'rejected') expect(await result).toEqual(error);
      if (scenario !== 'unmounted') await act(async () => root.unmount());
    },
  );

  it.each(['success', 'failure'] as const)('navigates to the manual-check successor after %s even away from bottom', async (outcome) => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSelectSession = vi.fn();
    const run = manualCheckRun();
    manualCheckMocks.submit.mockResolvedValue({
      taskId: run.taskId, id: run.runId, status: 'running',
      currentRound: 'round-001', currentNode: 'next-worker', currentAttempt: 'attempt-001',
    });
    await act(async () => root.render(
      <ConversationRunPage
        run={run} taskTitle="Manual check" appConfig={{ turnFiles: { cardPreviewLimit: 10, attachmentCardPreviewLimit: 1 } } as AppConfigVm}
        agentRegistry={null} followMode="manual" onRerun={vi.fn()} onEditWorkflow={vi.fn()}
        onSelectSession={onSelectSession} onAutoFollowChange={vi.fn()}
        initialSessionTreeExpansion={{}} onSessionTreeExpansionChange={vi.fn()}
      />,
    ));
    try {
      const props = chatMocks.render.mock.lastCall?.[0];
      await act(async () => props.onAtBottomChange(false));
      expect(props.onSubmitManualCheck).toBeTypeOf('function');
      await act(async () => props.onSubmitManualCheck(outcome));
      expect(manualCheckMocks.submit).toHaveBeenCalledWith(
        run.projectId, run.taskId, run.runId, 'round-001', 'check-worker', 'attempt-001', outcome,
      );
      expect(onSelectSession).toHaveBeenCalledExactlyOnceWith({
        roundId: 'round-001', nodeId: 'next-worker', attemptId: 'attempt-001',
      }, true);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not re-enable auto-follow when a manually browsed run remounts', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onAutoFollowChange = vi.fn();
    const run = {
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
      runStatus: 'completed',
      runMode: 'workflow',
      activeSessions: [],
      selectedSession: null,
      sessionTree: { rounds: [], selectedSessionKey: null },
      workflowGraph: { nodes: [], edges: [] },
    } as unknown as ConversationRunVm;

    await act(async () => {
      root.render(
        <ConversationRunPage
          run={run}
          taskTitle="Run 1"
          appConfig={{} as AppConfigVm}
          agentRegistry={null as AgentRegistryVm | null}
          followMode="manual"
          onRerun={vi.fn()}
          onEditWorkflow={vi.fn()}
          onSelectSession={vi.fn()}
          onAutoFollowChange={onAutoFollowChange}
          initialSessionTreeExpansion={{}}
          onSessionTreeExpansionChange={vi.fn()}
        />,
      );
    });

    expect(onAutoFollowChange).not.toHaveBeenCalled();
    expect(headerMocks.render).toHaveBeenCalledWith(
      expect.objectContaining({ taskTitle: 'Run 1' }),
      undefined,
    );

    await act(async () => root.unmount());
  });

  it('passes the selected dynamic worktree and registry without requiring a parent session payload', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const selectedKey = 'round-001/ai-dynamic/attempt-001/goodbye-worker/attempt-001';
    const registry: AgentRegistryVm = { agents: [], catalog: [] };
    const leaf = {
      roundId: 'round-001',
      nodeId: 'goodbye-worker',
      attemptId: 'attempt-001',
      outerNodeId: 'ai-dynamic',
      outerAttemptId: 'attempt-001',
      pathLabel: selectedKey,
      status: 'completed',
      current: false,
      manualCheckPending: false,
      sessionEstablished: true,
      worktreePath: 'D:/repo/.gold-band/worktrees/child',
      worktreeBranch: 'gb-dynamic-child',
      artifactCount: 0,
      attachmentCount: 0,
    };
    const run = {
      projectId: 'project-1',
      taskId: 'task-028',
      runId: 'run-001',
      runStatus: 'completed',
      runMode: 'auto',
      activeSessions: [],
      inputAttachments: [],
      selectedSession: null,
      sessionTree: {
        selectedSessionKey: selectedKey,
        rounds: [{
          roundId: 'round-001',
          index: 1,
          label: 'Round 1',
          status: 'completed',
          nodes: [{
            nodeId: 'ai-dynamic',
            label: 'AUTO',
            nodeType: 'ai-dynamic',
            status: 'completed',
            attempts: [],
            outerNodes: [{
              nodeId: 'goodbye-worker',
              label: 'Goodbye worker',
              nodeType: 'worker',
              status: 'completed',
              attempts: [leaf],
            }],
          }],
        }],
      },
      workflowStatus: 'valid',
      workflowValid: true,
      workflowGraph: { nodes: [], edges: [] },
      resumable: false,
      worktree: { path: 'C:/GoldBand/worktrees/outer', branch: 'outer', forkCommit: 'abc123' },
    } as unknown as ConversationRunVm;

    await act(async () => {
      root.render(
        <ConversationRunPage
          run={run}
          taskTitle="AUTO run"
          appConfig={{ turnFiles: { cardPreviewLimit: 10, attachmentCardPreviewLimit: 1 } } as AppConfigVm}
          agentRegistry={registry}
          followMode="manual"
          onRerun={vi.fn()}
          onEditWorkflow={vi.fn()}
          onSelectSession={vi.fn()}
          onAutoFollowChange={vi.fn()}
          initialSessionTreeExpansion={{}}
          onSessionTreeExpansionChange={vi.fn()}
        />,
      );
    });

    expect(chatMocks.render).toHaveBeenCalledWith(
      expect.objectContaining({
        worktreePath: 'D:/repo/.gold-band/worktrees/child',
        managedWorktreeBranch: 'gb-dynamic-child',
        session: null,
        agentRegistry: registry,
      }),
      undefined,
    );

    await act(async () => root.unmount());
  });

  it('does not forward a superseded Direct runtime error after a follow-up turn completes', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const selectedKey = 'round-001/direct-agent/attempt-001';
    const lifecycle = {
      runtime: {
        status: 'paused',
        outcome: null,
        pauseReason: 'runtime-abnormal',
        resumable: true,
        current: true,
        active: false,
        continuable: false,
        phase: 'idle',
        revision: 4,
      },
      control: { mode: 'non-runtime-controlled' },
      acp: {
        revision: 33,
        turnId: 'follow-up-turn',
        sessionAvailability: 'established',
        liveTurnActivity: 'idle',
        latestTurnStatus: 'completed',
        stopping: false,
        stopReason: 'end_turn',
      },
      displayStatus: 'paused',
      runtimeDisplay: {
        code: 'runtime-abnormal',
        tone: 'danger',
        terminal: false,
        resumable: true,
        reasonCode: 'runtime-abnormal',
        blockingError: false,
      },
      composer: {
        mode: 'normal',
        submitTarget: 'acp-prompt',
        processingKind: 'processing',
        canStop: false,
        lockInput: false,
      },
    };
    const leaf = {
      roundId: 'round-001',
      nodeId: 'direct-agent',
      attemptId: 'attempt-001',
      pathLabel: selectedKey,
      status: 'paused',
      current: true,
      manualCheckPending: false,
      sessionEstablished: true,
      artifactCount: 0,
      attachmentCount: 0,
      runtimeDisplay: lifecycle.runtimeDisplay,
      lifecycle,
    };
    const run = {
      projectId: 'project-1',
      taskId: 'task-333',
      runId: 'run-001',
      runStatus: 'paused',
      runMode: 'direct',
      pauseReason: 'runtime-abnormal',
      runtimeErrorMessage: 'old provider failure',
      activeSessions: [],
      inputAttachments: [],
      selectedSession: null,
      sessionTree: {
        selectedSessionKey: selectedKey,
        rounds: [{
          roundId: 'round-001',
          index: 1,
          label: 'Round 1',
          status: 'paused',
          nodes: [{
            nodeId: 'direct-agent',
            label: 'Direct Agent',
            nodeType: 'worker',
            status: 'paused',
            attempts: [leaf],
            outerNodes: [],
          }],
        }],
      },
      workflowStatus: 'valid',
      workflowValid: true,
      workflowGraph: { nodes: [], edges: [] },
      resumable: true,
    } as unknown as ConversationRunVm;

    await act(async () => {
      root.render(
        <ConversationRunPage
          run={run}
          taskTitle="Direct run"
          appConfig={{ turnFiles: { cardPreviewLimit: 10, attachmentCardPreviewLimit: 1 } } as AppConfigVm}
          agentRegistry={null}
          followMode="auto"
          onRerun={vi.fn()}
          onEditWorkflow={vi.fn()}
          onSelectSession={vi.fn()}
          onAutoFollowChange={vi.fn()}
          initialSessionTreeExpansion={{}}
          onSessionTreeExpansionChange={vi.fn()}
        />,
      );
    });

    expect(chatMocks.render).toHaveBeenCalledWith(
      expect.objectContaining({
        runtimeComposerContext: expect.objectContaining({
          lifecycle,
          runtimeError: null,
          runtimeErrorFallback: 'old provider failure',
        }),
      }),
      undefined,
    );

    await act(async () => root.unmount());
  });

  it('restores dynamic session auto-follow after a scroll-only pause returns to bottom', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const changes: boolean[] = [];
    const run = terminalDynamicRun();

    function FollowHarness() {
      const [followMode, setFollowMode] = React.useState<'auto' | 'manual'>('auto');
      return (
        <ConversationRunPage
          run={run}
          taskTitle="AUTO run"
          appConfig={{ turnFiles: { cardPreviewLimit: 10, attachmentCardPreviewLimit: 1 } } as AppConfigVm}
          agentRegistry={null}
          followMode={followMode}
          onRerun={vi.fn()}
          onEditWorkflow={vi.fn()}
          onSelectSession={vi.fn()}
          onAutoFollowChange={(enabled) => {
            changes.push(enabled);
            setFollowMode(enabled ? 'auto' : 'manual');
          }}
          initialSessionTreeExpansion={{}}
          onSessionTreeExpansionChange={vi.fn()}
        />
      );
    }

    await act(async () => root.render(<FollowHarness />));
    const leaveBottom = chatMocks.render.mock.lastCall?.[0].onAtBottomChange as ((atBottom: boolean) => void);
    await act(async () => leaveBottom(false));
    const returnToBottom = chatMocks.render.mock.lastCall?.[0].onAtBottomChange as ((atBottom: boolean) => void);
    await act(async () => returnToBottom(true));

    expect(changes).toEqual([false, true]);
    await act(async () => root.unmount());
  });

  it('does not restore dynamic auto-follow by scrolling a manually selected terminal leaf to bottom', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onAutoFollowChange = vi.fn();

    await act(async () => {
      root.render(
        <ConversationRunPage
          run={terminalDynamicRun()}
          taskTitle="AUTO run"
          appConfig={{ turnFiles: { cardPreviewLimit: 10, attachmentCardPreviewLimit: 1 } } as AppConfigVm}
          agentRegistry={null}
          followMode="manual"
          onRerun={vi.fn()}
          onEditWorkflow={vi.fn()}
          onSelectSession={vi.fn()}
          onAutoFollowChange={onAutoFollowChange}
          initialSessionTreeExpansion={{}}
          onSessionTreeExpansionChange={vi.fn()}
        />,
      );
    });
    const returnToBottom = chatMocks.render.mock.lastCall?.[0].onAtBottomChange as ((atBottom: boolean) => void);
    await act(async () => returnToBottom(true));

    expect(onAutoFollowChange).not.toHaveBeenCalledWith(true);
    await act(async () => root.unmount());
  });
});

function manualCheckRun(): ConversationRunVm {
  const run = terminalDynamicRun();
  const leaf = {
    ...run.sessionTree.rounds[0].nodes[0].outerNodes[0].attempts[0],
    nodeId: 'check-worker', outerNodeId: null, outerAttemptId: null,
    status: 'paused', current: true, manualCheckPending: true,
  };
  return {
    ...run, runMode: 'workflow', runStatus: 'paused',
    sessionTree: {
      selectedSessionKey: 'round-001/check-worker/attempt-001',
      rounds: [{ ...run.sessionTree.rounds[0], nodes: [{
        ...run.sessionTree.rounds[0].nodes[0], nodeId: leaf.nodeId, attempts: [leaf], outerNodes: [],
      }] }],
    },
  };
}

function terminalDynamicRun() {
  const selectedKey = 'round-001/ai-dynamic/attempt-001/bootstrap/attempt-001';
  const leaf = {
    roundId: 'round-001',
    nodeId: 'bootstrap',
    attemptId: 'attempt-001',
    outerNodeId: 'ai-dynamic',
    outerAttemptId: 'attempt-001',
    pathLabel: selectedKey,
    status: 'completed',
    outcome: 'success',
    current: false,
    manualCheckPending: false,
    sessionEstablished: true,
    artifactCount: 0,
    attachmentCount: 0,
    runtimeDisplay: { code: 'success', tone: 'success' },
    lifecycle: {
      runtime: { status: 'completed', active: false },
      control: { mode: 'non-runtime-controlled', transitionCause: 'runtime-terminal' },
      acp: { liveTurnActivity: 'idle', stopping: false },
      composer: { mode: 'normal', supersededBy: null },
    },
  };
  return {
    projectId: 'project-1',
    taskId: 'task-028',
    runId: 'run-001',
    runStatus: 'running',
    runMode: 'auto',
    activeSessions: [],
    inputAttachments: [],
    selectedSession: null,
    sessionTree: {
      selectedSessionKey: selectedKey,
      rounds: [{
        roundId: 'round-001',
        index: 1,
        label: 'Round 1',
        status: 'running',
        nodes: [{
          nodeId: 'ai-dynamic',
          label: 'AUTO',
          nodeType: 'ai-dynamic',
          status: 'running',
          attempts: [],
          outerNodes: [{
            nodeId: 'bootstrap',
            label: 'Bootstrap',
            nodeType: 'worker',
            status: 'completed',
            attempts: [leaf],
          }],
        }],
      }],
    },
    workflowStatus: 'valid',
    workflowValid: true,
    workflowGraph: { nodes: [], edges: [] },
    resumable: false,
  } as unknown as ConversationRunVm;
}
