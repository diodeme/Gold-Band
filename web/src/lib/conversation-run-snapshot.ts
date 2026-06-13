import type {
  AcpSessionVm,
  ConversationRunVm,
  ConversationSessionLeafVm,
  ConversationSessionTreeVm,
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
  },
): ConversationRunVm | null {
  if (!current || !snapshot.session) return current;
  if (current.taskId !== snapshot.taskId || current.runId !== snapshot.runId) return current;
  const snapshotKey = conversationSessionKeyFromParts(snapshot);
  if (current.sessionTree.selectedSessionKey !== snapshotKey) return current;
  if (!findConversationLeafByKey(current.sessionTree, snapshotKey)) return current;
  return {
    ...current,
    selectedSession: snapshot.session,
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
  return ['pending', 'running', 'in_progress', 'sending', 'cancelling', 'cancel_requested'].includes(status?.toLowerCase() ?? '');
}

function isConversationActiveLeaf(leaf: ConversationSessionLeafVm) {
  return Boolean(leaf.lifecycle?.runtime.active || leaf.lifecycle?.acp.active || leaf.lifecycle?.acp.stopping)
    || isConversationActiveStatus(leaf.status);
}

function isConversationUnknownStatus(status?: string | null) {
  const normalized = status?.trim().toLowerCase();
  return !normalized || normalized === 'unknown';
}

function findConversationLeafByKey(tree: ConversationSessionTreeVm, key?: string | null): ConversationSessionLeafVm | null {
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
    sessionId: leaf.sessionId,
    startedAt: leaf.startedAt,
  };
}
