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
    pendingPermissions: [],
    pendingElicitations: [],
    diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
  };
}

async function renderPausedDialog(options: {
  onOptimisticEventsChange?: (events: AcpUiEventVm[]) => void;
} = {}) {
  fixtureIndex += 1;
  const id = String(fixtureIndex);
  const session = cancelledSession(id);
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
            isOrchestrated: true,
            lifecycle: pausedLifecycle(),
            workflowValid: true,
          }}
          showSystemPromptAction={false}
          showRawFramesAction={false}
          usageCompact
          onOptimisticEventsChange={options.onOptimisticEventsChange}
        />
      </TooltipProvider>,
    );
  });
  return { container, id, root, session };
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

  it('submits one atomic runtime continue and settles the optimistic bubble to processing', async () => {
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
        status: 'processing',
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
