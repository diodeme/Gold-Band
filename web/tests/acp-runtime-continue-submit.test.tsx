/** @vitest-environment jsdom */

import React, { act, useLayoutEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  continueConversationRuntime: vi.fn(),
  getAcpSession: vi.fn(),
  submitConversationPrompt: vi.fn(),
  stopActiveSession: vi.fn(),
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, ...apiMocks };
});

vi.mock('@/components/prompt-kit/markdown', () => ({
  Markdown: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { ACPChatDialog, createAcpEventWindowCacheKey } from '@/components/acp/ACPChatDialog';
import { useAcpComposerDraft, type AcpComposerDraft } from '@/lib/acp-composer-draft';
import { getRuntimeApi } from '@/api/client';
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
  initialDraft?: AcpComposerDraft;
  onSubmitManualCheck?: (outcome: 'success' | 'failure') => Promise<void>;
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
  if (options.initialDraft) {
    const draftKey = createAcpEventWindowCacheKey({
      projectId: `project-${id}`, taskId: `task-${id}`, runId: `run-${id}`,
      roundId: session.roundId, nodeId: session.nodeId, attemptId: session.attemptId, branchId: 'root',
    });
    function SeedDraft() {
      const controller = useAcpComposerDraft(draftKey);
      useLayoutEffect(() => { controller.restoreIfEmpty(options.initialDraft!); }, []);
      return null;
    }
    await act(async () => root.render(<SeedDraft />));
  }
  const render = async (
    lifecycle: ConversationAttemptLifecycleVm,
    nextSession: AcpSessionVm = session,
    locator = session,
  ) => act(async () => {
    root.render(
      <TooltipProvider>
        <ACPChatDialog
          session={nextSession}
          projectId={`project-${id}`}
          taskId={`task-${id}`}
          runId={`run-${id}`}
          roundId={locator.roundId}
          nodeId={locator.nodeId}
          attemptId={locator.attemptId}
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
          manualCheckPending={Boolean(options.onSubmitManualCheck)}
          onSubmitManualCheck={options.onSubmitManualCheck}
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
  Range.prototype.getClientRects = () => [] as unknown as DOMRectList;
  apiMocks.stopActiveSession.mockReset();
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
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  document.body.replaceChildren();
});

describe('ACP runtime continue submission', () => {
  const contextDraft = (): AcpComposerDraft => ({
    content: 'cancelled draft',
    attachments: [
      { id: 'image', name: 'draft.png', size: 3, mime: 'image/png', source: 'browser-file', path: 'C:/draft.png',
        file: new File(['png'], 'draft.png', { type: 'image/png' }), previewUrl: 'blob:draft-image' },
      { id: 'file', name: 'notes.txt', size: 4, mime: 'text/plain', source: 'dialog', path: 'C:/notes.txt' },
    ],
    quotes: [{ id: 'quote', sourceKey: 'original-message', text: 'quoted message' }],
  });
  function mockHistory() {
    const cursor = { generation: 1, position: 1, messageId: 'history' };
    vi.spyOn(getRuntimeApi(), 'listComposerHistory').mockImplementation(async (_, query) => ({
      items: query.direction === 'newer' ? [] : [{ cursor, textBytes: 12 }], head: cursor, nextCursor: null,
    }));
    vi.spyOn(getRuntimeApi(), 'getComposerHistoryText').mockResolvedValue({ cursor, text: 'history text' });
  }
  async function press(textarea: HTMLTextAreaElement, key: string) {
    await flushInteraction(() => textarea.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true })));
  }
  function expectContext(container: HTMLElement, visible: boolean) {
    const composer = container.querySelector('[data-conversation-composer]')!;
    expect(composer.querySelectorAll('[data-composer-attachment-chip]')).toHaveLength(visible ? 2 : 0);
    expect(composer.querySelectorAll('[data-composer-quote-chip]')).toHaveLength(visible ? 1 : 0);
    if (visible) expect(composer.querySelector('img')?.getAttribute('src')).toBe('blob:draft-image');
  }

  it.each([false, true])('restores the full unaccepted draft when stop settles first: %s', async (stopFirst) => {
    mockHistory();
    const revoke = vi.fn();
    vi.stubGlobal('URL', class extends URL { static revokeObjectURL = revoke; });
    let rejectSubmit!: (error: Error) => void;
    let resolveStop!: (value: unknown) => void;
    apiMocks.submitConversationPrompt.mockImplementation(() => new Promise((_, reject) => { rejectSubmit = reject; }));
    apiMocks.stopActiveSession.mockImplementation(() => new Promise((resolve) => { resolveStop = resolve; }));
    const { container, root } = await renderPausedDialog({ initialDraft: contextDraft() });
    try {
      const textarea = container.querySelector('textarea')!;
      await setTextareaValue(textarea, 'cancelled draft');
      await flushInteraction(() => container.querySelector<HTMLButtonElement>('[data-acp-send]')!.click());
      expect(textarea.value).toBe('');
      const stop = container.querySelector('svg.lucide-circle-stop')?.closest('button');
      expect(stop).toBeTruthy();
      await flushInteraction(() => stop!.click());
      expect(apiMocks.stopActiveSession).toHaveBeenCalledTimes(1);
      if (stopFirst) await act(async () => resolveStop({ status: 'stopped', session: null, lifecycle: pausedLifecycle() }));
      await act(async () => rejectSubmit(new Error('submission cancelled')));
      expect(textarea.value).toBe('cancelled draft');
      expectContext(container, true);
      await act(async () => resolveStop({ status: 'stopped', session: null, lifecycle: pausedLifecycle() }));
      await press(textarea, 'ArrowUp');
      expect(textarea.value).toBe('history text');
      expectContext(container, false);
      await press(textarea, 'ArrowDown');
      expect(textarea.value).toBe('cancelled draft');
      expectContext(container, true);
      expect(revoke).not.toHaveBeenCalled();
    } finally {
      await act(async () => resolveStop({ kind: 'stopped', session: null, lifecycle: pausedLifecycle() }));
      await unmount(root);
    }
  });

  it.each(['Enter', 'continue'] as const)('submits recalled text without hidden draft context via %s and restores that text on rejection', async (command) => {
    mockHistory();
    const revoke = vi.fn();
    vi.stubGlobal('URL', class extends URL { static revokeObjectURL = revoke; });
    apiMocks.continueConversationRuntime.mockResolvedValue({ kind: 'rejected', session: null, lifecycle: null });
    const { container, root } = await renderPausedDialog({ initialDraft: contextDraft() });
    try {
      const textarea = container.querySelector('textarea')!;
      expectContext(container, true);
      await press(textarea, 'ArrowUp');
      expectContext(container, false);
      if (command === 'Enter') await press(textarea, 'Enter');
      else await flushInteraction(() => container.querySelector<HTMLButtonElement>('[data-acp-continue-workflow]')!.click());
      const calls = command === 'Enter' ? apiMocks.submitConversationPrompt : apiMocks.continueConversationRuntime;
      expect(calls).toHaveBeenCalledTimes(1);
      const args = calls.mock.calls[0];
      expect(args[command === 'Enter' ? 6 : 8]).toEqual({ displayText: 'history text', quotes: [] });
      expect(args.at(-1)).toBeUndefined();
      expect(textarea.value).toBe('history text');
      expectContext(container, false);
      expect(revoke).toHaveBeenCalledExactlyOnceWith('blob:draft-image');
    } finally { await unmount(root); }
  });

  it('keeps an accepted submission detached even when its response contains no timeline admission', async () => {
    const { container, root, session } = await renderPausedDialog({ initialDraft: contextDraft() });
    const revoke = vi.fn();
    vi.stubGlobal('URL', class extends URL { static revokeObjectURL = revoke; });
    apiMocks.submitConversationPrompt.mockResolvedValue({ kind: 'acp-session', session, lifecycle: pausedLifecycle() });
    try {
      await flushInteraction(() => container.querySelector<HTMLButtonElement>('[data-acp-send]')!.click());
      expect(container.querySelector('textarea')!.value).toBe('');
      expectContext(container, false);
      expect(revoke).toHaveBeenCalledExactlyOnceWith('blob:draft-image');
    } finally { await unmount(root); }
  });

  it('does not hide a new attempt decision after an old manual-check response arrives', async () => {
    let resolve!: () => void;
    const submit = vi.fn(() => new Promise<void>((done) => { resolve = done; }));
    const { container, root, render, session } = await renderPausedDialog({ onSubmitManualCheck: submit });
    const decision = () => [...container.querySelectorAll('button')].find((item) =>
      item.textContent === '成功' || item.textContent === 'acp.manualCheckSuccess',
    );
    try {
      await flushInteraction(() => decision()!.click());
      const next = { ...session, attemptId: 'replacement-attempt' };
      await render(pausedLifecycle(), next, next);
      expect(decision()?.disabled).toBe(false);
      await act(async () => resolve());
      expect(decision()?.disabled).toBe(false);
    } finally {
      await unmount(root);
    }
  });

  it.each(['success', 'failure'] as const)('submits manual-check %s once and settles the decision buttons', async (outcome) => {
    let resolve!: () => void;
    const submit = vi.fn(() => new Promise<void>((done) => { resolve = done; }));
    const { container, root } = await renderPausedDialog({ onSubmitManualCheck: submit });
    try {
      const button = [...container.querySelectorAll('button')].find((item) =>
        item.textContent === (outcome === 'success' ? '成功' : '失败')
        || item.textContent === `acp.manualCheck${outcome === 'success' ? 'Success' : 'Failure'}`,
      );
      expect(button).toBeDefined();
      await flushInteraction(() => button!.click());
      expect(submit).toHaveBeenCalledExactlyOnceWith(outcome);
      expect(button!.disabled).toBe(true);
      await flushInteraction(() => button!.click());
      expect(submit).toHaveBeenCalledTimes(1);
      await act(async () => resolve());
      expect(container.contains(button!)).toBe(false);
    } finally {
      await unmount(root);
    }
  });

  it('shows an unknown runtime failure verbatim and clears it when a new turn starts', async () => {
    const lifecycle = runtimeAbnormalLifecycle();
    lifecycle.acp = { ...lifecycle.acp, revision: 3, turnId: 'unknown-turn', turnError: {
      code: { domain: 'internal', code: 'internal.unknown' },
      domain: 'internal', recovery: 'manual', diagnostic: '磁盘空间不足。 (os error 112)',
    } };
    const { container, root, render, session } = await renderPausedDialog({
      initialLifecycle: lifecycle, sessionStatus: 'failed', session: { turnError: lifecycle.acp.turnError },
    });
    try {
      expect(container.textContent).toContain('磁盘空间不足。 (os error 112)');
      expect(container.textContent).not.toContain('检查所选 Agent');
      const next = pausedLifecycle();
      next.acp = { ...next.acp, revision: 4, turnId: 'next-turn', latestTurnStatus: 'none', liveTurnActivity: 'starting', turnError: null };
      await render(next, { ...session, status: 'pending', turnError: null });
      expect(container.textContent).not.toContain('os error 112');
    } finally {
      await unmount(root);
    }
  });

  it('shows the background restore failure without diagnostic history and clears it on retry', async () => {
    const lifecycle = pausedLifecycle();
    lifecycle.acp.latestTurnStatus = 'failed';
    lifecycle.acp.revision = 3;
    lifecycle.acp.turnId = 'turn-a';
    lifecycle.acp.turnError = {
      code: { domain: 'provider', code: 'acp.session-request-failed' },
      domain: 'provider', recovery: 'manual', params: { method: 'session/resume' },
      diagnostic: 'restore failed',
      raw: { code: -32603, message: 'Internal error', data: { details: 'thread session-a already has an active writer' } },
    };
    const { container, root, render, session } = await renderPausedDialog({
      isOrchestrated: false,
      initialLifecycle: lifecycle,
      sessionStatus: 'failed',
      session: { turnError: lifecycle.acp.turnError },
    });
    try {
      expect(container.textContent).toContain('thread session-a already has an active writer');
      const retry = pausedLifecycle();
      retry.acp = { ...retry.acp, revision: 4, turnId: 'turn-b', latestTurnStatus: 'none', liveTurnActivity: 'starting', turnError: null };
      await render(retry, { ...session, status: 'pending', turnError: null });
      expect(container.textContent).not.toContain('already has an active writer');
    } finally {
      await unmount(root);
    }
  });

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
