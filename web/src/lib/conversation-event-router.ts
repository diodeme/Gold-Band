import type { AcpSessionUpdatedEventVm } from '@/api/client';
import { mergeAcpExecutionError } from '@/lib/acp-execution-error';
import { getRuntimeApi } from '@/api/client';
import type {
  AcpAgentExecutionVm,
  AcpSessionVm,
  AcpUiEventVm,
  ConversationAttemptLifecycleVm,
} from '@/types';
import { useCallback, useMemo, useRef, useSyncExternalStore } from 'react';
import {
  recordAcpStreamingDiagnostic,
  summarizeAcpStreamingEvent,
} from '@/lib/acp-streaming-diagnostics';
import {
  mergeConversationAcpFacet,
  mergeConversationPromptQueue,
} from '@/lib/acp-runtime-composer-state';

type Listener = (event: AcpSessionUpdatedEventVm) => void;

const listeners = new Set<Listener>();
const attemptListeners = new Map<string, Set<Listener>>();
const branchSnapshots = new Map<string, ConversationBranchLiveSnapshot>();
const branchReplayBuffers = new Map<string, ConversationBranchReplayBuffer>();
const branchListeners = new Map<string, Set<() => void>>();
const attemptBranchListeners = new Map<string, Set<() => void>>();
const attemptBranchVersions = new Map<string, number>();
/** Replay buffers hold transcript content, so they are evicted per branch. */
const branchReplayOrder: string[] = [];
const MAX_REPLAY_BRANCHES = 64;
/**
 * Branch snapshots hold lifecycle facts newer than any fetched projection, so
 * they are evicted only as a whole attempt nobody observes: an observed view
 * never falls back to a stale projected status, and a reopened attempt fetches
 * a fresh projection.
 */
const snapshotAttemptOrder: string[] = [];
const MAX_SNAPSHOT_ATTEMPTS = 16;
const MAX_REPLAY_EVENTS_PER_BRANCH = 64;
const MAX_REPLAY_BYTES_PER_BRANCH = 512 * 1024;
const MAX_REPLAY_EVENT_BYTES = 256 * 1024;
const MAX_REPLAY_BYTES_GLOBAL = 4 * 1024 * 1024;
const ROUTER_RETRY_BASE_MS = 250;
const ROUTER_RETRY_MAX_MS = 5_000;
let retainedReplayBytes = 0;
let started = false;
let starting: Promise<void> | null = null;
let retryTimer: ReturnType<typeof globalThis.setTimeout> | null = null;
let retryAttempt = 0;

interface RetainedConversationEvent {
  event: AcpUiEventVm;
  timelineGeneration: number;
  timelineRevision: number | null;
  generation: number;
  estimatedBytes: number;
}

interface ConversationBranchReplayBuffer {
  sessionId: string | null;
  generation: number;
  headSeq: number;
  timelineGeneration: number;
  headRevision: number;
  lossWatermarkGeneration: number;
  lossWatermarkRevision: number;
  lossWatermarkSeq: number;
  lossWatermarkRouterGeneration: number;
  retainedBytes: number;
  events: Map<string, RetainedConversationEvent>;
  order: string[];
}

export interface ConversationBranchReplaySnapshot {
  sessionId: string | null;
  generation: number;
  headSeq: number;
  timelineGeneration: number;
  headRevision: number;
  lossWatermarkGeneration: number;
  lossWatermarkRevision: number;
  lossWatermarkSeq: number;
  lossWatermarkRouterGeneration: number;
  requiresCatchUp: boolean;
  retainedBytes: number;
  events: AcpUiEventVm[];
}

export const CONVERSATION_EVENT_REPLAY_LIMITS = {
  branchCount: MAX_REPLAY_BRANCHES,
  snapshotAttempts: MAX_SNAPSHOT_ATTEMPTS,
  eventsPerBranch: MAX_REPLAY_EVENTS_PER_BRANCH,
  bytesPerBranch: MAX_REPLAY_BYTES_PER_BRANCH,
  eventBytes: MAX_REPLAY_EVENT_BYTES,
  globalBytes: MAX_REPLAY_BYTES_GLOBAL,
} as const;

function hasConversationEventRouterSubscribers() {
  return listeners.size > 0
    || attemptListeners.size > 0
    || branchListeners.size > 0
    || attemptBranchListeners.size > 0;
}

function clearConversationEventRouterRetry(resetAttempt = false) {
  if (retryTimer !== null) {
    globalThis.clearTimeout(retryTimer);
    retryTimer = null;
  }
  if (resetAttempt) retryAttempt = 0;
}

function stopConversationEventRouterRetryWhenIdle() {
  if (hasConversationEventRouterSubscribers()) return;
  clearConversationEventRouterRetry(true);
}

function recordConversationEventRouterSubscriptionError(
  error: unknown,
  retryDelayMs: number,
) {
  try {
    console.error('[Gold Band] Conversation event router subscription failed', {
      error,
      retryAttempt,
      retryDelayMs,
    });
  } catch {
    // Diagnostics must not interfere with subscription recovery.
  }
}

function scheduleConversationEventRouterRetry(error: unknown) {
  if (
    started
    || retryTimer !== null
    || !hasConversationEventRouterSubscribers()
  ) return;
  const retryDelayMs = Math.min(
    ROUTER_RETRY_BASE_MS * (2 ** Math.min(retryAttempt, 5)),
    ROUTER_RETRY_MAX_MS,
  );
  retryAttempt += 1;
  recordConversationEventRouterSubscriptionError(error, retryDelayMs);
  retryTimer = globalThis.setTimeout(() => {
    retryTimer = null;
    requestConversationEventRouterStart();
  }, retryDelayMs);
}

function startConversationEventRouterNativeSubscription() {
  const subscribe = getRuntimeApi().subscribeAcpSessionUpdates;
  if (!subscribe) return Promise.resolve();
  return subscribe((event) => {
    recordAcpStreamingDiagnostic(
      'router-received',
      () => summarizeAcpStreamingEvent(event),
    );
    applyConversationEventToBranchSnapshots(event);
    const projectedEvent = conversationEventForListenerProjection(event);
    notifyConversationEventListeners(
      attemptListeners.get(attemptKey(event)) ?? [],
      projectedEvent,
      'attempt',
    );
    notifyConversationEventListeners(listeners, projectedEvent, 'global');
  }).then(() => undefined);
}

function requestConversationEventRouterStart(startImmediately = false) {
  if (started) return Promise.resolve();
  if (starting) return starting;
  if (retryTimer !== null) {
    if (!startImmediately) return null;
    clearConversationEventRouterRetry();
  }
  const attempt = startConversationEventRouterNativeSubscription();
  starting = attempt;
  void attempt.then(
    () => {
      started = true;
      clearConversationEventRouterRetry(true);
    },
    (error) => {
      scheduleConversationEventRouterRetry(error);
    },
  ).finally(() => {
    if (starting === attempt) starting = null;
  });
  return attempt;
}

function conversationEventForListenerProjection(
  event: AcpSessionUpdatedEventVm,
): AcpSessionUpdatedEventVm {
  if (!event.event || isValidConversationTimelineGeneration(event.timelineGeneration)) {
    return event;
  }
  const {
    event: _invalidEvent,
    timelineGeneration: _invalidGeneration,
    timelineRevision: _invalidRevision,
    ...controlEnvelope
  } = event;
  return {
    ...controlEnvelope,
    timelineRecoveryRequired: true,
  } as AcpSessionUpdatedEventVm;
}

function notifyConversationEventListeners(
  targetListeners: Iterable<Listener>,
  event: AcpSessionUpdatedEventVm,
  scope: 'attempt' | 'global',
) {
  for (const listener of targetListeners) {
    try {
      listener(event);
    } catch (error) {
      recordConversationEventListenerError(scope, event, error);
    }
  }
}

function recordConversationEventListenerError(
  scope: 'attempt' | 'global',
  event: AcpSessionUpdatedEventVm,
  error: unknown,
) {
  try {
    console.error('[Gold Band] Conversation event listener failed', {
      scope,
      attemptKey: attemptKey(event),
      branchId: conversationEventBranchId(event),
      eventKind: event.event?.kind ?? null,
      eventId: event.event?.id ?? null,
      error,
    });
  } catch {
    // Diagnostics must not interfere with the remaining event listeners.
  }
}

export async function ensureConversationEventRouterStarted() {
  await requestConversationEventRouterStart(true);
}

export interface ConversationBranchLiveSnapshot {
  executionError: ConversationAttemptLifecycleVm['executionError'];
  revision: number;
  contentRevision: number;
  acpRevision: number;
  status: string | null;
  providerStateSeq?: number | null;
  attention: boolean;
  acp: ConversationAttemptLifecycleVm['acp'] | null;
  promptQueue: ConversationAttemptLifecycleVm['promptQueue'];
}

const EMPTY_BRANCH_SNAPSHOT: ConversationBranchLiveSnapshot = {
  executionError: null,
  revision: 0,
  contentRevision: 0,
  acpRevision: 0,
  status: null,
  attention: false,
  acp: null,
  promptQueue: null,
};

export interface ConversationAttemptLocator {
  projectId?: string | null;
  taskId: string;
  taskUuid?: string | null;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
}

function attemptKey(locator: ConversationAttemptLocator) {
  const taskUuid = locator.taskUuid?.trim();
  const taskIdentity = taskUuid
    ? ['taskUuid', taskUuid]
    : ['taskId', locator.taskId];
  return JSON.stringify([
    locator.projectId ?? null,
    taskIdentity,
    locator.runId,
    locator.roundId,
    locator.nodeId,
    locator.attemptId,
    locator.outerNodeId ?? null,
    locator.outerAttemptId ?? null,
  ]);
}

export function conversationAttemptStoreKey(locator: ConversationAttemptLocator) {
  return attemptKey(locator);
}

export function conversationBranchStoreKey(locator: Parameters<typeof attemptKey>[0], branchId: string) {
  return `${attemptKey(locator)}:${branchId}`;
}

function normalizeConversationSessionId(sessionId: string | null | undefined) {
  if (typeof sessionId !== 'string') return null;
  const normalized = sessionId.trim();
  return normalized.length > 0 ? normalized : null;
}

function createConversationBranchReplayBuffer(
  sessionId: string | null,
): ConversationBranchReplayBuffer {
  return {
    sessionId,
    generation: 0,
    headSeq: 0,
    timelineGeneration: 0,
    headRevision: 0,
    lossWatermarkGeneration: 0,
    lossWatermarkRevision: 0,
    lossWatermarkSeq: 0,
    lossWatermarkRouterGeneration: 0,
    retainedBytes: 0,
    events: new Map<string, RetainedConversationEvent>(),
    order: [],
  };
}

function resetConversationBranchReplayOwner(
  key: string,
  buffer: ConversationBranchReplayBuffer,
  sessionId: string,
) {
  clearRetainedEvents(buffer);
  buffer.sessionId = sessionId;
  buffer.generation = 0;
  buffer.headSeq = 0;
  buffer.timelineGeneration = 0;
  buffer.headRevision = 0;
  buffer.lossWatermarkGeneration = 0;
  buffer.lossWatermarkRevision = 0;
  buffer.lossWatermarkSeq = 0;
  buffer.lossWatermarkRouterGeneration = 0;
  branchSnapshots.set(key, EMPTY_BRANCH_SNAPSHOT);
}

function reconcileConversationReplaySessionOwner(
  locator: ConversationAttemptLocator,
  sessionId: string | null | undefined,
  requiredBranchIds: Iterable<string>,
  reconcileAttempt = false,
) {
  const normalizedSessionId = normalizeConversationSessionId(sessionId);
  const prefix = `${attemptKey(locator)}:`;
  const requiredKeys = [...requiredBranchIds].map((branchId) => `${prefix}${branchId}`);
  const resetKeys = new Set<string>();
  const requiredOwnerMismatch = normalizedSessionId != null
    && requiredKeys.some((key) => (
      branchReplayBuffers.get(key)?.sessionId !== normalizedSessionId
    ));
  if (normalizedSessionId && (reconcileAttempt || requiredOwnerMismatch)) {
    for (const [key, buffer] of branchReplayBuffers) {
      if (!key.startsWith(prefix)) continue;
      if (buffer.sessionId && buffer.sessionId !== normalizedSessionId) {
        resetConversationBranchReplayOwner(key, buffer, normalizedSessionId);
        resetKeys.add(key);
      } else if (!buffer.sessionId) {
        buffer.sessionId = normalizedSessionId;
      }
    }
  }
  for (const key of requiredKeys) {
    const buffer = branchReplayBuffers.get(key);
    if (!buffer) {
      branchReplayBuffers.set(
        key,
        createConversationBranchReplayBuffer(normalizedSessionId),
      );
      continue;
    }
    if (
      normalizedSessionId
      && buffer.sessionId
      && buffer.sessionId !== normalizedSessionId
    ) {
      resetConversationBranchReplayOwner(key, buffer, normalizedSessionId);
      resetKeys.add(key);
    } else if (normalizedSessionId && !buffer.sessionId) {
      buffer.sessionId = normalizedSessionId;
    }
  }
  return resetKeys;
}

function notifyConversationSessionOwnerResets(resetKeys: Iterable<string>) {
  for (const key of resetKeys) notifyBranch(key);
}

export function applyConversationEventToBranchSnapshots(event: AcpSessionUpdatedEventVm) {
  if (event.executionError) {
    const key = conversationBranchStoreKey(event, 'root');
    const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
    const failure = mergeAcpExecutionError(current.acp, event.executionError, current.executionError);
    if (failure && failure !== current.executionError) {
      storeBranchSnapshot(key, { ...current, revision: current.revision + 1, executionError: failure, attention: true });
      notifyBranch(key);
    }
  }
  const timelineGeneration = isValidConversationTimelineGeneration(event.timelineGeneration)
    ? event.timelineGeneration
    : null;
  const timelineEvent = event.event
    && timelineGeneration !== null
    ? event.event
    : null;
  const eventBranchId = timelineEvent ? conversationEventBranchId(event) : null;
  const resetKeys = timelineEvent
    ? reconcileConversationReplaySessionOwner(
        event,
        timelineEvent.sessionId,
        eventBranchId ? [eventBranchId] : [],
      )
    : new Set<string>();
  try {
    if (event.lifecycle) reconcileConversationBranchLifecycle(event, event.lifecycle);
    if (timelineEvent && timelineGeneration !== null) {
      const key = conversationBranchStoreKey(event, eventBranchId ?? 'root');
      const accepted = retainConversationEvent(
        key,
        timelineEvent,
        timelineGeneration,
        event.timelineRevision ?? null,
      );
      if (!accepted) return;
      const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
      if (timelineEvent.kind === 'agentState') {
        if (isNativeAgentState(timelineEvent) && timelineEvent.status) {
          if (conversationEventPosition(timelineEvent) <= (current.providerStateSeq ?? -1)) return;
          updateAgentBranchSnapshot(
            key,
            timelineEvent.status,
            timelineEvent.status === 'waiting_permission',
            conversationEventPosition(timelineEvent),
          );
        }
        return;
      }
      if (current.providerStateSeq != null) {
        const request = timelineEvent.kind === 'permissionRequest' || timelineEvent.kind === 'elicitationRequest';
        const response = timelineEvent.kind === 'elicitationResponse';
        const pending = request && (timelineEvent.status ?? 'pending') === 'pending';
        const newerInteraction = conversationEventPosition(timelineEvent) > current.providerStateSeq;
        updateBranchSnapshot(
          key,
          current.status,
          newerInteraction && (request || response) && !isTerminalBranchStatus(current.status)
            ? pending
            : current.attention,
          true,
        );
        return;
      }
      if (isSyntheticAgentPrompt(timelineEvent)) {
        updateBranchSnapshot(key, current.status, current.attention, true);
        return;
      }
      if (isAgentBranchResult(timelineEvent)) {
        updateBranchSnapshot(key, 'completed', false, true);
        return;
      }
      const request = timelineEvent.kind === 'permissionRequest' || timelineEvent.kind === 'elicitationRequest';
      const response = timelineEvent.kind === 'elicitationResponse';
      const interaction = request || response;
      const pending = request && (timelineEvent.status ?? 'pending') === 'pending';
      const terminal = isTerminalBranchStatus(current.status);
      const status = terminal
        ? current.status
        : pending
          ? 'waiting_permission'
          : 'running';
      updateBranchSnapshot(
        key,
        status,
        terminal ? current.attention : interaction ? pending : current.attention,
        true,
      );
      return;
    }
    if (event.session) {
      reconcileConversationBranchSession(event, event.session);
    }
  } finally {
    notifyConversationSessionOwnerResets(resetKeys);
  }
}

export function reconcileConversationBranchSession(
  locator: ConversationAttemptLocator,
  session: AcpSessionVm,
) {
  const prefix = `${attemptKey(locator)}:`;
  const branchId = session.branchId || 'root';
  const branchKey = `${prefix}${branchId}`;
  const branchExecution = session.branchExecution;
  const projectedAgents = session.timelineProjection?.agents ?? [];
  const resetKeys = reconcileConversationReplaySessionOwner(
    locator,
    session.sessionId,
    [branchId, ...projectedAgents.map((agent) => agent.agentExecutionId)],
    true,
  );
  try {
    if (branchId === 'root') {
      updateBranchSnapshot(
        branchKey,
        session.status,
        (branchSnapshots.get(branchKey) ?? EMPTY_BRANCH_SNAPSHOT).attention,
      );
    } else {
      const projectedStatus = branchExecution?.executionStatus ?? session.status;
      const authoritativeStatus = branchExecution?.providerStateSeq == null
        && isTerminalSessionStatus(session.status)
        && !isTerminalBranchStatus(projectedStatus)
        ? session.status
        : projectedStatus;
      updateAgentBranchSnapshot(
        branchKey,
        authoritativeStatus,
        branchExecution?.hasAttention ?? false,
        branchExecution?.providerStateSeq,
      );
    }

    const projectedBranchKeys = new Set<string>();
    const branchTerminal = isTerminalSessionStatus(session.status);
    for (const agent of projectedAgents) {
      const key = `${prefix}${agent.agentExecutionId}`;
      projectedBranchKeys.add(key);
      updateAgentBranchSnapshot(
        key,
        branchTerminal && agent.providerStateSeq == null && !isTerminalBranchStatus(agent.executionStatus)
          ? 'interrupted'
          : agent.executionStatus,
        branchTerminal && agent.providerStateSeq == null ? false : agent.hasAttention,
        agent.providerStateSeq,
      );
    }

    if (branchId !== 'root' || !isTerminalSessionStatus(session.status)) return;
    const rootKey = `${prefix}root`;
    for (const [key, current] of branchSnapshots) {
      if (!key.startsWith(prefix) || key === rootKey || projectedBranchKeys.has(key)) continue;
      if (current.providerStateSeq != null || isTerminalBranchStatus(current.status)) continue;
      updateBranchSnapshot(key, 'interrupted', false);
    }
  } finally {
    notifyConversationSessionOwnerResets(resetKeys);
  }
}

function reconcileConversationBranchLifecycle(
  event: AcpSessionUpdatedEventVm,
  lifecycle: ConversationAttemptLifecycleVm,
) {
  const prefix = `${attemptKey(event)}:`;
  const rootKey = `${prefix}root`;
  const rootCurrent = branchSnapshots.get(rootKey) ?? EMPTY_BRANCH_SNAPSHOT;
  const acp = mergeConversationAcpFacet(rootCurrent.acp, lifecycle.acp);
  const promptQueue = mergeConversationPromptQueue(rootCurrent.promptQueue, lifecycle.promptQueue);
  const status = branchStatusFromAcp(acp) ?? rootCurrent.status;
  updateBranchSnapshot(
    rootKey,
    status,
    rootCurrent.attention,
    false,
    { acp, promptQueue, executionError: lifecycle.executionError },
  );
  if (!status || !isTerminalSessionStatus(status)) return;
  for (const [key, current] of branchSnapshots) {
    if (!key.startsWith(prefix) || key === rootKey || current.providerStateSeq != null || isTerminalBranchStatus(current.status)) continue;
    updateBranchSnapshot(key, 'interrupted', false);
  }
}

function branchStatusFromAcp(
  acp: ConversationAttemptLifecycleVm['acp'] | null | undefined,
) {
  if (!acp) return null;
  switch (acp.liveTurnActivity) {
    case 'starting': return 'pending';
    case 'accepted':
    case 'running': return 'running';
    case 'cancel-requested': return 'cancelling';
    case 'idle':
      switch (acp.latestTurnStatus) {
        case 'completed': return 'completed';
        case 'cancelled': return 'cancelled';
        case 'failed': return 'failed';
        default: return null;
      }
  }
}

export function resolveConversationBranchDisplayStatus(
  persistedStatus: string | null | undefined,
  liveStatus: string | null | undefined,
  persistedProviderStateSeq?: number | null,
  liveProviderStateSeq?: number | null,
) {
  if (persistedProviderStateSeq !== liveProviderStateSeq
    && (persistedProviderStateSeq != null || liveProviderStateSeq != null)) {
    return (liveProviderStateSeq ?? -1) > (persistedProviderStateSeq ?? -1)
      ? liveStatus ?? persistedStatus ?? null
      : persistedStatus ?? liveStatus ?? null;
  }
  if (isTerminalBranchStatus(persistedStatus ?? null)) return persistedStatus ?? null;
  if (isTerminalBranchStatus(liveStatus ?? null)) return liveStatus ?? null;
  return liveStatus ?? persistedStatus ?? null;
}

function isNativeAgentState(event: AcpUiEventVm) {
  if (!event.raw || typeof event.raw !== 'object' || Array.isArray(event.raw)) return false;
  return (event.raw as Record<string, unknown>).source === 'nativeSubagentState';
}

function isSyntheticAgentPrompt(event: AcpSessionUpdatedEventVm['event']) {
  if (!event?.raw || typeof event.raw !== 'object' || Array.isArray(event.raw)) return false;
  return (event.raw as Record<string, unknown>).source === 'agentBranchPrompt';
}

function isAgentBranchResult(event: AcpSessionUpdatedEventVm['event']) {
  if (!event?.raw || typeof event.raw !== 'object' || Array.isArray(event.raw)) return false;
  return (event.raw as Record<string, unknown>).source === 'agentBranchResult';
}

function updateBranchSnapshot(
  key: string,
  status: string | null,
  attention: boolean,
  contentChanged = false,
  control?: {
    acp: ConversationAttemptLifecycleVm['acp'];
    promptQueue: ConversationAttemptLifecycleVm['promptQueue'];
    executionError?: ConversationAttemptLifecycleVm['executionError'];
  },
  providerStateSeq?: number | null,
) {
  const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
  const nextProviderStateSeq = providerStateSeq ?? current.providerStateSeq;
  const nextAcp = control?.acp ?? current.acp;
  const nextPromptQueue = control?.promptQueue ?? current.promptQueue;
  const executionError = mergeAcpExecutionError(nextAcp, control?.executionError, current.executionError);
  const nextAcpRevision = nextAcp?.revision ?? current.acpRevision;
  if (
    current.status === status
    && current.providerStateSeq === nextProviderStateSeq
    && current.attention === attention
    && current.acpRevision === nextAcpRevision
    && current.acp === nextAcp
    && current.promptQueue === nextPromptQueue
    && current.executionError === executionError
  ) {
    if (contentChanged) {
      storeBranchSnapshot(key, {
        ...current,
        contentRevision: current.contentRevision + 1,
      });
    }
    return;
  }
  storeBranchSnapshot(key, {
    revision: current.revision + 1,
    contentRevision: current.contentRevision + Number(contentChanged),
    acpRevision: nextAcpRevision,
    status,
    providerStateSeq: nextProviderStateSeq,
    attention,
    acp: nextAcp,
    executionError,
    promptQueue: nextPromptQueue,
  });
  notifyBranch(key);
}

function updateAgentBranchSnapshot(
  key: string,
  status: string,
  attention: boolean,
  providerStateSeq?: number | null,
) {
  const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
  if (providerStateSeq != null || current.providerStateSeq != null) {
    if (providerStateSeq == null || providerStateSeq < (current.providerStateSeq ?? -1)) return;
    // State sequence orders lifecycle, not ancestor attention: a descendant
    // permission can change snapshot attention without a new parent state event.
    // Explicit close may settle at the same sequence, but cannot revive it.
    if (providerStateSeq === current.providerStateSeq) {
      const settledStatus = isTerminalBranchStatus(current.status)
        ? current.status
        : isTerminalBranchStatus(status) ? status : current.status ?? status;
      updateBranchSnapshot(key, settledStatus, isTerminalBranchStatus(settledStatus) ? false : attention);
      return;
    }
    updateBranchSnapshot(key, status, isTerminalBranchStatus(status) ? false : attention, false, undefined, providerStateSeq);
    return;
  }
  if (isTerminalBranchStatus(current.status) && !isTerminalBranchStatus(status)) {
    updateBranchSnapshot(key, current.status, current.attention);
    return;
  }
  updateBranchSnapshot(key, status, isTerminalBranchStatus(status) ? false : attention);
}

function isTerminalBranchStatus(status: string | null) {
  return status != null && ['completed', 'failed', 'cancelled', 'canceled', 'interrupted', 'stopped'].includes(status.toLowerCase());
}

function isTerminalSessionStatus(status: string) {
  return ['completed', 'failed', 'cancelled', 'canceled', 'interrupted', 'stopped'].includes(status.toLowerCase());
}

function storeBranchSnapshot(key: string, snapshot: ConversationBranchLiveSnapshot) {
  branchSnapshots.set(key, snapshot);
  touchRecent(branchReplayOrder, key);
  while (branchReplayOrder.length > MAX_REPLAY_BRANCHES) {
    const oldest = branchReplayOrder.shift();
    if (!oldest) break;
    deleteBranchReplayBuffer(oldest);
  }
  touchRecent(snapshotAttemptOrder, attemptKeyOfBranchKey(key));
  if (snapshotAttemptOrder.length > MAX_SNAPSHOT_ATTEMPTS) evictUnobservedSnapshotAttempt();
}

function touchRecent(order: string[], key: string) {
  const existing = order.indexOf(key);
  if (existing >= 0) order.splice(existing, 1);
  order.push(key);
}

function isAttemptObserved(attempt: string) {
  if (attemptListeners.has(attempt) || attemptBranchListeners.has(attempt)) return true;
  const prefix = `${attempt}:`;
  for (const key of branchListeners.keys()) {
    if (key.startsWith(prefix)) return true;
  }
  return false;
}

function evictUnobservedSnapshotAttempt() {
  const index = snapshotAttemptOrder.findIndex((attempt) => !isAttemptObserved(attempt));
  if (index < 0) return;
  const [attempt] = snapshotAttemptOrder.splice(index, 1);
  const prefix = `${attempt}:`;
  for (const key of [...branchSnapshots.keys()]) {
    if (key.startsWith(prefix)) branchSnapshots.delete(key);
  }
}

function retainConversationEvent(
  key: string,
  event: AcpUiEventVm,
  timelineGeneration: number,
  timelineRevision: number | null,
) {
  const eventSessionId = normalizeConversationSessionId(event.sessionId);
  const buffer = branchReplayBuffers.get(key)
    ?? createConversationBranchReplayBuffer(eventSessionId);
  if (eventSessionId && buffer.sessionId && eventSessionId !== buffer.sessionId) {
    resetConversationBranchReplayOwner(key, buffer, eventSessionId);
  } else if (eventSessionId && !buffer.sessionId) {
    buffer.sessionId = eventSessionId;
  }
  if (
    buffer.timelineGeneration !== 0
    && timelineGeneration < buffer.timelineGeneration
  ) {
    return false;
  }
  buffer.generation += 1;
  if (
    buffer.timelineGeneration !== 0
    && timelineGeneration > buffer.timelineGeneration
  ) {
    clearRetainedEvents(buffer);
    buffer.headSeq = 0;
    buffer.headRevision = 0;
    buffer.lossWatermarkGeneration = 0;
    buffer.lossWatermarkRevision = 0;
    buffer.lossWatermarkSeq = 0;
    buffer.lossWatermarkRouterGeneration = 0;
  }
  buffer.timelineGeneration = timelineGeneration;
  // Clock ticks are display state supplied by live delivery and session queries.
  // They never become transcript items and cannot be recovered by message sequence.
  if (event.kind === 'timingUpdate') {
    branchReplayBuffers.set(key, buffer);
    return true;
  }
  buffer.headSeq = Math.max(buffer.headSeq, conversationEventPosition(event));
  buffer.headRevision = Math.max(buffer.headRevision, timelineRevision ?? 0);

  const replayKey = conversationReplayEventKey(event);
  const previous = buffer.events.get(replayKey);
  if (previous) removeRetainedEvent(buffer, replayKey, previous);

  const estimatedBytes = estimateConversationEventBytes(event);
  if (estimatedBytes > MAX_REPLAY_EVENT_BYTES) {
    recordReplayLoss(
      buffer,
      timelineGeneration,
      timelineRevision,
      conversationEventPosition(event),
      buffer.generation,
    );
    branchReplayBuffers.set(key, buffer);
    return true;
  }

  const retained = {
    event,
    timelineGeneration,
    timelineRevision,
    generation: buffer.generation,
    estimatedBytes,
  };
  buffer.events.set(replayKey, retained);
  buffer.order.push(replayKey);
  buffer.retainedBytes += estimatedBytes;
  retainedReplayBytes += estimatedBytes;
  trimBranchReplayBuffer(buffer);
  branchReplayBuffers.set(key, buffer);
  trimGlobalReplayBuffer(key);
  return true;
}

function trimBranchReplayBuffer(buffer: ConversationBranchReplayBuffer) {
  while (
    buffer.order.length > MAX_REPLAY_EVENTS_PER_BRANCH
    || buffer.retainedBytes > MAX_REPLAY_BYTES_PER_BRANCH
  ) {
    const oldestKey = buffer.order[0];
    if (!oldestKey) break;
    const oldest = buffer.events.get(oldestKey);
    if (!oldest) {
      buffer.order.shift();
      continue;
    }
    removeRetainedEvent(buffer, oldestKey, oldest);
    recordReplayLoss(
      buffer,
      oldest.timelineGeneration,
      oldest.timelineRevision,
      conversationEventPosition(oldest.event),
      buffer.generation,
    );
  }
}

function trimGlobalReplayBuffer(currentKey: string) {
  if (retainedReplayBytes <= MAX_REPLAY_BYTES_GLOBAL) return;
  for (const key of branchReplayOrder) {
    if (retainedReplayBytes <= MAX_REPLAY_BYTES_GLOBAL) break;
    if (key === currentKey) continue;
    const buffer = branchReplayBuffers.get(key);
    if (!buffer || buffer.retainedBytes === 0) continue;
    clearRetainedEvents(buffer, true);
  }
}

function removeRetainedEvent(
  buffer: ConversationBranchReplayBuffer,
  key: string,
  retained: RetainedConversationEvent,
) {
  buffer.events.delete(key);
  const orderIndex = buffer.order.indexOf(key);
  if (orderIndex >= 0) buffer.order.splice(orderIndex, 1);
  buffer.retainedBytes = Math.max(0, buffer.retainedBytes - retained.estimatedBytes);
  retainedReplayBytes = Math.max(0, retainedReplayBytes - retained.estimatedBytes);
}

function recordReplayLoss(
  buffer: ConversationBranchReplayBuffer,
  timelineGeneration: number,
  timelineRevision: number | null,
  timelineSeq: number,
  routerGeneration: number,
) {
  if (timelineGeneration !== buffer.lossWatermarkGeneration) {
    buffer.lossWatermarkGeneration = timelineGeneration;
    buffer.lossWatermarkRevision = 0;
    buffer.lossWatermarkSeq = 0;
    buffer.lossWatermarkRouterGeneration = 0;
  }
  if (timelineRevision != null && timelineRevision > 0) {
    buffer.lossWatermarkRevision = Math.max(
      buffer.lossWatermarkRevision,
      timelineRevision,
    );
  }
  if (timelineSeq > 0) {
    buffer.lossWatermarkSeq = Math.max(
      buffer.lossWatermarkSeq,
      timelineSeq,
    );
  }
  if (
    buffer.lossWatermarkRevision === 0
    && buffer.lossWatermarkSeq === 0
  ) return;
  buffer.lossWatermarkRouterGeneration = Math.max(
    buffer.lossWatermarkRouterGeneration,
    routerGeneration,
  );
}

function clearRetainedEvents(
  buffer: ConversationBranchReplayBuffer,
  recordLoss = false,
) {
  if (recordLoss && buffer.events.size > 0) {
    buffer.generation += 1;
    for (const retained of buffer.events.values()) {
      recordReplayLoss(
        buffer,
        retained.timelineGeneration,
        retained.timelineRevision,
        conversationEventPosition(retained.event),
        buffer.generation,
      );
    }
  }
  retainedReplayBytes = Math.max(0, retainedReplayBytes - buffer.retainedBytes);
  buffer.retainedBytes = 0;
  buffer.events.clear();
  buffer.order.splice(0, buffer.order.length);
}

function deleteBranchReplayBuffer(key: string) {
  const buffer = branchReplayBuffers.get(key);
  if (!buffer) return;
  clearRetainedEvents(buffer);
  branchReplayBuffers.delete(key);
}

function conversationReplayEventKey(event: AcpUiEventVm) {
  return `${event.kind}:${event.id}`;
}

function conversationEventPosition(event: AcpUiEventVm) {
  return event.endedSeq ?? event.seq;
}

function estimateConversationEventBytes(event: AcpUiEventVm) {
  const stack: unknown[] = [event];
  let estimatedBytes = 0;
  while (stack.length > 0 && estimatedBytes <= MAX_REPLAY_EVENT_BYTES) {
    const value = stack.pop();
    if (typeof value === 'string') {
      estimatedBytes += value.length * 2;
    } else if (typeof value === 'number' || typeof value === 'boolean') {
      estimatedBytes += 8;
    } else if (Array.isArray(value)) {
      estimatedBytes += value.length * 8;
      if (estimatedBytes > MAX_REPLAY_EVENT_BYTES) continue;
      for (const item of value) stack.push(item);
    } else if (value && typeof value === 'object') {
      for (const property in value) {
        if (!Object.prototype.hasOwnProperty.call(value, property)) continue;
        estimatedBytes += 16 + property.length * 2;
        if (estimatedBytes > MAX_REPLAY_EVENT_BYTES) break;
        stack.push((value as Record<string, unknown>)[property]);
      }
    }
  }
  return estimatedBytes;
}

function notifyBranch(key: string) {
  for (const listener of branchListeners.get(key) ?? []) listener();
  const attempt = attemptKeyOfBranchKey(key);
  const attemptListenerSet = attemptBranchListeners.get(attempt);
  if (!attemptListenerSet) return;
  attemptBranchVersions.set(attempt, (attemptBranchVersions.get(attempt) ?? 0) + 1);
  for (const listener of attemptListenerSet) listener();
}

// Branch ids never contain ':', so the attempt key ends at the last one.
function attemptKeyOfBranchKey(key: string) {
  return key.slice(0, key.lastIndexOf(':'));
}

const ACTIVE_AGENT_BRANCH_STATUSES = new Set(['queued', 'running', 'in_progress', 'waiting_permission']);

export function isActiveAgentBranchStatus(status: string | null | undefined) {
  return status != null && ACTIVE_AGENT_BRANCH_STATUSES.has(status.toLowerCase());
}

export interface ConversationRunningAgent {
  agentExecutionId: string;
  status: string;
  attention: boolean;
  /**
   * The status comes from the provider's own Agent state machine (ordered by
   * provider state sequence) rather than inferred from launch and turn evidence.
   */
  native: boolean;
  /** Null while a live-launched Agent is not in the persisted projection yet. */
  execution: AcpAgentExecutionVm | null;
}

/**
 * Agents of one attempt whose resolved status is active. The persisted
 * projection is the complete set; live snapshots only override status by
 * provider state sequence, exactly as an Agent link card resolves it.
 * `includeUnprojected` adds live branches the projection does not know yet;
 * only the root view may use it, because the live store cannot tell which
 * branch launched them.
 */
export function selectConversationRunningAgents(
  locator: ConversationAttemptLocator,
  projectionAgents: readonly AcpAgentExecutionVm[],
  includeUnprojected: boolean,
): ConversationRunningAgent[] {
  const running: ConversationRunningAgent[] = [];
  const projected = new Set<string>();
  for (const execution of projectionAgents) {
    if (projected.has(execution.agentExecutionId)) continue;
    projected.add(execution.agentExecutionId);
    const live = branchSnapshots.get(conversationBranchStoreKey(locator, execution.agentExecutionId));
    const status = resolveConversationBranchDisplayStatus(
      execution.executionStatus,
      live?.status,
      execution.providerStateSeq,
      live?.providerStateSeq,
    );
    if (!status || !isActiveAgentBranchStatus(status)) continue;
    running.push({
      agentExecutionId: execution.agentExecutionId,
      status,
      attention: live && live.revision > 0 ? live.attention : execution.hasAttention,
      native: (live?.providerStateSeq ?? execution.providerStateSeq) != null,
      execution,
    });
  }
  if (!includeUnprojected) return running;
  const prefix = `${attemptKey(locator)}:`;
  for (const [key, live] of branchSnapshots) {
    if (!key.startsWith(prefix)) continue;
    const branchId = key.slice(prefix.length);
    if (branchId === 'root' || projected.has(branchId)) continue;
    if (!live.status || !isActiveAgentBranchStatus(live.status)) continue;
    running.push({
      agentExecutionId: branchId,
      status: live.status,
      attention: live.attention,
      native: live.providerStateSeq != null,
      execution: null,
    });
  }
  return running;
}

function sameRunningAgents(left: readonly ConversationRunningAgent[], right: readonly ConversationRunningAgent[]) {
  return left.length === right.length && left.every((agent, index) => {
    const other = right[index];
    return agent.agentExecutionId === other.agentExecutionId
      && agent.status === other.status
      && agent.attention === other.attention
      && agent.native === other.native
      && agent.execution === other.execution;
  });
}

const NO_RUNNING_AGENTS: ConversationRunningAgent[] = [];

/** One attempt-level subscription; the result keeps its identity until the running set changes. */
export function useConversationRunningAgents(
  locator: ConversationAttemptLocator,
  projectionAgents: readonly AcpAgentExecutionVm[],
  includeUnprojected: boolean,
) {
  const key = attemptKey(locator);
  const subscribe = useCallback((listener: () => void) => {
    const set = attemptBranchListeners.get(key) ?? new Set<() => void>();
    set.add(listener);
    attemptBranchListeners.set(key, set);
    void requestConversationEventRouterStart();
    return () => {
      set.delete(listener);
      if (set.size === 0) {
        attemptBranchListeners.delete(key);
        attemptBranchVersions.delete(key);
      }
      stopConversationEventRouterRetryWhenIdle();
    };
  }, [key]);
  const getVersion = useCallback(() => attemptBranchVersions.get(key) ?? 0, [key]);
  const version = useSyncExternalStore(subscribe, getVersion, () => 0);
  const previous = useRef<ConversationRunningAgent[]>(NO_RUNNING_AGENTS);
  return useMemo(() => {
    void version;
    const next = selectConversationRunningAgents(locator, projectionAgents, includeUnprojected);
    if (sameRunningAgents(previous.current, next)) return previous.current;
    previous.current = next.length === 0 ? NO_RUNNING_AGENTS : next;
    return previous.current;
    // The locator is identified by its attempt key.
  }, [key, version, projectionAgents, includeUnprojected]);
}

export function useConversationBranchLiveSnapshot(locator: Parameters<typeof attemptKey>[0], branchId: string) {
  const key = conversationBranchStoreKey(locator, branchId);
  const subscribe = useCallback((listener: () => void) => {
    const set = branchListeners.get(key) ?? new Set<() => void>();
    set.add(listener);
    branchListeners.set(key, set);
    void requestConversationEventRouterStart();
    return () => {
      set.delete(listener);
      if (set.size === 0) branchListeners.delete(key);
      stopConversationEventRouterRetryWhenIdle();
    };
  }, [key]);
  const getSnapshot = useCallback(
    () => branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT,
    [key],
  );
  return useSyncExternalStore(
    subscribe,
    getSnapshot,
    () => EMPTY_BRANCH_SNAPSHOT,
  );
}

export function subscribeConversationEvents(listener: Listener) {
  listeners.add(listener);
  void requestConversationEventRouterStart();
  return () => {
    listeners.delete(listener);
    stopConversationEventRouterRetryWhenIdle();
  };
}

export function subscribeConversationAttemptEvents(
  locator: ConversationAttemptLocator,
  listener: Listener,
) {
  const key = attemptKey(locator);
  const keyedListeners = attemptListeners.get(key) ?? new Set<Listener>();
  keyedListeners.add(listener);
  attemptListeners.set(key, keyedListeners);
  void requestConversationEventRouterStart();
  return () => {
    keyedListeners.delete(listener);
    if (keyedListeners.size === 0) attemptListeners.delete(key);
    stopConversationEventRouterRetryWhenIdle();
  };
}

export function conversationEventBranchId(event: AcpSessionUpdatedEventVm) {
  return event.branchId ?? 'root';
}

export function isValidConversationTimelineGeneration(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) > 0;
}

export function conversationEventMatchesAttempt(
  event: AcpSessionUpdatedEventVm,
  locator: ConversationAttemptLocator,
) {
  return attemptKey(event) === attemptKey(locator);
}

export function readConversationBranchLiveSnapshot(
  locator: Parameters<typeof attemptKey>[0],
  branchId: string,
) {
  return branchSnapshots.get(conversationBranchStoreKey(locator, branchId)) ?? EMPTY_BRANCH_SNAPSHOT;
}

export function readConversationBranchReplaySnapshot(
  locator: Parameters<typeof attemptKey>[0],
  branchId: string,
): ConversationBranchReplaySnapshot {
  const buffer = branchReplayBuffers.get(conversationBranchStoreKey(locator, branchId));
  if (!buffer) {
    return {
      sessionId: null,
      generation: 0,
      headSeq: 0,
      timelineGeneration: 0,
      headRevision: 0,
      lossWatermarkGeneration: 0,
      lossWatermarkRevision: 0,
      lossWatermarkSeq: 0,
      lossWatermarkRouterGeneration: 0,
      requiresCatchUp: false,
      retainedBytes: 0,
      events: [],
    };
  }
  return {
    sessionId: buffer.sessionId,
    generation: buffer.generation,
    headSeq: buffer.headSeq,
    timelineGeneration: buffer.timelineGeneration,
    headRevision: buffer.headRevision,
    lossWatermarkGeneration: buffer.lossWatermarkGeneration,
    lossWatermarkRevision: buffer.lossWatermarkRevision,
    lossWatermarkSeq: buffer.lossWatermarkSeq,
    lossWatermarkRouterGeneration: buffer.lossWatermarkRouterGeneration,
    requiresCatchUp:
      buffer.lossWatermarkRevision > 0
      || buffer.lossWatermarkSeq > 0,
    retainedBytes: buffer.retainedBytes,
    events: [...buffer.events.values()]
      .sort((left, right) => (
        conversationEventPosition(left.event) - conversationEventPosition(right.event)
        || left.generation - right.generation
      ))
      .map((retained) => retained.event),
  };
}

export function acknowledgeConversationBranchReplay(
  locator: Parameters<typeof attemptKey>[0],
  branchId: string,
  sessionId: string | null,
  timelineGeneration: number,
  coveredRevision: number,
  coveredSeq: number | null | undefined,
  observedGeneration: number,
) {
  const buffer = branchReplayBuffers.get(conversationBranchStoreKey(locator, branchId));
  if (!buffer) return true;
  if (buffer.sessionId !== normalizeConversationSessionId(sessionId)) return false;
  if (observedGeneration < 0 || observedGeneration > buffer.generation) return false;

  let prefixTimelineGeneration = 0;
  for (const retained of buffer.events.values()) {
    if (retained.generation > observedGeneration) continue;
    prefixTimelineGeneration = Math.max(
      prefixTimelineGeneration,
      retained.timelineGeneration,
    );
  }
  const hasReplayLoss = buffer.lossWatermarkRevision > 0
    || buffer.lossWatermarkSeq > 0;
  const lossBelongsToPrefix = hasReplayLoss
    && buffer.lossWatermarkRouterGeneration <= observedGeneration;
  if (lossBelongsToPrefix) {
    prefixTimelineGeneration = Math.max(
      prefixTimelineGeneration,
      buffer.lossWatermarkGeneration,
    );
  }
  const replayGenerationAhead = prefixTimelineGeneration > 0
    && timelineGeneration < prefixTimelineGeneration;
  const lossRevisionUncovered = lossBelongsToPrefix
    && buffer.lossWatermarkRevision > 0
    && (
      timelineGeneration < buffer.lossWatermarkGeneration
      || (
        (
          buffer.lossWatermarkGeneration === 0
          || timelineGeneration === buffer.lossWatermarkGeneration
        )
        && coveredRevision < buffer.lossWatermarkRevision
      )
    );
  const canonicalCoveredSeq = Number.isSafeInteger(coveredSeq)
    && Number(coveredSeq) > 0
    ? Number(coveredSeq)
    : 0;
  const lossSeqUncovered = lossBelongsToPrefix
    && buffer.lossWatermarkSeq > 0
    && (
      timelineGeneration < buffer.lossWatermarkGeneration
      || (
        (
          buffer.lossWatermarkGeneration === 0
          || timelineGeneration === buffer.lossWatermarkGeneration
        )
        && canonicalCoveredSeq < buffer.lossWatermarkSeq
      )
    );
  if (
    replayGenerationAhead
    || lossRevisionUncovered
    || lossSeqUncovered
  ) {
    return false;
  }
  for (const [key, retained] of [...buffer.events]) {
    if (retained.generation > observedGeneration) continue;
    removeRetainedEvent(buffer, key, retained);
  }
  if (lossBelongsToPrefix) {
    buffer.lossWatermarkRevision = 0;
    buffer.lossWatermarkSeq = 0;
    buffer.lossWatermarkGeneration = 0;
    buffer.lossWatermarkRouterGeneration = 0;
  }
  return true;
}

export function resetConversationEventRouterSnapshots() {
  branchSnapshots.clear();
  branchReplayBuffers.clear();
  branchReplayOrder.splice(0, branchReplayOrder.length);
  snapshotAttemptOrder.splice(0, snapshotAttemptOrder.length);
  retainedReplayBytes = 0;
}
