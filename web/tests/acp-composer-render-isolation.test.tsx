/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const { getAgentCommandCatalog, mergeAcpEventWindowsCounter, streamdownRender } = vi.hoisted(() => ({
  getAgentCommandCatalog: vi.fn(),
  mergeAcpEventWindowsCounter: vi.fn(),
  streamdownRender: vi.fn(),
}));

vi.mock('streamdown', () => ({
  defaultUrlTransform: (url: string) => url,
  parseMarkdownIntoBlocks: (markdown: string) => [markdown],
  Streamdown: ({ children }: { children: React.ReactNode }) => {
    streamdownRender();
    return <div>{children}</div>;
  },
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return {
    ...actual,
    getAcpSession: vi.fn().mockResolvedValue(null),
    getAgentCommandCatalog,
  };
});

vi.mock('@/lib/acp-event-reducer', async () => {
  const actual = await vi.importActual<typeof import('@/lib/acp-event-reducer')>(
    '@/lib/acp-event-reducer',
  );
  return {
    ...actual,
    mergeAcpEventWindows: (
      ...args: Parameters<typeof actual.mergeAcpEventWindows>
    ) => {
      mergeAcpEventWindowsCounter();
      return actual.mergeAcpEventWindows(...args);
    },
  };
});

import {
  ACPChatDialog,
  createAcpLoadedEventWindow,
  createAcpEventWindowCacheKey,
  resetAcpResourceCache,
  restoreAcpLoadedEventWindow,
  storeAcpLoadedEventWindow,
} from '@/components/acp/ACPChatDialog';
import { GitBranchPickerSnapshotProvider } from '@/components/git/GitBranchPickerSnapshotContext';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpSessionVm, ConversationAttemptLifecycleVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function completedSession(): AcpSessionVm {
  return {
    branchId: 'root',
    parentBranchId: null,
    readOnly: false,
    branchExecution: null,
    sessionId: 'composer-render-session',
    title: 'Composer render isolation',
    roundId: 'round-render',
    nodeId: 'node-render',
    attemptId: 'attempt-render',
    provider: 'test',
    status: 'completed',
    restored: false,
    events: [{
      id: 'assistant-message',
      seq: 1,
      timestamp: '1Z',
      kind: 'textDelta',
      sessionId: 'composer-render-session',
      content: 'historical **Markdown** message',
      title: null,
      toolCallId: null,
      status: 'completed',
      startedSeq: 1,
      endedSeq: 1,
      raw: {},
    }],
    eventPage: {
      loadedCount: 1,
      total: 1,
      oldestSeq: 1,
      newestSeq: 1,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: null,
      newestCursor: null,
    },
    timelineProjection: { agents: [], todoEntries: [] },
    pendingInteractions: [],
    diagnostics: { rawFrameCount: 0, eventCount: 1, errorCount: 0 },
  };
}

function nonRuntimeControlledLifecycle(): ConversationAttemptLifecycleVm {
  return {
    runtime: {
      status: 'completed',
      outcome: 'success',
      pauseReason: null,
      resumable: false,
      current: true,
      active: false,
      continuable: false,
      phase: 'terminal',
    },
    control: { mode: 'non-runtime-controlled' },
    acp: {
      sessionAvailability: 'established',
      liveTurnActivity: 'idle',
      latestTurnStatus: 'completed',
      stopping: false,
    },
    displayStatus: 'completed',
    runtimeDisplay: {
      code: 'completed',
      tone: 'success',
      icon: 'check',
      terminal: true,
      resumable: false,
      reasonCode: null,
      blockingError: false,
    },
    continueKind: null,
    composer: {
      mode: 'normal',
      submitTarget: 'acp-prompt',
      processingKind: 'processing',
      statusKey: null,
      canStop: false,
      lockInput: false,
    },
  };
}

beforeEach(() => {
  resetAcpResourceCache();
  getAgentCommandCatalog.mockResolvedValue({
    agentType: 'test',
    projectId: 'worktree-project',
    skillCommands: [{ name: 'worktree-skill', description: 'Scanned from the worktree' }],
    commands: [
      { name: 'stale-native', description: 'Cached from an older session update' },
      { name: 'worktree-skill', description: 'Scanned from the worktree' },
    ],
    updatedAt: '2026-08-25T00:00:00Z',
  });
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
    configurable: true,
    value: vi.fn(),
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
    window.setTimeout(() => callback(performance.now()), 0)
  ));
  vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
});

afterEach(() => {
  resetAcpResourceCache();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  streamdownRender.mockClear();
  Reflect.deleteProperty(HTMLElement.prototype, 'scrollIntoView');
  document.body.replaceChildren();
});

describe('ACP composer render isolation', () => {
  it('does not restore and merge the historical window on an unrelated parent render', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const session = completedSession();
    const eventWindowKey = createAcpEventWindowCacheKey({
      projectId: 'project-render',
      taskId: 'task-render',
      runId: 'run-render',
      roundId: 'round-render',
      nodeId: 'node-render',
      attemptId: 'attempt-render',
    });
    storeAcpLoadedEventWindow(
      eventWindowKey,
      createAcpLoadedEventWindow(session),
      288,
    );
    const view = (marker: string) => (
      <div data-marker={marker}>
        <TooltipProvider>
          <ACPChatDialog
            session={session}
            projectId="project-render"
            taskId="task-render"
            runId="run-render"
            roundId="round-render"
            nodeId="node-render"
            attemptId="attempt-render"
            showSystemPromptAction={false}
            showRawFramesAction={false}
            usageCompact
          />
        </TooltipProvider>
      </div>
    );

    try {
      await act(async () => root.render(view('initial')));
      expect(mergeAcpEventWindowsCounter).toHaveBeenCalled();
      mergeAcpEventWindowsCounter.mockClear();

      await act(async () => root.render(view('parent-update')));

      expect(container.querySelector('[data-marker]')?.getAttribute('data-marker'))
        .toBe('parent-update');
      expect(mergeAcpEventWindowsCounter).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('never writes the previous event window under a newly selected eventWindowKey', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const sessionA = completedSession();
    const sessionB: AcpSessionVm = {
      ...completedSession(),
      sessionId: 'composer-render-session-b',
      nodeId: 'node-render-b',
      attemptId: 'attempt-render-b',
      events: [{
        ...completedSession().events[0]!,
        id: 'assistant-message-b',
        sessionId: 'composer-render-session-b',
        content: 'session B historical Markdown',
      }],
    };
    const observations: Array<{
      key: string;
      sessionId: string | null;
      eventIds: string[];
    }> = [];

    function Harness({ selected }: { selected: 'a' | 'b' }) {
      const selectedSession = selected === 'a' ? sessionA : sessionB;
      const selectedNodeId = selected === 'a' ? 'node-render' : 'node-render-b';
      const selectedAttemptId = selected === 'a' ? 'attempt-render' : 'attempt-render-b';
      const eventWindowKey = createAcpEventWindowCacheKey({
        projectId: 'project-render',
        taskId: 'task-render',
        runId: 'run-render',
        roundId: 'round-render',
        nodeId: selectedNodeId,
        attemptId: selectedAttemptId,
      });
      React.useEffect(() => {
        const window = restoreAcpLoadedEventWindow(eventWindowKey, null, 288);
        observations.push({
          key: eventWindowKey,
          sessionId: window.sessionId,
          eventIds: window.events.map((event) => event.id),
        });
      }, [eventWindowKey]);
      return (
        <TooltipProvider>
          <ACPChatDialog
            session={selectedSession}
            projectId="project-render"
            taskId="task-render"
            runId="run-render"
            roundId="round-render"
            nodeId={selectedNodeId}
            attemptId={selectedAttemptId}
            showSystemPromptAction={false}
            showRawFramesAction={false}
            usageCompact
          />
        </TooltipProvider>
      );
    }

    try {
      await act(async () => root.render(<Harness selected="a" />));
      await act(async () => root.render(<Harness selected="b" />));

      const keyB = createAcpEventWindowCacheKey({
        projectId: 'project-render',
        taskId: 'task-render',
        runId: 'run-render',
        roundId: 'round-render',
        nodeId: 'node-render-b',
        attemptId: 'attempt-render-b',
      });
      expect(observations.find((observation) => observation.key === keyB)).toEqual({
        key: keyB,
        sessionId: 'composer-render-session-b',
        eventIds: ['assistant-message-b'],
      });
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('joins a branch-only session info tab to the composer surface', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <GitBranchPickerSnapshotProvider>
            <TooltipProvider>
              <ACPChatDialog
                session={completedSession()}
                projectId="project-render"
                taskId="task-render"
                runId="run-render"
                roundId="round-render"
                nodeId="node-render"
                attemptId="attempt-render"
                showBranchControl
                showSystemPromptAction={false}
                showRawFramesAction={false}
                usageCompact
              />
            </TooltipProvider>
          </GitBranchPickerSnapshotProvider>,
        );
      });

      expect(container.querySelector('[data-acp-session-info-item="branch"]')).not.toBeNull();
      const composerRail = container.querySelector('[data-acp-conversation-rail="composer"]');
      expect(composerRail?.classList.contains(
        '[--acp-composer-rail-shadow:var(--gb-material-shadow)]',
      )).toBe(true);
      expect(composerRail?.classList.contains(
        'dark:[--acp-composer-rail-shadow:var(--gb-elevation-overlay)]',
      )).toBe(true);
      expect(composerRail?.classList.contains(
        '[filter:drop-shadow(var(--acp-composer-rail-shadow))]',
      )).toBe(true);
      expect(composerRail?.className.match(/drop-shadow\(/gu) ?? []).toHaveLength(1);
      expect(composerRail?.className).not.toContain('--gb-material-edge-shadow');
      const promptInput = container.querySelector('[data-slot="prompt-input"]');
      expect(promptInput?.classList.contains('rounded-tl-none')).toBe(true);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps the conversation shell mounted when an established session payload is temporarily absent', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={null}
              sessionEstablished
              sessionReferenceId="persisted-session"
              projectId="project-render"
              taskId="task-render"
              runId="run-render"
              roundId="round-render"
              nodeId="node-render"
              attemptId="attempt-render"
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
      });

      expect(container.querySelector('textarea')).not.toBeNull();
      expect(container.textContent).not.toContain('ACP session failed');
      expect(container.textContent).not.toContain('ACP 会话失败');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps historical Markdown stable and measures textarea height once per input update', async () => {
    const scrollHeight = vi.spyOn(HTMLTextAreaElement.prototype, 'scrollHeight', 'get').mockReturnValue(72);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={completedSession()}
              projectId="project-render"
              taskId="task-render"
              runId="run-render"
              roundId="round-render"
              nodeId="node-render"
              attemptId="attempt-render"
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
      });
      const initialMarkdownRenders = streamdownRender.mock.calls.length;
      expect(initialMarkdownRenders).toBe(1);
      scrollHeight.mockClear();

      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      expect(textarea).not.toBeNull();
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      await act(async () => {
        valueSetter?.call(textarea, 'a');
        textarea?.dispatchEvent(new Event('input', { bubbles: true }));
      });

      expect(textarea?.value).toBe('a');
      expect(streamdownRender).toHaveBeenCalledTimes(initialMarkdownRenders);
      expect(scrollHeight).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('opens the slash menu with session commands and scanned Skills in a non-runtime-controlled worktree', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const session = {
      ...completedSession(),
      providerCwd: 'D:\\repo\\.gold-band\\worktrees\\run-1',
      availableCommands: [
        { name: 'native-status', description: 'Current ACP session command' },
      ],
    };
    try {
      await act(async () => {
        root.render(
          <TooltipProvider>
            <ACPChatDialog
              session={session}
              projectId="project-render"
              taskId="task-render"
              runId="run-render"
              roundId="round-render"
              nodeId="node-render"
              attemptId="attempt-render"
              runtimeComposerContext={{
                isOrchestrated: false,
                lifecycle: nonRuntimeControlledLifecycle(),
                workflowValid: true,
              }}
              showSystemPromptAction={false}
              showRawFramesAction={false}
              usageCompact
            />
          </TooltipProvider>,
        );
        await Promise.resolve();
      });

      expect(getAgentCommandCatalog).toHaveBeenCalledWith('test', session.providerCwd);
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
      await act(async () => {
        valueSetter?.call(textarea, '/');
        textarea?.dispatchEvent(new Event('input', { bubbles: true }));
      });

      const menu = document.querySelector('[data-slot="slash-command-menu"]');
      expect(menu).not.toBeNull();
      expect(menu?.textContent).toContain('/native-status');
      expect(menu?.textContent).toContain('/worktree-skill');
      expect(menu?.textContent).not.toContain('/stale-native');
      expect(menu?.classList.contains('bg-popover')).toBe(true);
      expect(menu?.className).not.toMatch(/bg-popover\/[0-9]/);
      expect(menu?.className).not.toContain('backdrop-blur');
    } finally {
      await act(async () => root.unmount());
    }
  });
});
