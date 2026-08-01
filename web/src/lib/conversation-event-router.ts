import type { AcpSessionUpdatedEventVm } from '@/api/client';
import { getRuntimeApi } from '@/api/client';
import { useSyncExternalStore } from 'react';

type Listener = (event: AcpSessionUpdatedEventVm) => void;

const listeners = new Set<Listener>();
const branchSnapshots = new Map<string, ConversationBranchLiveSnapshot>();
const branchListeners = new Map<string, Set<() => void>>();
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
      updateBranchSnapshots(event);
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

function updateBranchSnapshots(event: AcpSessionUpdatedEventVm) {
  if (event.event) {
    const key = conversationBranchStoreKey(event, conversationEventBranchId(event));
    const current = branchSnapshots.get(key) ?? EMPTY_BRANCH_SNAPSHOT;
    const interaction = event.event.kind === 'permissionRequest' || event.event.kind === 'elicitationRequest' || event.event.kind === 'elicitationResponse';
    const pending = interaction && (event.event.status ?? 'pending') === 'pending';
    branchSnapshots.set(key, {
      revision: current.revision + 1,
      status: pending ? 'waiting_permission' : (current.status ?? 'running'),
      attention: interaction ? pending : current.attention,
    });
    notifyBranch(key);
    return;
  }
  if (!event.session) return;
  const prefix = `${attemptKey(event)}:`;
  for (const [key, current] of branchSnapshots) {
    if (!key.startsWith(prefix)) continue;
    branchSnapshots.set(key, { ...current, revision: current.revision + 1, status: event.session.status });
    notifyBranch(key);
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
  return (event.projectId ?? null) === (locator.projectId ?? null)
    && event.taskId === locator.taskId
    && event.runId === locator.runId
    && event.roundId === locator.roundId
    && event.nodeId === locator.nodeId
    && event.attemptId === locator.attemptId
    && (event.outerNodeId ?? null) === (locator.outerNodeId ?? null)
    && (event.outerAttemptId ?? null) === (locator.outerAttemptId ?? null);
}
