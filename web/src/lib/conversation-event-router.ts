import type { AcpSessionUpdatedEventVm } from '@/api/client';
import { getRuntimeApi } from '@/api/client';
import type { AcpUiEventVm } from '@/types';
import { useSyncExternalStore } from 'react';

type Listener = (event: AcpSessionUpdatedEventVm) => void;

const listeners = new Set<Listener>();
const branchSnapshots = new Map<string, ConversationBranchLiveSnapshot>();
const branchReplayBuffers = new Map<string, ConversationBranchReplayBuffer>();
const branchListeners = new Map<string, Set<() => void>>();
const branchSnapshotOrder: string[] = [];
const MAX_BRANCH_SNAPSHOTS = 64;
const MAX_REPLAY_EVENTS_PER_BRANCH = 64;
const MAX_REPLAY_BYTES_PER_BRANCH = 512 * 1024;
const MAX_REPLAY_EVENT_BYTES = 256 * 1024;
const MAX_REPLAY_BYTES_GLOBAL = 4 * 1024 * 1024;
let retainedReplayBytes = 0;
let started = false;
let starting: Promise<void> | null = null;

interface RetainedConversationEvent {
  event: AcpUiEventVm;
  generation: number;
  estimatedBytes: number;
}

interface ConversationBranchReplayBuffer {
  generation: number;
  headSeq: number;
  requiresCatchUp: boolean;
  retainedBytes: number;
  events: Map<string, RetainedConversationEvent>;
  order: string[];
}

export interface ConversationBranchReplaySnapshot {
  generation: number;
  headSeq: number;
  requiresCatchUp: boolean;
  retainedBytes: number;
  events: AcpUiEventVm[];
}

export const CONVERSATION_EVENT_REPLAY_LIMITS = {
  branchCount: MAX_BRANCH_SNAPSHOTS,
  eventsPerBranch: MAX_REPLAY_EVENTS_PER_BRANCH,
  bytesPerBranch: MAX_REPLAY_BYTES_PER_BRANCH,
  eventBytes: MAX_REPLAY_EVENT_BYTES,
  globalBytes: MAX_REPLAY_BYTES_GLOBAL,
} as const;

async function ensureStarted() {
  if (started || starting) return starting;
  starting = (async () => {
    const subscribe = getRuntimeApi().subscribeAcpSessionUpdates;
    if (!subscribe) {
      started = true;
      return;
    }
    await subscribe((event) => {
      applyConversationEventToBranchSnapshots(event);
      for (const listener of listeners) listener(event);
    });
    started = true;
  })().finally(() => {
    starting = null;
  });
  return starting;
}

export async function ensureConversationEventRouterStarted() {
  await ensureStarted();
}

export interface ConversationBranchLiveSnapshot {
  revision: number;
  contentRevision: number;
  status: string | null;
  attention: boolean;
}

const EMPTY_BRANCH_SNAPSHOT: ConversationBranchLiveSnapshot = {
  revision: 0,
  contentRevision: 0,
  status: null,
  attention: false,
};

function attemptKey(locator: {
  projectId?: string | null;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
}) {
  return [locator.projectId ?? 'default', locator.taskId, locator.runId, locator.roundId, locator.nodeId, locator.attemptId, locator.outerNodeId ?? '', locator.outerAttemptId ?? ''].join(':');
}

export function conversationBranchStoreKey(locator: Parameters<typeof attemptKey>[0], branchId: string) {
  return `${attemptKey(locator)}:${branchId}`;
}

export function applyConversationEventToBranchSnapshots(event: AcpSessionUpdatedEventVm) {
  if (event.event) {
    const key = conversationBranchStoreKey(event, conversationEventBranchId(event));
    retainConversationEvent(key, event.event);
    const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
    if (isSyntheticAgentPrompt(event.event)) {
      updateBranchSnapshot(key, current.status, current.attention, true);
      return;
    }
    if (isAgentBranchResult(event.event)) {
      updateBranchSnapshot(key, 'completed', false, true);
      return;
    }
    const request = event.event.kind === 'permissionRequest' || event.event.kind === 'elicitationRequest';
    const response = event.event.kind === 'elicitationResponse';
    const interaction = request || response;
    const pending = request && (event.event.status ?? 'pending') === 'pending';
    const status = pending
      ? 'waiting_permission'
      : isTerminalBranchStatus(current.status)
        ? current.status
        : 'running';
    updateBranchSnapshot(key, status, interaction ? pending : current.attention, true);
    return;
  }
  if (!event.session) return;
  const prefix = `${attemptKey(event)}:`;
  const rootKey = `${prefix}root`;
  const rootCurrent = branchSnapshots.get(rootKey) ?? EMPTY_BRANCH_SNAPSHOT;
  updateBranchSnapshot(rootKey, event.session.status, rootCurrent.attention);
  const projectedBranchKeys = new Set<string>();
  for (const agent of event.session.timelineProjection?.agents ?? []) {
    const key = `${prefix}${agent.agentExecutionId}`;
    projectedBranchKeys.add(key);
    updateBranchSnapshot(key, agent.executionStatus, agent.hasAttention);
  }
  if (!isTerminalSessionStatus(event.session.status)) return;
  for (const [key, current] of branchSnapshots) {
    if (!key.startsWith(prefix) || key === rootKey || projectedBranchKeys.has(key)) continue;
    updateBranchSnapshot(key, 'interrupted', false);
  }
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
) {
  const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
  if (current.status === status && current.attention === attention) {
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
    status,
    attention,
  });
  notifyBranch(key);
}

function isTerminalBranchStatus(status: string | null) {
  return status != null && ['completed', 'failed', 'cancelled', 'canceled', 'interrupted', 'stopped'].includes(status.toLowerCase());
}

function isTerminalSessionStatus(status: string) {
  return ['completed', 'failed', 'cancelled', 'canceled', 'interrupted', 'stopped'].includes(status.toLowerCase());
}

function storeBranchSnapshot(key: string, snapshot: ConversationBranchLiveSnapshot) {
  branchSnapshots.set(key, snapshot);
  const existing = branchSnapshotOrder.indexOf(key);
  if (existing >= 0) branchSnapshotOrder.splice(existing, 1);
  branchSnapshotOrder.push(key);
  let retainedActive = 0;
  while (branchSnapshotOrder.length > MAX_BRANCH_SNAPSHOTS && retainedActive < branchSnapshotOrder.length) {
    const oldest = branchSnapshotOrder.shift();
    if (!oldest) break;
    if (branchListeners.has(oldest)) {
      branchSnapshotOrder.push(oldest);
      retainedActive += 1;
    } else {
      branchSnapshots.delete(oldest);
      deleteBranchReplayBuffer(oldest);
      retainedActive = 0;
    }
  }
}

function retainConversationEvent(key: string, event: AcpUiEventVm) {
  const buffer = branchReplayBuffers.get(key) ?? {
    generation: 0,
    headSeq: 0,
    requiresCatchUp: false,
    retainedBytes: 0,
    events: new Map<string, RetainedConversationEvent>(),
    order: [],
  };
  buffer.generation += 1;
  buffer.headSeq = Math.max(buffer.headSeq, conversationEventPosition(event));

  const replayKey = conversationReplayEventKey(event);
  const previous = buffer.events.get(replayKey);
  if (previous) removeRetainedEvent(buffer, replayKey, previous);

  const estimatedBytes = estimateConversationEventBytes(event);
  if (estimatedBytes > MAX_REPLAY_EVENT_BYTES) {
    buffer.requiresCatchUp = true;
    branchReplayBuffers.set(key, buffer);
    return;
  }

  const retained = { event, generation: buffer.generation, estimatedBytes };
  buffer.events.set(replayKey, retained);
  buffer.order.push(replayKey);
  buffer.retainedBytes += estimatedBytes;
  retainedReplayBytes += estimatedBytes;
  trimBranchReplayBuffer(buffer);
  branchReplayBuffers.set(key, buffer);
  trimGlobalReplayBuffer(key);
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
    buffer.requiresCatchUp = true;
  }
}

function trimGlobalReplayBuffer(currentKey: string) {
  if (retainedReplayBytes <= MAX_REPLAY_BYTES_GLOBAL) return;
  for (const key of branchSnapshotOrder) {
    if (retainedReplayBytes <= MAX_REPLAY_BYTES_GLOBAL) break;
    if (key === currentKey) continue;
    const buffer = branchReplayBuffers.get(key);
    if (!buffer || buffer.retainedBytes === 0) continue;
    clearRetainedEvents(buffer);
    buffer.requiresCatchUp = true;
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

function clearRetainedEvents(buffer: ConversationBranchReplayBuffer) {
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
}

export function useConversationBranchLiveSnapshot(locator: Parameters<typeof attemptKey>[0], branchId: string) {
  const key = conversationBranchStoreKey(locator, branchId);
  return useSyncExternalStore(
    (listener) => {
      const set = branchListeners.get(key) ?? new Set<() => void>();
      set.add(listener);
      branchListeners.set(key, set);
      void ensureStarted();
      return () => {
        set.delete(listener);
        if (set.size === 0) branchListeners.delete(key);
      };
    },
    () => branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT,
    () => EMPTY_BRANCH_SNAPSHOT,
  );
}

export function subscribeConversationEvents(listener: Listener) {
  listeners.add(listener);
  void ensureStarted();
  return () => { listeners.delete(listener); };
}

export function conversationEventBranchId(event: AcpSessionUpdatedEventVm) {
  return event.branchId ?? 'root';
}

export function conversationEventMatchesAttempt(
  event: AcpSessionUpdatedEventVm,
  locator: {
    projectId?: string | null;
    taskId: string;
    runId: string;
    roundId: string;
    nodeId: string;
    attemptId: string;
    outerNodeId?: string | null;
    outerAttemptId?: string | null;
  },
) {
  return (event.projectId == null || locator.projectId == null || event.projectId === locator.projectId)
    && event.taskId === locator.taskId
    && event.runId === locator.runId
    && event.roundId === locator.roundId
    && event.nodeId === locator.nodeId
    && event.attemptId === locator.attemptId
    && (event.outerNodeId ?? null) === (locator.outerNodeId ?? null)
    && (event.outerAttemptId ?? null) === (locator.outerAttemptId ?? null);
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
      generation: 0,
      headSeq: 0,
      requiresCatchUp: false,
      retainedBytes: 0,
      events: [],
    };
  }
  return {
    generation: buffer.generation,
    headSeq: buffer.headSeq,
    requiresCatchUp: buffer.requiresCatchUp,
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
  snapshotHeadSeq: number,
  observedGeneration: number,
) {
  const buffer = branchReplayBuffers.get(conversationBranchStoreKey(locator, branchId));
  if (
    !buffer
    || buffer.generation !== observedGeneration
    || snapshotHeadSeq < buffer.headSeq
  ) {
    return false;
  }
  clearRetainedEvents(buffer);
  buffer.requiresCatchUp = false;
  return true;
}

export function resetConversationEventRouterSnapshots() {
  branchSnapshots.clear();
  branchReplayBuffers.clear();
  branchSnapshotOrder.splice(0, branchSnapshotOrder.length);
  retainedReplayBytes = 0;
}
