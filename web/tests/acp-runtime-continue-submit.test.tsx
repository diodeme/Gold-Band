/** @vitest-environment jsdom */

import React, { act, useLayoutEffect } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const apiMocks = vi.hoisted(() => ({
  continueConversationRuntime: vi.fn(),
  getAcpSession: vi.fn(),
  submitConversationPrompt: vi.fn(),
  stopActiveSession: vi.fn(),
  showConversationMessageAttachment: vi.fn(),
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
import i18n from '@/i18n';
import { TooltipProvider } from '@/components/ui/tooltip';
import { RightWorkspaceProvider, useRightWorkspace } from '@/components/workspace/right-workspace-context';
import { RightWorkspaceDock } from '@/components/workspace/RightWorkspaceDock';
import { ConversationAssetWorkspacePanel } from '@/components/workspace/files/ConversationAssetWorkspacePanel';
import { DraftAttachmentWorkspacePanel } from '@/components/workspace/files/DraftAttachmentWorkspacePanel';
import type {
  AcpSessionVm,
  AcpUiEventVm,
  ConversationAttemptLifecycleVm,
} from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function TestProviders({ children }: { children: React.ReactNode }) {
  return React.createElement(
    RightWorkspaceProvider,
    null,
    React.createElement(TooltipProvider, null, children),
  );
}

function AttachmentWorkspaceProbe() {
  const workspace = useRightWorkspace();
  useLayoutEffect(() => {
    const removeAsset = workspace.registerResourceRenderer('conversation-asset', (resource) => (
      resource.kind === 'conversation-asset' ? <ConversationAssetWorkspacePanel key={resource.key} resource={resource} /> : null
    ));
    const removeDraft = workspace.registerResourceRenderer('draft-attachment', (resource) => (
      resource.kind === 'draft-attachment' ? <DraftAttachmentWorkspacePanel key={resource.key} resource={resource} /> : null
    ));
    return () => { removeAsset(); removeDraft(); };
  }, [workspace.registerResourceRenderer]);
  const active = workspace.tabs.find((tab) => tab.key === workspace.activeTabKey);
  return <output
    data-attachment-workspace-count={workspace.tabs.length}
    data-attachment-workspace-kind={active?.kind}
    data-attachment-workspace-path={active?.kind === 'conversation-asset' ? active.path : undefined}
  />;
}

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

function readySessionHistory(): Pick<AcpSessionVm, 'events' | 'eventPage'> {
  return {
    events: [{
      id: 'previous-response', seq: 1, timestamp: '10Z', kind: 'textDelta',
      content: 'Previous response', status: 'completed', startedSeq: 1, endedSeq: 1,
    }],
    eventPage: {
      loadedCount: 1, total: 1, oldestSeq: 1, newestSeq: 1,
      hasOlder: false, hasNewer: false, oldestCursor: '1', newestCursor: '1',
    },
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
  showAttachmentWorkspace?: boolean;
} = {}) {
  fixtureIndex += 1;
  const id = String(fixtureIndex);
  const session = {
    ...cancelledSession(id),
    ...readySessionHistory(),
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
    apiMocks.getAcpSession.mockResolvedValue(nextSession);
    root.render(
      <TestProviders>
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
        {options.showAttachmentWorkspace ? <><AttachmentWorkspaceProbe /><RightWorkspaceDock /></> : null}
      </TestProviders>,
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
  apiMocks.getAcpSession.mockResolvedValue(session);
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      <TestProviders>
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
      </TestProviders>,
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
      <TestProviders>
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
      </TestProviders>,
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
  apiMocks.showConversationMessageAttachment.mockReset().mockResolvedValue({
    title: 'draft.png', kind: 'message-attachment',
    content: 'data:image/png;base64,AQIDBA==', metadata: { mimeType: 'image/png' },
  });
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
    quotes: [{ id: 'quote', source: { kind: 'agentMessage', messageKey: 'original-message' }, text: 'quoted message' }],
    workspaceFiles: [],
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
      expect(args[command === 'Enter' ? 6 : 8]).toEqual({
        displayText: 'history text',
        quotes: [],
        workspaceFiles: [],
      });
      expect(args.at(-1)).toBeUndefined();
      expect(textarea.value).toBe('history text');
      expectContext(container, false);
      expect(revoke).toHaveBeenCalledExactlyOnceWith('blob:draft-image');
    } finally { await unmount(root); }
  });

  it('opens three same-name persisted images by canonical path and reuses only the selected image tab', async () => {
    const images = [
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC',
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNg+M8AAAICAQB7CYF4AAAAAElFTkSuQmCC',
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGNgYPgPAAEDAQAIicLsAAAAAElFTkSuQmCC',
    ].map((data) => `data:image/png;base64,${data}`);
    const paths = ['a', 'b', 'c'].map((hash) => `user-inputs/${hash.repeat(32)}/image.png`);
    apiMocks.showConversationMessageAttachment.mockImplementation(async (...args: unknown[]) => {
      const index = paths.indexOf(args[7] as string);
      if (index < 0) throw new Error('Unexpected attachment path');
      const imageIndex = String(args[5]).endsWith('-other') ? (index + 1) % images.length : index;
      return { title: 'image.png', kind: 'message-attachment', content: images[imageIndex], metadata: { mimeType: 'image/png' } };
    });
    const events: AcpUiEventVm[] = paths.map((path, index) => ({
      id: `image-${index}`, seq: index + 1, timestamp: `178698000${index}Z`,
      kind: 'userTextDelta', content: '', status: 'completed',
      raw: { attachments: [{ name: 'image.png', path, type: 'image/png', size: 69 }] },
    }));
    const { container, root, id, session, render } = await renderPausedDialog({
      showAttachmentWorkspace: true,
      session: { events },
    });
    try {
      const thumbnails = Array.from(container.querySelectorAll<HTMLButtonElement>('[data-acp-attachment-row="images"] button[aria-label="image.png"]'));
      expect(thumbnails).toHaveLength(3);
      expect(thumbnails.map((button) => button.querySelector('img')?.getAttribute('src'))).toEqual(images);
      apiMocks.showConversationMessageAttachment.mockClear();
      for (const [index, thumbnail] of thumbnails.entries()) {
        await flushInteraction(() => thumbnail.click());
        expect(container.querySelector('[data-attachment-workspace-path]')?.getAttribute('data-attachment-workspace-path')).toBe(paths[index]);
        expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe(images[index]);
        expect(container.querySelectorAll('[data-right-workspace-tab]')).toHaveLength(index + 1);
        expect(apiMocks.showConversationMessageAttachment).toHaveBeenLastCalledWith(
          `project-${id}`, `task-${id}`, `run-${id}`, session.roundId, session.nodeId, session.attemptId,
          'image.png', paths[index], undefined, undefined,
        );
      }
      expect(apiMocks.showConversationMessageAttachment).toHaveBeenCalledTimes(3);
      await flushInteraction(() => thumbnails[0]!.click());
      expect(container.querySelectorAll('[data-right-workspace-tab]')).toHaveLength(3);
      expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe(images[0]);
      expect(apiMocks.showConversationMessageAttachment).toHaveBeenCalledTimes(4);
      const tabs = Array.from(container.querySelectorAll<HTMLElement>('[data-right-workspace-tab]'));
      for (const index of [2, 1, 0]) {
        await flushInteraction(() => tabs[index]!.querySelector('button')!.click());
        expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe(images[index]);
        expect(container.querySelectorAll('[data-right-workspace-tab]')).toHaveLength(3);
      }
      // The same canonical relative path in another attempt or branch is a
      // different workspace resource, never an alias of the original tab.
      const otherAttempt = { ...session, attemptId: `${session.attemptId}-other` };
      await render(pausedLifecycle(), otherAttempt, otherAttempt);
      await flushInteraction(() => container.querySelector<HTMLButtonElement>('[data-acp-attachment-row="images"] button')!.click());
      expect(container.querySelectorAll('[data-right-workspace-tab]')).toHaveLength(4);
      expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe(images[1]);
      expect(apiMocks.showConversationMessageAttachment).toHaveBeenLastCalledWith(
        `project-${id}`, `task-${id}`, `run-${id}`, session.roundId, session.nodeId, otherAttempt.attemptId,
        'image.png', paths[0], undefined, undefined,
      );
      const otherBranch = { ...session, branchId: 'branch-other' };
      await render(pausedLifecycle(), otherBranch);
      await flushInteraction(() => container.querySelector<HTMLButtonElement>('[data-acp-attachment-row="images"] button')!.click());
      expect(container.querySelectorAll('[data-right-workspace-tab]')).toHaveLength(5);
      expect(container.querySelector('[data-right-workspace-tab][data-state="active"]')?.getAttribute('data-right-workspace-resource-key')).toContain(':branch-other:');
      expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe(images[0]);
      await render(pausedLifecycle(), session);
      await flushInteraction(() => container.querySelector<HTMLButtonElement>('[data-acp-attachment-row="images"] button')!.click());
      expect(container.querySelectorAll('[data-right-workspace-tab]')).toHaveLength(5);
      expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe(images[0]);
    } finally { await unmount(root); }
  });

  it('never aliases a submitted same-name draft to an existing persisted attachment', async () => {
    const optimistic: AcpUiEventVm[] = [];
    const revoke = vi.fn();
    vi.stubGlobal('URL', class extends URL {
      static createObjectURL = vi.fn(() => 'blob:detached-draft');
      static revokeObjectURL = revoke;
    });
    // This is a real pre-existing locator, not an expected new storage path.
    // A later same-name draft must not claim it by guessing from the basename.
    const persisted: AcpUiEventVm = {
      id: 'earlier-attachment', seq: 1, timestamp: '1786980001Z',
      kind: 'userTextDelta', content: 'earlier image', status: 'completed',
      raw: { attachments: [{ name: 'draft.png', path: 'user-inputs/draft.png', type: 'image/png', size: 4 }] },
    };
    const { container, root, render, session } = await renderPausedDialog({
      initialDraft: contextDraft(),
      showAttachmentWorkspace: true,
      session: { events: [persisted] },
      onOptimisticEventsChange: (events) => optimistic.splice(0, optimistic.length, ...events),
    });
    apiMocks.submitConversationPrompt.mockResolvedValue({ kind: 'acp-session', session, lifecycle: pausedLifecycle() });
    try {
      const draftImage = container.querySelector<HTMLImageElement>('[data-composer-attachment-chip] img')!;
      expect(draftImage).not.toBeNull();
      await flushInteraction(() => draftImage.closest('button')!.click());
      expect(container.querySelector('[data-attachment-workspace-kind]')?.getAttribute('data-attachment-workspace-kind')).toBe('draft-attachment');
      expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe('blob:detached-draft');

      await flushInteraction(() => container.querySelector<HTMLButtonElement>('[data-acp-send]')!.click());
      const promptId = optimistic.map((event) => (event.raw as { promptId?: string })?.promptId).find(Boolean);
      expect(promptId).toBeTruthy();
      await render(pausedLifecycle(), { ...session, events: [persisted, {
        id: 'admitted-same-name', seq: 2, timestamp: '1786980002Z', kind: 'userTextDelta',
        content: 'cancelled draft', status: 'processing', raw: { source: 'goldBandPrompt', promptId },
      }] });
      expect(revoke).toHaveBeenCalledExactlyOnceWith('blob:draft-image');
      expect(revoke).not.toHaveBeenCalledWith('blob:detached-draft');

      const persistedButton = container.querySelector<HTMLButtonElement>('[data-acp-attachment-row="images"] button[aria-label="draft.png"]');
      expect(persistedButton).not.toBeNull();
      await flushInteraction(() => persistedButton!.click());
      expect(container.querySelector('[data-attachment-workspace-kind]')?.getAttribute('data-attachment-workspace-kind')).toBe('conversation-asset');
      expect(container.querySelector('[data-attachment-workspace-count]')?.getAttribute('data-attachment-workspace-count')).toBe('2');
      expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe('data:image/png;base64,AQIDBA==');
      const draftTab = container.querySelector<HTMLElement>('[data-right-workspace-resource-key^="draft-attachment:"]')!;
      await flushInteraction(() => draftTab.querySelector('button')!.click());
      expect(container.querySelector('[data-workspace-image-canvas] img')?.getAttribute('src')).toBe('blob:detached-draft');
      expect(revoke).not.toHaveBeenCalledWith('blob:detached-draft');
    } finally { await unmount(root); }
  });

  it('releases an accepted submission only once canonical admission arrives', async () => {
    const optimistic: AcpUiEventVm[] = [];
    const revoke = vi.fn();
    vi.stubGlobal('URL', class extends URL { static revokeObjectURL = revoke; });
    const { container, root, render, session } = await renderPausedDialog({
      initialDraft: contextDraft(),
      onOptimisticEventsChange: (events) => {
        optimistic.length = 0;
        optimistic.push(...events);
      },
    });
    apiMocks.submitConversationPrompt.mockResolvedValue({
      kind: 'acp-session',
      session,
      lifecycle: pausedLifecycle(),
    });
    try {
      await flushInteraction(() => container.querySelector<HTMLButtonElement>('[data-acp-send]')!.click());
      expect(container.querySelector('textarea')!.value).toBe('');
      expectContext(container, false);
      expect(revoke).not.toHaveBeenCalled();

      const promptId = optimistic
        .map((event) => (event.raw as { promptId?: string } | undefined)?.promptId)
        .find((value): value is string => Boolean(value));
      expect(promptId).toBeTruthy();
      const admitted: AcpUiEventVm = {
        id: 'admitted-prompt',
        seq: 9,
        timestamp: '1786980009Z',
        kind: 'userTextDelta',
        content: 'cancelled draft',
        status: 'processing',
        raw: { source: 'goldBandPrompt', promptId },
      };
      await render(pausedLifecycle(), { ...session, events: [admitted] });

      expect(revoke).toHaveBeenCalledExactlyOnceWith('blob:draft-image');
      expect(container.querySelector('textarea')!.value).toBe('');
    } finally { await unmount(root); }
  });

  it('reclaims the draft when the user stops while the prompt is still sending', async () => {
    const revoke = vi.fn();
    vi.stubGlobal('URL', class extends URL { static revokeObjectURL = revoke; });
    apiMocks.stopActiveSession.mockResolvedValue({
      status: 'accepted',
      session: null,
      lifecycle: runningLifecycle(),
    });
    let resolveSubmit!: (value: unknown) => void;
    apiMocks.submitConversationPrompt.mockImplementation(
      () => new Promise((resolve) => { resolveSubmit = resolve; }),
    );
    const { container, root } = await renderPausedDialog({ initialDraft: contextDraft() });
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea')!;
      expect(textarea.value).toBe('cancelled draft');
      expectContext(container, true);
      await flushInteraction(() => {
        container.querySelector<HTMLButtonElement>('[data-acp-send="true"]')!.click();
      });
      expect(textarea.value).toBe('');
      expectContext(container, false);

      const stop = container.querySelector('svg.lucide-circle-stop')!.closest('button');
      await flushInteraction(() => stop!.click());
      expect(apiMocks.stopActiveSession).toHaveBeenCalledTimes(1);
      expect(textarea.value).toBe('cancelled draft');
      expectContext(container, true);

      await act(async () => resolveSubmit({
        kind: 'acp-session-started',
        session: null,
        run: null,
        lifecycle: runningLifecycle(),
        admissionWasTerminal: false,
      }));
      expect(textarea.value).toBe('cancelled draft');
      expectContext(container, true);
      expect(revoke).not.toHaveBeenCalled();
    } finally {
      await unmount(root);
    }
  });

  it('restores the draft when an accepted prompt fails before canonical admission', async () => {
    const optimistic: AcpUiEventVm[] = [];
    const accepted = runningLifecycle();
    accepted.runtime = { ...accepted.runtime, revision: 1 };
    accepted.acp = { ...accepted.acp, revision: 1 };
    apiMocks.submitConversationPrompt.mockResolvedValue({
      kind: 'acp-session-started',
      session: null,
      run: null,
      lifecycle: accepted,
      admissionWasTerminal: false,
    });
    const { container, root, render } = await renderPausedDialog({
      onOptimisticEventsChange: (events) => {
        optimistic.length = 0;
        optimistic.push(...events);
      },
    });
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea')!;
      await setTextareaValue(textarea, 'config blocked draft');
      await flushInteraction(() => {
        container.querySelector<HTMLButtonElement>('[data-acp-send="true"]')!.click();
      });
      expect(textarea.value).toBe('');

      const promptId = optimistic
        .map((event) => (event.raw as { promptId?: string } | undefined)?.promptId)
        .find((value): value is string => Boolean(value));
      expect(promptId).toBeTruthy();

      const failed = runtimeAbnormalLifecycle();
      failed.runtime = { ...failed.runtime, revision: 2 };
      failed.acp = {
        ...failed.acp,
        revision: 2,
        turnId: promptId!,
        latestTurnStatus: 'failed',
        liveTurnActivity: 'idle',
        stopping: false,
      };
      await render(failed);
      expect(textarea.value).toBe('config blocked draft');
    } finally {
      await unmount(root);
    }
  });

  it.each([
    ['accepted', false],
    ['no-op', true],
  ] as const)('background Stop %s leaves the Stop button visible: %s', async (status, visibleAfter) => {
    apiMocks.stopActiveSession.mockResolvedValue({
      operationId: 'background-stop', status, kind: 'background-cancel-requested',
      run: null, session: null, lifecycle: null,
    });
    const completed = pausedLifecycle();
    completed.acp = { ...completed.acp, turnId: 'turn-a', latestTurnStatus: 'completed' };
    const backgroundControl = { sessionId: 'session', connectionGeneration: 1, activeTools: 1, expiresAtMs: Date.now() + 10_000 };
    completed.composer = { ...completed.composer, backgroundControl };
    const { container, root } = await renderPausedDialog({ initialLifecycle: completed, isOrchestrated: false, sessionStatus: 'completed' });
    const stopButton = () => container.querySelector('svg.lucide-circle-stop')?.closest('button') ?? null;
    try {
      expect(stopButton()).toBeTruthy();
      await flushInteraction(() => stopButton()!.click());
      expect(apiMocks.stopActiveSession).toHaveBeenCalledTimes(1);
      expect(apiMocks.stopActiveSession.mock.calls[0][9]).toEqual(backgroundControl);
      expect(Boolean(stopButton())).toBe(visibleAfter);
    } finally {
      await unmount(root);
    }
  });

  it('keeps an accepted draft consumed when its turn completes', async () => {
    const optimistic: AcpUiEventVm[] = [];
    const revoke = vi.fn();
    vi.stubGlobal('URL', class extends URL { static revokeObjectURL = revoke; });
    const accepted = runningLifecycle();
    accepted.runtime = { ...accepted.runtime, revision: 1 };
    accepted.acp = { ...accepted.acp, revision: 1 };
    apiMocks.submitConversationPrompt.mockResolvedValue({
      kind: 'acp-session-started',
      session: null,
      run: null,
      lifecycle: accepted,
      admissionWasTerminal: false,
    });
    const { container, root, render } = await renderPausedDialog({
      initialDraft: contextDraft(),
      onOptimisticEventsChange: (events) => {
        optimistic.length = 0;
        optimistic.push(...events);
      },
    });
    try {
      const textarea = container.querySelector<HTMLTextAreaElement>('textarea')!;
      await flushInteraction(() => {
        container.querySelector<HTMLButtonElement>('[data-acp-send="true"]')!.click();
      });
      expect(textarea.value).toBe('');

      const promptId = optimistic
        .map((event) => (event.raw as { promptId?: string } | undefined)?.promptId)
        .find((value): value is string => Boolean(value));
      const completed = pausedLifecycle();
      completed.runtime = { ...completed.runtime, revision: 2 };
      completed.acp = {
        ...completed.acp,
        revision: 2,
        turnId: promptId!,
        latestTurnStatus: 'completed',
        liveTurnActivity: 'idle',
        stopping: false,
      };
      await render(completed);

      expect(textarea.value).toBe('');
      expect(revoke).toHaveBeenCalledExactlyOnceWith('blob:draft-image');
    } finally {
      await unmount(root);
    }
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
        { displayText: '继续并补充测试', quotes: [], workspaceFiles: [] },
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

describe('ACP error banner ownership', () => {
  it.each(['starting', 'running', 'completed'] as const)(
    'hides the old Workflow pause error during a manual follow-up (%s)',
    async (phase) => {
      const initial = runtimeAbnormalLifecycle();
      const recovered = {
        ...initial,
        control: { mode: 'non-runtime-controlled' as const, transitionCause: 'manual-follow-up' as const },
        acp: {
          ...initial.acp,
          revision: 2,
          turnId: 'manual-follow-up',
          operationId: 'follow-up-operation',
          liveTurnActivity: phase === 'completed' ? 'idle' as const : phase,
          latestTurnStatus: phase === 'completed' ? 'completed' as const : 'none' as const,
          turnError: null,
        },
        displayStatus: phase,
        runtimeDisplay: { ...initial.runtimeDisplay, code: phase, tone: phase === 'completed' ? 'neutral' : 'running' },
      };
      const result = await renderPausedDialog({
        initialLifecycle: initial,
        isOrchestrated: true,
        runtimeError: 'old Workflow provider failure',
        sessionStatus: 'failed',
        session: readySessionHistory(),
      });
      try {
        expect(result.container.textContent).toContain('old Workflow provider failure');
        await result.render(recovered, { ...result.session, status: phase });
        expect(result.container.textContent).not.toContain('old Workflow provider failure');
        expect(recovered.runtime.status).toBe('paused');
        expect(recovered.runtime.pauseReason).toBe('runtime-abnormal');
        expect(apiMocks.continueConversationRuntime).not.toHaveBeenCalled();
      } finally {
        await unmount(result.root);
      }
    },
  );

  it('keeps the current follow-up failure instead of the old Workflow error', async () => {
    const lifecycle = runtimeAbnormalLifecycle();
    lifecycle.control.transitionCause = 'manual-follow-up';
    lifecycle.acp = {
      ...lifecycle.acp, turnId: 'new-failed-turn', operationId: 'new-operation',
      turnError: {
        code: { domain: 'internal', code: 'internal.unknown' },
        domain: 'internal', recovery: 'manual', diagnostic: 'Current follow-up failed',
      },
    };
    const result = await renderPausedDialog({
      initialLifecycle: lifecycle, isOrchestrated: true, sessionStatus: 'failed',
      runtimeError: 'old Workflow provider failure', session: readySessionHistory(),
    });
    try {
      expect(result.container.textContent).toContain('Current follow-up failed');
      expect(result.container.textContent).not.toContain('old Workflow provider failure');
    } finally {
      await unmount(result.root);
    }
  });

  it('dismisses only the current banner without changing lifecycle or submitting a command', async () => {
    const lifecycle = runtimeAbnormalLifecycle();
    lifecycle.acp = {
      ...lifecycle.acp, revision: 3, turnId: 'failed-turn', operationId: 'failed-operation',
      turnError: {
        code: { domain: 'internal', code: 'internal.unknown' },
        domain: 'internal', recovery: 'manual', diagnostic: 'Dismissible provider failure',
      },
    };
    const before = structuredClone(lifecycle);
    const result = await renderPausedDialog({
      initialLifecycle: lifecycle, sessionStatus: 'failed', session: readySessionHistory(),
    });
    try {
      const close = result.container.querySelector<HTMLButtonElement>(`button[aria-label="${i18n.t('common.close')}"]`);
      expect(close).not.toBeNull();
      await act(async () => close!.click());
      expect(result.container.textContent).not.toContain('Dismissible provider failure');
      await result.render({ ...lifecycle, acp: { ...lifecycle.acp, revision: 4 } });
      expect(result.container.textContent).not.toContain('Dismissible provider failure');
      expect(lifecycle).toEqual(before);
      expect(apiMocks.continueConversationRuntime).not.toHaveBeenCalled();
      expect(apiMocks.submitConversationPrompt).not.toHaveBeenCalled();
      expect(apiMocks.stopActiveSession).not.toHaveBeenCalled();
    } finally {
      await unmount(result.root);
    }
  });

  it.each(['turn', 'operation', 'session', 'scope'] as const)(
    'shows identical error text again after the banner %s identity changes',
    async (identity) => {
      const lifecycle = runtimeAbnormalLifecycle();
      lifecycle.acp = {
        ...lifecycle.acp, revision: 3, turnId: 'failed-turn', operationId: 'failed-operation',
        turnError: {
          code: { domain: 'internal', code: 'internal.unknown' },
          domain: 'internal', recovery: 'manual', diagnostic: 'Repeated provider failure',
        },
      };
      const result = await renderPausedDialog({
        initialLifecycle: lifecycle, sessionStatus: 'failed', session: readySessionHistory(),
      });
      try {
        const close = result.container.querySelector<HTMLButtonElement>(`button[aria-label="${i18n.t('common.close')}"]`);
        expect(close).not.toBeNull();
        await act(async () => close!.click());
        expect(result.container.textContent).not.toContain('Repeated provider failure');
        const nextSession = {
          ...result.session,
          sessionId: identity === 'session' ? 'another-session' : result.session.sessionId,
          attemptId: identity === 'scope' ? 'another-attempt' : result.session.attemptId,
        };
        await result.render({
          ...lifecycle,
          acp: {
            ...lifecycle.acp, revision: 4,
            turnId: identity === 'turn' ? 'new-turn' : lifecycle.acp.turnId,
            operationId: identity === 'operation' ? 'new-operation' : lifecycle.acp.operationId,
          },
        }, nextSession, nextSession);
        expect(result.container.textContent).toContain('Repeated provider failure');
      } finally {
        await unmount(result.root);
      }
    },
  );

  it('keeps a dismissed runtime error hidden across ACP metadata updates but not a new runtime failure', async () => {
    const lifecycle = runtimeAbnormalLifecycle();
    lifecycle.control.mode = 'runtime-controlled';
    lifecycle.runtime.revision = 7;
    const result = await renderPausedDialog({
      initialLifecycle: lifecycle, runtimeError: 'Runtime-owned failure', sessionStatus: 'failed',
    });
    try {
      const close = result.container.querySelector<HTMLButtonElement>(`button[aria-label="${i18n.t('common.close')}"]`)!;
      expect(result.container.textContent).toContain('Runtime-owned failure');
      await act(async () => close.click());
      const refreshed = { ...lifecycle, acp: { ...lifecycle.acp, revision: 100 } };
      await result.render(refreshed);
      expect(result.container.textContent).not.toContain('Runtime-owned failure');
      await result.render({ ...refreshed, runtime: { ...lifecycle.runtime, revision: 8 } });
      expect(result.container.textContent).toContain('Runtime-owned failure');
    } finally {
      await unmount(result.root);
    }
  });

  it('allows an unidentified failure to reappear after it has cleared', async () => {
    const lifecycle = runtimeAbnormalLifecycle();
    lifecycle.acp.turnError = {
      code: { domain: 'internal', code: 'internal.unknown' },
      domain: 'internal', recovery: 'manual', diagnostic: 'Unidentified failure',
    };
    const result = await renderPausedDialog({ initialLifecycle: lifecycle, sessionStatus: 'failed' });
    try {
      const close = result.container.querySelector<HTMLButtonElement>(`button[aria-label="${i18n.t('common.close')}"]`)!;
      await act(async () => close.click());
      expect(result.container.textContent).not.toContain('Unidentified failure');
      await result.render({ ...lifecycle, acp: { ...lifecycle.acp, latestTurnStatus: 'completed', turnError: null } });
      await result.render(lifecycle);
      expect(result.container.textContent).toContain('Unidentified failure');
    } finally {
      await unmount(result.root);
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
      initialLifecycle: runtimeAbnormalLifecycle(),
      isOrchestrated: false,
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
        { displayText: '首轮停止后的下一句', quotes: [], workspaceFiles: [] },
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
        { displayText: '边界消息只发送一次', quotes: [], workspaceFiles: [] },
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
