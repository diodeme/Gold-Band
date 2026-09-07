/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  continueConversationRuntime: vi.fn(),
  getAcpSession: vi.fn(),
  submitConversationPrompt: vi.fn(),
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
import type {
  AcpSessionVm,
  AcpUiEventVm,
  ConversationAttemptLifecycleVm,
} from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let fixtureIndex = 0;

function pausedLifecycle(): ConversationAttemptLifecycleVm {
  return {
    runtime: {
      status: 'paused',
      outcome: null,
      pauseReason: 'process-interrupted',
      resumable: true,
      current: true,
      active: false,
      continuable: true,
      phase: 'paused',
    },
    control: { mode: 'non-runtime-controlled' },
    acp: {
      sessionAvailability: 'established',
      liveTurnActivity: 'idle',
      latestTurnStatus: 'cancelled',
      stopping: false,
    },
    displayStatus: 'paused',
    runtimeDisplay: {
      code: 'paused',
      tone: 'warning',
      icon: 'pause',
      terminal: false,
      resumable: true,
      reasonCode: 'process-interrupted',
      blockingError: false,
    },
    continueKind: 'continue-current-attempt',
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

function runtimeAbnormalLifecycle(): ConversationAttemptLifecycleVm {
  return {
    ...pausedLifecycle(),
    runtime: {
      ...pausedLifecycle().runtime,
      pauseReason: 'runtime-abnormal',
    },
    acp: {
      ...pausedLifecycle().acp,
      latestTurnStatus: 'failed',
    },
    displayStatus: 'runtime-abnormal',
    runtimeDisplay: {
      ...pausedLifecycle().runtimeDisplay,
      code: 'runtime-abnormal',
      tone: 'danger',
      reasonCode: 'runtime-abnormal',
    },
  };
}

function runningLifecycle(): ConversationAttemptLifecycleVm {
  return {
    ...pausedLifecycle(),
    runtime: {
      status: 'running',
      outcome: null,
      pauseReason: null,
      resumable: false,
      current: true,
      active: true,
      continuable: false,
      phase: 'provider-running',
    },
    acp: {
      sessionAvailability: 'established',
      liveTurnActivity: 'starting',
      latestTurnStatus: 'none',
      stopping: false,
    },
    displayStatus: 'running',
    runtimeDisplay: {
      code: 'running',
      tone: 'running',
      icon: 'dot',
      terminal: false,
      resumable: false,
      reasonCode: null,
      blockingError: false,
    },
    continueKind: null,
    composer: {
      mode: 'runtime-active',
      submitTarget: 'none',
      processingKind: 'processing',
      statusKey: 'conversation.runtime.runtimeActive',
      canStop: true,
      lockInput: true,
    },
  };
}

function activeDirectLifecycle(): ConversationAttemptLifecycleVm {
  return {
    ...runningLifecycle(),
    composer: {
      mode: 'runtime-active',
      submitTarget: 'queue-prompt',
      processingKind: 'processing',
      statusKey: 'conversation.runtime.runtimeActive',
      canStop: true,
      lockInput: false,
    },
    promptQueue: {
      revision: 0,
      items: [],
      maxItems: 10,
    },
  };
}

function cancelledSession(id: string): AcpSessionVm {
  return {
    branchId: 'root',
    parentBranchId: null,
    readOnly: false,
    branchExecution: null,
    sessionId: id,
    title: 'Runtime continue',
    roundId: `round-${id}`,
    nodeId: `node-${id}`,
    attemptId: `attempt-${id}`,
    provider: 'test',
    status: 'cancelled',
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
    pendingInteractions: [],
    diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
  };
}

async function renderPausedDialog(options: {
  onOptimisticEventsChange?: (events: AcpUiEventVm[]) => void;
  initialLifecycle?: ConversationAttemptLifecycleVm;
  isOrchestrated?: boolean;
  runtimeError?: string | null;
  runtimeErrorFallback?: string | null;
  sessionStatus?: string;
  session?: Partial<AcpSessionVm>;
} = {}) {
  fixtureIndex += 1;
  const id = String(fixtureIndex);
  const session = {
    ...cancelledSession(id),
    status: options.sessionStatus ?? 'cancelled',
    ...options.session,
  };
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  const render = async (
    lifecycle: ConversationAttemptLifecycleVm,
    nextSession: AcpSessionVm = session,
  ) => act(async () => {
    root.render(
      <TooltipProvider>
        <ACPChatDialog
          session={nextSession}
          projectId={`project-${id}`}
          taskId={`task-${id}`}
          runId={`run-${id}`}
          roundId={session.roundId}
          nodeId={session.nodeId}
          attemptId={session.attemptId}
          runtimeComposerContext={{
            isOrchestrated: options.isOrchestrated ?? true,
            lifecycle,
            workflowValid: true,
            runtimeError: options.runtimeError,
            runtimeErrorFallback: options.runtimeErrorFallback,
          }}
          showSystemPromptAction={false}
          showRawFramesAction={false}
          usageCompact
          onOptimisticEventsChange={options.onOptimisticEventsChange}
        />
      </TooltipProvider>,
    );
  });
  await render(options.initialLifecycle ?? pausedLifecycle());
  return { container, id, root, session, render };
}

async function renderActiveDirectDialog() {
  fixtureIndex += 1;
  const id = String(fixtureIndex);
  const readyEvent = {
    id: `ready-${id}`,
    seq: 1,
    timestamp: '1786980000Z',
    kind: 'textDelta',
    sessionId: id,
    content: 'ready',
    status: 'completed',
    startedSeq: 1,
    endedSeq: 1,
  } satisfies AcpUiEventVm;
  const session = {
    ...cancelledSession(id),
    title: 'Direct queue boundary',
    status: 'running',
    events: [readyEvent],
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
  } satisfies AcpSessionVm;
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TooltipProvider>
        <ACPChatDialog
          session={session}
          projectId={`project-${id}`}
          taskId={`task-${id}`}
          runId={`run-${id}`}
          roundId={session.roundId}
          nodeId={session.nodeId}
          attemptId={session.attemptId}
          runtimeComposerContext={{
            isOrchestrated: false,
            lifecycle: activeDirectLifecycle(),
            promptQueueEnabled: true,
            workflowValid: true,
          }}
          showSystemPromptAction={false}
          showRawFramesAction={false}
          usageCompact
        />
      </TooltipProvider>,
    );
  });
  return { container, id, root, session };
}

async function renderCancelledDirectAttemptDialog() {
  fixtureIndex += 1;
  const id = String(fixtureIndex);
  const lifecycle = pausedLifecycle();
  lifecycle.acp = {
    ...lifecycle.acp,
    sessionAvailability: 'unavailable',
  };
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TooltipProvider>
        <ACPChatDialog
          session={null}
          projectId={`project-${id}`}
          taskId={`task-${id}`}
          runId={`run-${id}`}
          roundId={`round-${id}`}
          nodeId={`node-${id}`}
          attemptId={`attempt-${id}`}
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
      </TooltipProvider>,
    );
  });
  return { container, id, root };
}

async function setTextareaValue(textarea: HTMLTextAreaElement, value: string) {
  const valueSetter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    'value',
  )?.set;
  await act(async () => {
    valueSetter?.call(textarea, value);
    textarea.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function flushInteraction(action: () => void) {
  await act(async () => {
    action();
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  });
}

async function unmount(root: Root) {
  await act(async () => root.unmount());
}

beforeEach(() => {
  apiMocks.continueConversationRuntime.mockReset();
  apiMocks.getAcpSession.mockReset().mockResolvedValue(null);
  apiMocks.submitConversationPrompt.mockReset().mockResolvedValue({
    kind: 'rejected',
    session: null,
    run: null,
    lifecycle: null,
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
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('ACP runtime continue submission', () => {
  it('keeps a newer-turn permission visible while lifecycle is terminal for the prior turn', async () => {
    const terminal = pausedLifecycle();
    terminal.acp = {
      ...terminal.acp,
      revision: 2,
      turnId: 'turn-1',
    };
    const { container, root, render, session } = await renderPausedDialog({
      initialLifecycle: terminal,
      sessionStatus: 'completed',
    });
    try {
      await render(terminal, {
        ...session,
        status: 'completed',
        eventPage: {
          ...session.eventPage,
          generation: 1,
          coveredRevision: 3,
          newestRevision: 3,
          newestSeq: 3,
        },
        pendingInteractions: [{
          kind: 'permission',
          interactionId: 'request-turn-2',
          turnId: 'turn-2',
          promptEventId: 'prompt-turn-2',
          title: 'NEW_TURN_PERMISSION_CARD',
          options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }],
          raw: { requestId: 'request-turn-2' },
        }],
      });

      expect(container.textContent).toContain('NEW_TURN_PERMISSION_CARD');
    } finally {
      await unmount(root);
    }
  });

  it('settles an old permission card once and still accepts a real permission from the next turn', async () => {
    const firstTurn = runningLifecycle();
    firstTurn.acp = {
      ...firstTurn.acp,
      revision: 1,
      turnId: 'turn-1',
    };
    const terminal = pausedLifecycle();
    terminal.acp = {
      ...terminal.acp,
      revision: 2,
      turnId: 'turn-1',
    };
    const nextTurn = runningLifecycle();
    nextTurn.acp = {
      ...nextTurn.acp,
      revision: 3,
      turnId: 'turn-2',
    };
    const oldPermission = {
      kind: 'permission' as const,
      interactionId: 'request-old',
      turnId: 'turn-1',
      promptEventId: 'prompt-turn-1',
      title: 'OLD_PERMISSION_CARD',
      options: [{ optionId: 'allow', name: 'Allow', kind: 'allow_once' }],
      raw: { requestId: 'request-old' },
    };
    const { container, root, render, session } = await renderPausedDialog({
      initialLifecycle: firstTurn,
      sessionStatus: 'running',
      session: {
        pendingInteractions: [oldPermission],
      },
    });
    try {
      expect(container.textContent).toContain('OLD_PERMISSION_CARD');

      await render(terminal);
      expect(container.textContent).not.toContain('OLD_PERMISSION_CARD');

      await render(nextTurn);
      expect(container.textContent).not.toContain('OLD_PERMISSION_CARD');

      await render(nextTurn, {
        ...session,
        eventPage: {
          ...session.eventPage,
          coveredRevision: 4,
          newestRevision: 4,
          newestSeq: 4,
        },
        pendingInteractions: [{
          ...oldPermission,
          turnId: 'turn-2',
          promptEventId: 'prompt-turn-2',
          title: 'NEW_PERMISSION_CARD',
          raw: { requestId: 'request-old' },
        }],
      });
      expect(container.textContent).toContain('NEW_PERMISSION_CARD');
    } finally {
      await unmount(root);
    }
  });

  it('settles an old elicitation card once and still accepts a real elicitation from the next turn', async () => {
    const firstTurn = runningLifecycle();
    firstTurn.acp = { ...firstTurn.acp, revision: 1, turnId: 'turn-1' };
    const terminal = pausedLifecycle();
    terminal.acp = { ...terminal.acp, revision: 2, turnId: 'turn-1' };
    const nextTurn = runningLifecycle();
    nextTurn.acp = { ...nextTurn.acp, revision: 3, turnId: 'turn-2' };
    const oldElicitation = {
      kind: 'elicitation' as const,
      interactionId: 'elicitation-old',
      turnId: 'turn-1',
      promptEventId: 'prompt-turn-1',
      message: 'OLD_ELICITATION_CARD',
      requestedSchema: {
        type: 'object',
        properties: { answer: { type: 'string', title: 'Answer' } },
      },
      raw: { elicitationId: 'elicitation-old' },
    };
    const { container, root, render, session } = await renderPausedDialog({
      initialLifecycle: firstTurn,
      sessionStatus: 'running',
      session: {
        pendingInteractions: [oldElicitation],
      },
    });
    try {
      expect(container.textContent).toContain('OLD_ELICITATION_CARD');

      await render(terminal);
      expect(container.textContent).not.toContain('OLD_ELICITATION_CARD');

      await render(nextTurn);
      expect(container.textContent).not.toContain('OLD_ELICITATION_CARD');

      await render(nextTurn, {
        ...session,
        eventPage: {
          ...session.eventPage,
          coveredRevision: 4,
          newestRevision: 4,
          newestSeq: 4,
        },
        pendingInteractions: [{
          ...oldElicitation,
          interactionId: 'elicitation-new',
          turnId: 'turn-2',
          promptEventId: 'prompt-turn-2',
          message: 'NEW_ELICITATION_CARD',
          raw: { elicitationId: 'elicitation-new' },
        }],
      });
      expect(container.textContent).toContain('NEW_ELICITATION_CARD');
    } finally {
      await unmount(root);
    }
  });

  it('submits on the same mounted page after terminal lifecycle settles a stale running session snapshot', async () => {
    const { container, root, render } = await renderPausedDialog({
      initialLifecycle: runningLifecycle(),
      sessionStatus: 'running',
    });
    try {
      await render(pausedLifecycle());
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      expect(textarea).not.toBeNull();
      await setTextareaValue(textarea!, '停止后的第二句');

      const sendButton = container.querySelector<HTMLButtonElement>('[data-acp-send="true"]');
      expect(sendButton?.disabled).toBe(false);
      await flushInteraction(() => sendButton?.click());
      expect(apiMocks.submitConversationPrompt).toHaveBeenCalledTimes(1);

      apiMocks.submitConversationPrompt.mockClear();
      await flushInteraction(() => {
        textarea!.dispatchEvent(new KeyboardEvent('keydown', {
          key: 'Enter',
          bubbles: true,
        }));
      });
      expect(apiMocks.submitConversationPrompt).toHaveBeenCalledTimes(1);
    } finally {
      await unmount(root);
    }
  });

  it('keeps the send button and Enter on the ordinary conversation path', async () => {
    const { container, root } = await renderPausedDialog();
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      expect(textarea).not.toBeNull();
      await setTextareaValue(textarea!, '普通发送');

      const continueButton = container.querySelector<HTMLButtonElement>(
        '[data-acp-continue-workflow="true"]',
      );
      expect(continueButton?.textContent).toContain('继续并发送');

      await flushInteraction(() => {
        container.querySelector<HTMLButtonElement>('[data-acp-send="true"]')?.click();
      });
      expect(apiMocks.submitConversationPrompt).toHaveBeenCalledTimes(1);
      expect(apiMocks.continueConversationRuntime).not.toHaveBeenCalled();

      apiMocks.submitConversationPrompt.mockClear();
      await flushInteraction(() => {
        textarea!.dispatchEvent(new KeyboardEvent('keydown', {
          key: 'Enter',
          bubbles: true,
        }));
      });
      expect(apiMocks.submitConversationPrompt).toHaveBeenCalledTimes(1);
      expect(apiMocks.continueConversationRuntime).not.toHaveBeenCalled();
    } finally {
      await unmount(root);
    }
  });

  it('submits one atomic runtime continue while keeping the optimistic bubble in sending state', async () => {
    const optimisticSnapshots: AcpUiEventVm[][] = [];
    apiMocks.continueConversationRuntime.mockResolvedValue({
      kind: 'runtime-continue-started',
      session: null,
      run: null,
      lifecycle: runningLifecycle(),
    });
    const { container, id, root, session } = await renderPausedDialog({
      onOptimisticEventsChange: (events) => optimisticSnapshots.push(events),
    });
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      await setTextareaValue(textarea!, '继续并补充测试');
      await flushInteraction(() => {
        container.querySelector<HTMLButtonElement>(
          '[data-acp-continue-workflow="true"]',
        )?.click();
      });

      expect(apiMocks.continueConversationRuntime).toHaveBeenCalledTimes(1);
      expect(apiMocks.submitConversationPrompt).not.toHaveBeenCalled();
      expect(apiMocks.continueConversationRuntime).toHaveBeenCalledWith(
        `project-${id}`,
        `task-${id}`,
        `run-${id}`,
        session.roundId,
        session.nodeId,
        session.attemptId,
        undefined,
        undefined,
        { displayText: '继续并补充测试', quotes: [] },
        expect.any(String),
        undefined,
      );
      expect(optimisticSnapshots.at(-1)?.at(-1)).toMatchObject({
        content: '继续并补充测试',
        kind: 'userTextDelta',
        status: 'sending',
      });
    } finally {
      await unmount(root);
    }
  });

  it('restores the detached draft when runtime continue fails', async () => {
    apiMocks.continueConversationRuntime.mockRejectedValue(new Error('continue failed'));
    const { container, root } = await renderPausedDialog();
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      await setTextareaValue(textarea!, '失败后保留');
      await flushInteraction(() => {
        container.querySelector<HTMLButtonElement>(
          '[data-acp-continue-workflow="true"]',
        )?.click();
      });

      expect(apiMocks.continueConversationRuntime).toHaveBeenCalledTimes(1);
      expect(textarea?.value).toBe('失败后保留');
      expect(container.textContent).toContain('continue failed');
    } finally {
      await unmount(root);
    }
  });
});

describe('ACP Direct queue submission', () => {
  it('invalidates a stale parent runtime error after the local Direct lifecycle recovers', async () => {
    const recoveredLifecycle = {
      ...runtimeAbnormalLifecycle(),
      acp: {
        ...runtimeAbnormalLifecycle().acp,
        latestTurnStatus: 'completed' as const,
      },
    };
    const result = await renderPausedDialog({
      initialLifecycle: runtimeAbnormalLifecycle(),
      isOrchestrated: false,
      runtimeError: 'end_turn: old provider failure',
      session: {
        diagnostics: {
          rawFrameCount: 8,
          eventCount: 1,
          errorCount: 1,
          lastError: 'ACP prompt failed: old provider failure',
          lastErrorTimestamp: '10Z',
        },
      },
    });
    try {
      expect(result.container.textContent).toContain('old provider failure');
      await result.render(recoveredLifecycle, {
        ...result.session,
        status: 'completed',
        events: [{
          id: 'recovered-response',
          seq: 2,
          timestamp: '11Z',
          kind: 'textDelta',
          sessionId: result.session.sessionId,
          content: 'recovered',
          status: 'completed',
          startedSeq: 2,
          endedSeq: 2,
        }],
        eventPage: {
          loadedCount: 1,
          total: 1,
          oldestSeq: 2,
          newestSeq: 2,
          hasOlder: false,
          hasNewer: false,
          oldestCursor: '2',
          newestCursor: '2',
        },
      });
      expect(result.container.textContent).not.toContain('old provider failure');
    } finally {
      await unmount(result.root);
    }
  });

  it('uses a stale run error only as fallback for ACP diagnostics', async () => {
    const result = await renderPausedDialog({
      runtimeErrorFallback: 'old provider failure',
    });
    try {
      expect(result.container.textContent).toContain('old provider failure');
      await result.render(pausedLifecycle(), {
        ...result.session,
        status: 'completed',
        events: [{
          id: 'recovered-thought',
          seq: 2,
          timestamp: '11Z',
          kind: 'thoughtDelta',
          sessionId: result.session.sessionId,
          content: 'recovered',
          status: 'completed',
          startedSeq: 2,
          endedSeq: 2,
        }],
        eventPage: {
          loadedCount: 1,
          total: 1,
          oldestSeq: 2,
          newestSeq: 2,
          hasOlder: false,
          hasNewer: false,
          oldestCursor: '2',
          newestCursor: '2',
        },
        diagnostics: {
          rawFrameCount: 8,
          eventCount: 2,
          errorCount: 1,
          lastError: 'old provider failure',
          lastErrorTimestamp: '10Z',
        },
      });
      expect(result.container.textContent).not.toContain('old provider failure');
    } finally {
      await unmount(result.root);
    }
  });

  it('hides a stale run fallback after a diagnostic-free follow-up completes', async () => {
    const recoveredLifecycle = {
      ...runtimeAbnormalLifecycle(),
      acp: {
        ...runtimeAbnormalLifecycle().acp,
        latestTurnStatus: 'completed' as const,
        stopReason: 'end_turn',
      },
    };
    const result = await renderPausedDialog({
      initialLifecycle: runtimeAbnormalLifecycle(),
      isOrchestrated: false,
      runtimeErrorFallback: 'end_turn: old provider failure',
    });
    try {
      expect(result.container.textContent).toContain('old provider failure');
      await result.render(recoveredLifecycle, {
        ...result.session,
        status: 'completed',
        events: [{
          id: 'diagnostic-free-recovered-response',
          seq: 2,
          timestamp: '11Z',
          kind: 'textDelta',
          sessionId: result.session.sessionId,
          content: 'recovered',
          status: 'completed',
          startedSeq: 2,
          endedSeq: 2,
        }],
        eventPage: {
          loadedCount: 1,
          total: 1,
          oldestSeq: 2,
          newestSeq: 2,
          hasOlder: false,
          hasNewer: false,
          oldestCursor: '2',
          newestCursor: '2',
        },
      });
      expect(result.container.textContent).not.toContain('old provider failure');
    } finally {
      await unmount(result.root);
    }
  });

  it('continues on the same page after startup is stopped before a provider session exists', async () => {
    apiMocks.submitConversationPrompt.mockResolvedValue({
      kind: 'acp-session-started',
      session: null,
      run: null,
      lifecycle: runningLifecycle(),
    });
    const { container, id, root } = await renderCancelledDirectAttemptDialog();
    try {
      expect(container.textContent).not.toContain('ACP session failed');
      expect(container.textContent).not.toContain('ACP 会话失败');
      expect(container.querySelector('[data-acp-continue-workflow="true"]')).toBeNull();

      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      expect(textarea).not.toBeNull();
      await setTextareaValue(textarea!, '首轮停止后的下一句');

      const sendButton = container.querySelector<HTMLButtonElement>('[data-acp-send="true"]');
      expect(sendButton?.disabled).toBe(false);
      await flushInteraction(() => sendButton?.click());

      expect(apiMocks.submitConversationPrompt).toHaveBeenCalledWith(
        `project-${id}`,
        `task-${id}`,
        `run-${id}`,
        `round-${id}`,
        `node-${id}`,
        `attempt-${id}`,
        { displayText: '首轮停止后的下一句', quotes: [] },
        expect.any(String),
        expect.objectContaining({
          sessionId: null,
          status: 'cancelled',
        }),
        undefined,
        undefined,
        undefined,
      );
    } finally {
      await unmount(root);
    }
  });

  it('settles a queue-targeted submission when the backend starts it directly at the idle boundary', async () => {
    const { container, id, root, session } = await renderActiveDirectDialog();
    apiMocks.submitConversationPrompt.mockResolvedValue({
      kind: 'acp-session',
      session: { ...session, status: 'completed' },
      run: null,
      lifecycle: activeDirectLifecycle(),
    });
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea');
      await setTextareaValue(textarea!, '边界消息只发送一次');

      const sendButton = container.querySelector<HTMLButtonElement>('[data-acp-send="true"]');
      expect(sendButton?.disabled).toBe(false);
      await flushInteraction(() => {
        sendButton?.click();
      });

      expect(apiMocks.submitConversationPrompt).toHaveBeenCalledWith(
        `project-${id}`,
        `task-${id}`,
        `run-${id}`,
        session.roundId,
        session.nodeId,
        session.attemptId,
        { displayText: '边界消息只发送一次', quotes: [] },
        null,
        expect.any(Object),
        undefined,
        undefined,
        undefined,
      );
      expect(textarea?.value).toBe('');
      expect(container.textContent).not.toContain('unexpected prompt queue response');
    } finally {
      await unmount(root);
    }
  });
});
