import type {
  AcpSessionVm,
  ConversationAttemptLifecycleVm,
  ConversationRunVm,
  ConversationSessionLeafVm,
  ConversationSessionTreeVm,
  GraphNodeVm,
  GraphVm,
} from '@/types';

export type ConversationRunSnapshotSource =
  | 'create'
  | 'initial-load'
  | 'live-refresh'
  | 'workflow-save'
  | 'session-stopped'
  | 'continue'
  | 'rerun';

export function conversationSessionKeyFromParts(parts: {
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
}) {
  if (parts.outerNodeId && parts.outerAttemptId) {
    return `${parts.roundId}/${parts.outerNodeId}/${parts.outerAttemptId}/${parts.nodeId}/${parts.attemptId}`;
  }
  return `${parts.roundId}/${parts.nodeId}/${parts.attemptId}`;
}

export interface ConversationRunSnapshotMergeOptions {
  selectedSessionKey?: string | null;
  preserveSelectedSession?: boolean;
}

export function applyConversationSelectedSessionSnapshot(
  current: ConversationRunVm | null,
  snapshot: {
    taskId: string;
    runId: string;
    roundId: string;
    nodeId: string;
    attemptId: string;
    outerNodeId?: string | null;
    outerAttemptId?: string | null;
    session?: AcpSessionVm | null;
    lifecycle?: ConversationAttemptLifecycleVm | null;
  },
): ConversationRunVm | null {
  if (!current || (!snapshot.session && !snapshot.lifecycle)) return current;
  if (current.taskId !== snapshot.taskId || current.runId !== snapshot.runId) return current;
  const snapshotKey = conversationSessionKeyFromParts(snapshot);
  if (current.sessionTree.selectedSessionKey !== snapshotKey) return current;
  const leaf = findConversationLeafByKey(current.sessionTree, snapshotKey);
  if (!leaf) return current;
  const nextLeaf = mergeLeafRuntimeSession(leaf, snapshot.session, snapshot.lifecycle);
  const nextActiveSessions = updateActiveSessionsForLeaf(current.activeSessions, nextLeaf);
  const nextWorkflowGraph = patchWorkflowGraphLeaf(current.workflowGraph, nextLeaf);
  if (
    nextLeaf === leaf &&
    nextActiveSessions === current.activeSessions &&
    nextWorkflowGraph === current.workflowGraph &&
    !snapshot.session
  ) {
    return current;
  }
  return {
    ...current,
    selectedSession: snapshot.session ?? current.selectedSession,
    sessionTree: nextLeaf === leaf
      ? current.sessionTree
      : mapConversationTreeLeaf(current.sessionTree, snapshotKey, () => nextLeaf),
    activeSessions: nextActiveSessions,
    workflowGraph: nextWorkflowGraph,
  };
}

export function applyConversationBackgroundSessionRuntimeSnapshot(
  current: ConversationRunVm | null,
  snapshot: {
    taskId: string;
    runId: string;
    roundId: string;
    nodeId: string;
    attemptId: string;
    outerNodeId?: string | null;
    outerAttemptId?: string | null;
    session?: AcpSessionVm | null;
    lifecycle?: ConversationAttemptLifecycleVm | null;
  },
): ConversationRunVm | null {
  if (!current || (!snapshot.session && !snapshot.lifecycle)) return current;
  if (current.taskId !== snapshot.taskId || current.runId !== snapshot.runId) return current;
  const snapshotKey = conversationSessionKeyFromParts(snapshot);
  if (current.sessionTree.selectedSessionKey === snapshotKey) return current;
  const currentLeaf = findConversationLeafByKey(current.sessionTree, snapshotKey);
  if (!currentLeaf) return current;

  const nextLeaf = mergeLeafRuntimeSession(currentLeaf, snapshot.session, snapshot.lifecycle);
  const leafChanged = nextLeaf !== currentLeaf;
  const nextActiveSessions = updateActiveSessionsForLeaf(current.activeSessions, nextLeaf);
  const nextWorkflowGraph = patchWorkflowGraphLeaf(current.workflowGraph, nextLeaf);
  if (!leafChanged && nextActiveSessions === current.activeSessions && nextWorkflowGraph === current.workflowGraph) return current;

  return {
    ...current,
    sessionTree: leafChanged
      ? mapConversationTreeLeaf(current.sessionTree, snapshotKey, () => nextLeaf)
      : current.sessionTree,
    activeSessions: nextActiveSessions,
    workflowGraph: nextWorkflowGraph,
  };
}

export function mergeConversationRunSnapshot(
  current: ConversationRunVm | null,
  incoming: ConversationRunVm,
  source: ConversationRunSnapshotSource,
  options: ConversationRunSnapshotMergeOptions = {},
): ConversationRunVm {
  if (!current || current.runId !== incoming.runId || current.taskId !== incoming.taskId) {
    return incoming;
  }

  const currentKey = current.sessionTree.selectedSessionKey ?? null;
  const incomingKey = incoming.sessionTree.selectedSessionKey ?? null;
  const preferredKey = options.selectedSessionKey ?? null;
  const canPreserveSelection = options.preserveSelectedSession && source !== 'create' && source !== 'rerun';
  const validPreferredKey = preferredKey && findConversationLeafByKey(incoming.sessionTree, preferredKey)
    ? preferredKey
    : null;
  const preservedKey = canPreserveSelection && currentKey && findConversationLeafByKey(incoming.sessionTree, currentKey)
    ? currentKey
    : null;
  const validIncomingKey = incomingKey && findConversationLeafByKey(incoming.sessionTree, incomingKey)
    ? incomingKey
    : null;
  const validCurrentKey = currentKey && findConversationLeafByKey(incoming.sessionTree, currentKey)
    ? currentKey
    : null;
  const defaultIncomingLeaf = findDefaultConversationLeaf(incoming.sessionTree);
  const selectedKey = validPreferredKey
    ?? preservedKey
    ?? validIncomingKey
    ?? validCurrentKey
    ?? (defaultIncomingLeaf ? conversationSessionKeyFromParts(defaultIncomingLeaf) : null);
  const currentLeaf = findConversationLeafByKey(current.sessionTree, selectedKey) ?? selectedLeafForRun(current);
  let merged: ConversationRunVm = {
    ...incoming,
    sessionTree: {
      ...incoming.sessionTree,
      selectedSessionKey: selectedKey,
    },
  };
  const incomingLeaf = findConversationLeafByKey(merged.sessionTree, selectedKey);
  if (selectedKey !== validIncomingKey) {
    merged = selectedKey && selectedKey === currentKey && current.selectedSession
      ? {
          ...merged,
          selectedSession: current.selectedSession,
          artifacts: current.artifacts,
          attachments: current.attachments,
        }
      : {
          ...merged,
          selectedSession: null,
          artifacts: [],
          attachments: [],
        };
  } else if (!merged.selectedSession && selectedKey && selectedKey === currentKey && current.selectedSession) {
    merged = {
      ...merged,
      selectedSession: current.selectedSession,
      artifacts: current.artifacts,
      attachments: current.attachments,
    };
  }
  if (
    selectedKey &&
    currentLeaf &&
    conversationSessionKeyFromParts(currentLeaf) === selectedKey &&
    isConversationActiveLeaf(currentLeaf) &&
    (!incomingLeaf || isConversationUnknownStatus(incomingLeaf.status))
  ) {
    merged = {
      ...merged,
      sessionTree: mapConversationTreeLeaf(merged.sessionTree, selectedKey, (leaf) => ({
        ...leaf,
        status: currentLeaf.status,
        runtimeDisplay: currentLeaf.runtimeDisplay,
        lifecycle: currentLeaf.lifecycle ?? leaf.lifecycle,
        current: currentLeaf.current || leaf.current,
        sessionId: leaf.sessionId ?? currentLeaf.sessionId,
        startedAt: leaf.startedAt ?? currentLeaf.startedAt,
      })),
    };
  }

  const mergedLeaf = selectedLeafForRun(merged);
  if (
    merged.runStatus === 'running' &&
    merged.activeSessions.length === 0 &&
    mergedLeaf &&
    isConversationActiveLeaf(mergedLeaf)
  ) {
    merged = {
      ...merged,
      activeSessions: [activeSessionFromLeaf(mergedLeaf)],
    };
  }

  if (
    current.selectedSession &&
    merged.selectedSession &&
    isConversationActiveStatus(current.selectedSession.status) &&
    isConversationUnknownStatus(merged.selectedSession.status)
  ) {
    merged = { ...merged, selectedSession: current.selectedSession };
  }

  return merged;
}

export function isConversationActiveStatus(status?: string | null) {
  const normalized = status?.trim().toLowerCase().replace(/_/g, '-') ?? '';
  return ['pending', 'ready', 'running', 'in-progress', 'active', 'sending', 'cancelling', 'cancel-requested'].includes(normalized);
}

export function isConversationActiveLifecycle(lifecycle?: ConversationAttemptLifecycleVm | null) {
  return Boolean(lifecycle?.runtime.active || lifecycle?.acp.active || lifecycle?.acp.stopping)
    || isConversationActiveStatus(lifecycle?.runtime.status)
    || isConversationActiveStatus(lifecycle?.acp.status);
}

function isConversationActiveLeaf(leaf: ConversationSessionLeafVm) {
  return Boolean(leaf.manualCheckPending || isConversationActiveLifecycle(leaf.lifecycle))
    || isConversationActiveStatus(leaf.status);
}

function isConversationUnknownStatus(status?: string | null) {
  const normalized = status?.trim().toLowerCase();
  return !normalized || normalized === 'unknown';
}

export function findConversationLeafByKey(tree: ConversationSessionTreeVm, key?: string | null): ConversationSessionLeafVm | null {
  if (!key) return null;
  for (const round of tree.rounds) {
    for (const node of round.nodes) {
      for (const attempt of node.attempts) {
        if (conversationSessionKeyFromParts(attempt) === key) return attempt;
      }
      for (const outer of node.outerNodes ?? []) {
        for (const attempt of outer.attempts) {
          if (conversationSessionKeyFromParts(attempt) === key) return attempt;
        }
      }
    }
  }
  return null;
}

function selectedLeafForRun(run: ConversationRunVm) {
  return findConversationLeafByKey(run.sessionTree, run.sessionTree.selectedSessionKey)
    ?? findDefaultConversationLeaf(run.sessionTree);
}

function findDefaultConversationLeaf(tree: ConversationSessionTreeVm) {
  let active: ConversationSessionLeafVm | null = null;
  let latest: ConversationSessionLeafVm | null = null;
  for (const round of tree.rounds) {
    for (const node of round.nodes) {
      for (const attempt of node.attempts) {
        if (attempt.current) return attempt;
        if (!active && isConversationActiveStatus(attempt.status)) active = attempt;
        if (!latest || conversationLeafSortKey(attempt) > conversationLeafSortKey(latest)) latest = attempt;
      }
      for (const outer of node.outerNodes ?? []) {
        for (const attempt of outer.attempts) {
          if (attempt.current) return attempt;
          if (!active && isConversationActiveStatus(attempt.status)) active = attempt;
          if (!latest || conversationLeafSortKey(attempt) > conversationLeafSortKey(latest)) latest = attempt;
        }
      }
    }
  }
  return active ?? latest;
}

function conversationLeafSortKey(leaf: ConversationSessionLeafVm) {
  return [
    leaf.startedAt ?? leaf.finishedAt ?? '',
    leaf.roundId,
    leaf.outerNodeId ?? '',
    leaf.nodeId,
    leaf.attemptId,
  ].join('\u0000');
}

function mapConversationTreeLeaf(
  tree: ConversationSessionTreeVm,
  key: string,
  update: (leaf: ConversationSessionLeafVm) => ConversationSessionLeafVm,
): ConversationSessionTreeVm {
  let changed = false;
  const rounds = tree.rounds.map((round) => ({
    ...round,
    nodes: round.nodes.map((node) => {
      const attempts = node.attempts.map((leaf) => {
        if (conversationSessionKeyFromParts(leaf) !== key) return leaf;
        changed = true;
        return update(leaf);
      });
      const outerNodes = node.outerNodes?.map((outer) => ({
        ...outer,
        attempts: outer.attempts.map((leaf) => {
          if (conversationSessionKeyFromParts(leaf) !== key) return leaf;
          changed = true;
          return update(leaf);
        }),
      }));
      return { ...node, attempts, outerNodes };
    }),
  }));
  return changed ? { ...tree, rounds } : tree;
}

function activeSessionFromLeaf(leaf: ConversationSessionLeafVm): ConversationRunVm['activeSessions'][number] {
  return {
    roundId: leaf.roundId,
    nodeId: leaf.nodeId,
    attemptId: leaf.attemptId,
    outerNodeId: leaf.outerNodeId,
    outerAttemptId: leaf.outerAttemptId,
    pathLabel: leaf.pathLabel,
    status: leaf.status,
    runtimeDisplay: leaf.runtimeDisplay,
    lifecycle: leaf.lifecycle,
    manualCheckPending: leaf.manualCheckPending,
    sessionId: leaf.sessionId,
    startedAt: leaf.startedAt,
  };
}

function patchWorkflowGraphLeaf(graph: GraphVm, leaf: ConversationSessionLeafVm): GraphVm {
  if (graph.nodes.length === 0) return graph;
  let changed = false;
  const nodes = graph.nodes.map((node) => {
    if (!graphNodeMatchesLeaf(node, leaf)) return node;
    const next = patchWorkflowGraphNode(node, leaf);
    if (next !== node) changed = true;
    return next;
  });
  return changed ? { ...graph, nodes } : graph;
}

function graphNodeMatchesLeaf(node: GraphNodeVm, leaf: ConversationSessionLeafVm) {
  return (node.nodeId ?? null) === leaf.nodeId &&
    (node.attemptId ?? null) === leaf.attemptId &&
    (node.outerNodeId ?? null) === (leaf.outerNodeId ?? null) &&
    (node.outerAttemptId ?? null) === (leaf.outerAttemptId ?? null);
}

function patchWorkflowGraphNode(node: GraphNodeVm, leaf: ConversationSessionLeafVm): GraphNodeVm {
  const outcome = leaf.outcome ?? null;
  const attempts = node.attempts?.map((attempt) => {
    if (attempt.attemptId !== leaf.attemptId) return attempt;
    if (
      attempt.status === leaf.status &&
      (attempt.outcome ?? null) === outcome &&
      attempt.runtimeDisplay === leaf.runtimeDisplay &&
      attempt.current === leaf.current
    ) {
      return attempt;
    }
    return {
      ...attempt,
      status: leaf.status,
      outcome,
      runtimeDisplay: leaf.runtimeDisplay,
      current: leaf.current,
    };
  });
  const attemptsChanged = attempts !== node.attempts && attempts?.some((attempt, index) => attempt !== node.attempts?.[index]);
  if (
    node.status === leaf.status &&
    (node.outcome ?? null) === outcome &&
    node.runtimeDisplay === leaf.runtimeDisplay &&
    node.current === leaf.current &&
    !attemptsChanged
  ) {
    return node;
  }
  return {
    ...node,
    status: leaf.status,
    outcome,
    runtimeDisplay: leaf.runtimeDisplay,
    current: leaf.current,
    attempts,
  };
}

function mergeLeafRuntimeSession(
  leaf: ConversationSessionLeafVm,
  session?: AcpSessionVm | null,
  lifecycle?: ConversationAttemptLifecycleVm | null,
) {
  const nextStatus = lifecycle?.displayStatus
    ?? (session && isConversationActiveStatus(session.status) &&
      !isConversationActiveStatus(leaf.status) &&
      !isConversationTerminalLeafStatus(leaf.status)
      ? session.status
      : leaf.status);
  const nextRuntimeDisplay = lifecycle?.runtimeDisplay ?? leaf.runtimeDisplay;
  const nextLifecycle = lifecycle ?? leaf.lifecycle;
  const nextSessionId = session?.sessionId ?? leaf.sessionId ?? null;
  const nextStartedAt = session?.sessionStartedAt ?? leaf.startedAt ?? null;
  if (
    nextStatus === leaf.status &&
    nextSessionId === (leaf.sessionId ?? null) &&
    nextStartedAt === (leaf.startedAt ?? null) &&
    nextRuntimeDisplay === leaf.runtimeDisplay &&
    nextLifecycle === leaf.lifecycle
  ) {
    return leaf;
  }
  return {
    ...leaf,
    status: nextStatus,
    runtimeDisplay: nextRuntimeDisplay,
    lifecycle: nextLifecycle,
    sessionId: nextSessionId,
    startedAt: nextStartedAt,
  };
}

function isConversationTerminalLeafStatus(status?: string | null) {
  return ['completed', 'complete', 'success', 'failed', 'failure', 'error', 'killed', 'cancelled', 'canceled'].includes(
    status?.trim().toLowerCase().replace(/_/g, '-') ?? '',
  );
}

function updateActiveSessionsForLeaf(
  current: ConversationRunVm['activeSessions'],
  leaf: ConversationSessionLeafVm,
): ConversationRunVm['activeSessions'] {
  if (isConversationActiveLeaf(leaf)) return upsertActiveSession(current, leaf);
  const next = current.filter((item) => conversationSessionKeyFromParts(item) !== conversationSessionKeyFromParts(leaf));
  return next.length === current.length ? current : next;
}

function upsertActiveSession(
  current: ConversationRunVm['activeSessions'],
  leaf: ConversationSessionLeafVm,
): ConversationRunVm['activeSessions'] {
  const next = activeSessionFromLeaf(leaf);
  const index = current.findIndex((item) => conversationSessionKeyFromParts(item) === conversationSessionKeyFromParts(leaf));
  if (index < 0) return [...current, next];
  if (sameActiveSession(current[index], next)) return current;
  return current.map((item, itemIndex) => (itemIndex === index ? next : item));
}

function sameActiveSession(
  left: ConversationRunVm['activeSessions'][number],
  right: ConversationRunVm['activeSessions'][number],
) {
  return left.roundId === right.roundId &&
    left.nodeId === right.nodeId &&
    left.attemptId === right.attemptId &&
    (left.outerNodeId ?? null) === (right.outerNodeId ?? null) &&
    (left.outerAttemptId ?? null) === (right.outerAttemptId ?? null) &&
    left.pathLabel === right.pathLabel &&
    left.status === right.status &&
    (left.sessionId ?? null) === (right.sessionId ?? null) &&
    (left.startedAt ?? null) === (right.startedAt ?? null) &&
    left.manualCheckPending === right.manualCheckPending &&
    left.runtimeDisplay === right.runtimeDisplay &&
    left.lifecycle === right.lifecycle;
}
