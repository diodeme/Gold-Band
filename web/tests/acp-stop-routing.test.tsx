/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  stopActiveSession: vi.fn(),
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, ...apiMocks };
});

vi.mock('@/components/prompt-kit/markdown', () => ({
  Markdown: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { ACPChatDialog } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import {
  deriveAcpRuntimeComposerState,
  routesAcpStopToBackgroundControl,
  type AcpRuntimeComposerStateInput,
} from '@/lib/acp-runtime-composer-state';
import { RightWorkspaceProvider } from '@/components/workspace/right-workspace-context';
import type {
  AcpSessionVm,
  ConversationAttemptLifecycleVm,
  DirectBackgroundControl,
} from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function TestProviders({ children }: { children: React.ReactNode }) {
  return React.createElement(
    RightWorkspaceProvider,
    null,
    React.createElement(TooltipProvider, null, children),
  );
}

/**
 * A Direct session whose last turn already ended: the pending question is
 * session-owned (no turnId) and background activity keeps the stop window
 * open. Stop must route to the background control path.
 */
function idleDirectLifecycleWithBackground(
  backgroundControl: DirectBackgroundControl,
): ConversationAttemptLifecycleVm {
  return {
    runtime: {
      status: 'idle',
      outcome: null,
      pauseReason: null,
      resumable: false,
      current: true,
      active: false,
      continuable: false,
      phase: 'idle',
    },
    control: { mode: 'non-runtime-controlled' },
    acp: {
      sessionAvailability: 'established',
      turnId: 'turn-settled',
      liveTurnActivity: 'idle',
      latestTurnStatus: 'completed',
      stopping: false,
    },
    displayStatus: 'idle',
    runtimeDisplay: {
      code: 'idle',
      tone: 'neutral',
      icon: 'dot',
      terminal: false,
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
      backgroundControl,
    },
    promptQueue: {
      revision: 0,
      items: [],
      maxItems: 10,
    },
  };
}

function sessionWithPendingBackgroundQuestion(id: string): AcpSessionVm {
  return {
    branchId: 'root',
    parentBranchId: null,
    readOnly: false,
    branchExecution: null,
    sessionId: id,
    title: 'Background question',
    roundId: 'round-1',
    nodeId: 'node-1',
    attemptId: 'attempt-1',
    provider: 'test',
    status: 'running',
    restored: false,
    events: [],
    eventPage: {
      loadedCount: 0,
      total: 0,
      oldestSeq: null,
      newestSeq: null,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: null,
      newestCursor: null,
    },
    timelineProjection: { agents: [], todoEntries: [] },
    pendingInteractions: [{
      kind: 'elicitation',
      interactionId: 'elicit-background',
      sessionId: id,
      turnId: null,
      promptEventId: null,
      message: '接下来想做什么？',
      toolCallId: 'ask-tool',
      requestedSchema: { type: 'object' },
      raw: {},
    }],
    diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
  };
}

beforeEach(() => {
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  apiMocks.stopActiveSession.mockReset().mockResolvedValue({
    status: 'accepted',
    kind: 'background-cancel-requested',
  });
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => (
    window.setTimeout(() => callback(performance.now()), 0)
  ));
  vi.stubGlobal('cancelAnimationFrame', (frameId: number) => window.clearTimeout(frameId));
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('routesAcpStopToBackgroundControl', () => {
  const backgroundControl: DirectBackgroundControl = {
    sessionId: 'session-pure',
    connectionGeneration: 3,
    activeTools: 1,
    expiresAtMs: Date.now() + 60_000,
  };
  const composerFor = (local: Partial<AcpRuntimeComposerStateInput>) => deriveAcpRuntimeComposerState({
    lifecycle: idleDirectLifecycleWithBackground(backgroundControl),
    promptQueueEnabled: true,
    workflowValid: true,
    prompt: '',
    waitingForUserInteraction: false,
    sending: false,
    awaitingResponse: false,
    waitingForOptimisticPrompt: false,
    localTurnId: null,
    cancelling: false,
    stopCommandPending: false,
    turnAccepted: false,
    hasResponseAfterTurn: false,
    hasTimelineItems: true,
    hasEffectiveEvents: true,
    timelineProcessingKind: 'processing',
    ...local,
  });

  it('routes to the background path when no turn is owned locally or by the backend', () => {
    expect(routesAcpStopToBackgroundControl(composerFor({ waitingForUserInteraction: true }), backgroundControl)).toBe(true);
    expect(routesAcpStopToBackgroundControl(composerFor({}), null)).toBe(false);
  });

  it.each([
    ['sending', { sending: true }],
    ['optimistic prompt', { waitingForOptimisticPrompt: true }],
    ['awaiting response', { awaitingResponse: true, turnAccepted: true }],
  ] as const)('keeps a locally submitted turn (%s) on the foreground path', (_label, local) => {
    const composer = composerFor({ localTurnId: 'turn-new', ...local });
    expect(routesAcpStopToBackgroundControl(composer, backgroundControl)).toBe(false);
  });
});

describe('ACP stop routing for session-owned interactions', () => {
  it('routes Stop through the background control path while a background question is pending', async () => {
    const backgroundControl: DirectBackgroundControl = {
      sessionId: 'session-bg',
      connectionGeneration: 12,
      activeTools: 1,
      expiresAtMs: Date.now() + 60_000,
    };
    const session = sessionWithPendingBackgroundQuestion('session-bg');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(
        <TestProviders>
          <ACPChatDialog
            session={session}
            projectId="project-1"
            taskId="task-1"
            runId="run-1"
            roundId={session.roundId}
            nodeId={session.nodeId}
            attemptId={session.attemptId}
            runtimeComposerContext={{
              isOrchestrated: false,
              lifecycle: idleDirectLifecycleWithBackground(backgroundControl),
              promptQueueEnabled: true,
              workflowValid: true,
            }}
            showSystemPromptAction={false}
            showRawFramesAction={false}
            usageCompact
          />
        </TestProviders>,
      );
    });
    try {
      const stop = container.querySelector('svg.lucide-circle-stop')?.closest('button');
      expect(stop).toBeTruthy();
      await act(async () => {
        stop!.click();
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(apiMocks.stopActiveSession).toHaveBeenCalledTimes(1);
      const args = apiMocks.stopActiveSession.mock.calls[0]!;
      expect(args[9]).toEqual(backgroundControl);
      expect(args[10]).toBe('turn-settled');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('routes Stop through the foreground path while a foreground turn is active', async () => {
    const backgroundControl: DirectBackgroundControl = {
      sessionId: 'session-fg',
      connectionGeneration: 12,
      activeTools: 1,
      expiresAtMs: Date.now() + 60_000,
    };
    const session = sessionWithPendingBackgroundQuestion('session-fg');
    const lifecycle = idleDirectLifecycleWithBackground(backgroundControl);
    lifecycle.acp = {
      ...lifecycle.acp,
      turnId: 'turn-active',
      liveTurnActivity: 'running',
      latestTurnStatus: 'none',
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    await act(async () => {
      root.render(
        <TestProviders>
          <ACPChatDialog
            session={session}
            projectId="project-1"
            taskId="task-1"
            runId="run-1"
            roundId={session.roundId}
            nodeId={session.nodeId}
            attemptId={session.attemptId}
            runtimeComposerContext={{
              isOrchestrated: false,
              lifecycle,
              promptQueueEnabled: true,
              workflowValid: true,
            }}
            showSystemPromptAction={false}
            showRawFramesAction={false}
            usageCompact
          />
        </TestProviders>,
      );
    });
    try {
      const stop = container.querySelector('svg.lucide-circle-stop')?.closest('button');
      expect(stop).toBeTruthy();
      await act(async () => {
        stop!.click();
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      });

      expect(apiMocks.stopActiveSession).toHaveBeenCalledTimes(1);
      const args = apiMocks.stopActiveSession.mock.calls[0]!;
      expect(args[9]).toBeUndefined();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
