import type { AcpSessionUpdatedEventVm } from '@/api/client';
import { getRuntimeApi } from '@/api/client';
import { useSyncExternalStore } from 'react';

type Listener = (event: AcpSessionUpdatedEventVm) => void;

const listeners = new Set<Listener>();
const branchSnapshots = new Map<string, ConversationBranchLiveSnapshot>();
const branchListeners = new Map<string, Set<() => void>>();
const branchSnapshotOrder: string[] = [];
const MAX_BRANCH_SNAPSHOTS = 64;
let started = false;
let starting: Promise<void> | null = null;

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

export interface ConversationBranchLiveSnapshot {
  revision: number;
  status: string | null;
  attention: boolean;
}

const EMPTY_BRANCH_SNAPSHOT: ConversationBranchLiveSnapshot = { revision: 0, status: null, attention: false };

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
    const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
    const request = event.event.kind === 'permissionRequest' || event.event.kind === 'elicitationRequest';
    const response = event.event.kind === 'elicitationResponse';
    const interaction = request || response;
    const pending = request && (event.event.status ?? 'pending') === 'pending';
    const status = pending
      ? 'waiting_permission'
      : isTerminalBranchStatus(current.status)
        ? current.status
        : 'running';
    updateBranchSnapshot(key, status, interaction ? pending : current.attention);
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

function updateBranchSnapshot(key: string, status: string | null, attention: boolean) {
  const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
  if (current.status === status && current.attention === attention) return;
  storeBranchSnapshot(key, {
    revision: current.revision + 1,
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
      retainedActive = 0;
    }
  }
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

export function resetConversationEventRouterSnapshots() {
  branchSnapshots.clear();
  branchSnapshotOrder.splice(0, branchSnapshotOrder.length);
}
