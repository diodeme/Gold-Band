/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { AcpSessionUpdatedEventVm } from '@/api/client';
import {
  applyConversationEventToBranchSnapshots,
  reconcileConversationBranchSession,
  resetConversationEventRouterSnapshots,
  selectConversationRunningAgents,
  useConversationRunningAgents,
  type ConversationRunningAgent,
} from '@/lib/conversation-event-router';
import type { AcpAgentExecutionVm, AcpSessionVm, AcpUiEventVm } from '@/types';

const locator = {
  projectId: 'project-1',
  taskId: 'task-1',
  taskUuid: 'task-uuid-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
};

function agent(
  agentExecutionId: string,
  executionStatus: string,
  providerStateSeq: number | null = null,
): AcpAgentExecutionVm {
  return {
    agentExecutionId,
    executionStatus,
    providerStateSeq,
    eventCount: 1,
    toolCallCount: 0,
    readFileCount: 0,
    writtenFileCount: 0,
    hasAttention: false,
    description: `${agentExecutionId} task`,
    todoEntries: [],
  };
}

function live(branchId: string, event: AcpUiEventVm, attemptId = locator.attemptId): AcpSessionUpdatedEventVm {
  return {
    ...locator,
    attemptId,
    branchId,
    event,
    timelineGeneration: 1,
    timelineRevision: event.endedSeq ?? event.seq,
  };
}

function nativeState(branchId: string, status: string, seq: number, attemptId?: string) {
  return live(branchId, {
    id: `native-state-${branchId}-${seq}`,
    seq,
    endedSeq: seq,
    timestamp: '1Z',
    kind: 'agentState',
    sessionId: 'session-1',
    content: null,
    title: null,
    toolCallId: null,
    status,
    raw: { source: 'nativeSubagentState', _meta: { goldBandConversation: { branchId } } },
  }, attemptId);
}

function rootSession(agents: AcpAgentExecutionVm[]): AcpSessionUpdatedEventVm {
  const session: AcpSessionVm = {
    branchId: 'root',
    parentBranchId: null,
    readOnly: false,
    sessionId: 'session-1',
    provider: 'test',
    status: 'running',
    restored: false,
    events: [],
    eventPage: { loadedCount: 0, total: 0, hasOlder: false, hasNewer: false },
    timelineProjection: { agents, todoEntries: [] },
    pendingInteractions: [],
    diagnostics: { rawFrameCount: 0, eventCount: 0, errorCount: 0 },
  };
  return { ...locator, session };
}

function ids(agents: ConversationRunningAgent[]) {
  return agents.map((entry) => entry.agentExecutionId);
}

describe('running conversation agents', () => {
  beforeEach(() => resetConversationEventRouterSnapshots());

  it('shows an Agent the projection recorded as completed once a newer live state resumes it', () => {
    const projection = [agent('agent-a', 'completed', 10)];
    reconcileConversationBranchSession(locator, rootSession(projection).session!);
    expect(selectConversationRunningAgents(locator, projection, true)).toEqual([]);

    applyConversationEventToBranchSnapshots(nativeState('agent-a', 'running', 20));

    const running = selectConversationRunningAgents(locator, projection, true);
    expect(ids(running)).toEqual(['agent-a']);
    expect(running[0].status).toBe('running');
    expect(running[0].execution).toBe(projection[0]);
  });

  it('keeps the newer persisted state over an older live state and drops ended Agents', () => {
    applyConversationEventToBranchSnapshots(nativeState('agent-a', 'completed', 5));
    const projection = [agent('agent-a', 'running', 30), agent('agent-b', 'running', 31)];
    expect(ids(selectConversationRunningAgents(locator, projection, true))).toEqual(['agent-a', 'agent-b']);

    applyConversationEventToBranchSnapshots(nativeState('agent-b', 'completed', 40));
    expect(ids(selectConversationRunningAgents(locator, projection, true))).toEqual(['agent-a']);
  });

  it('marks an Agent waiting for a permission as needing attention', () => {
    const projection = [agent('agent-a', 'running', 1)];
    applyConversationEventToBranchSnapshots(nativeState('agent-a', 'waiting_permission', 2));

    const [entry] = selectConversationRunningAgents(locator, projection, true);
    expect(entry.status).toBe('waiting_permission');
    expect(entry.attention).toBe(true);
  });

  it('adds live Agents missing from the projection only for the root view', () => {
    applyConversationEventToBranchSnapshots(nativeState('agent-new', 'running', 3));
    applyConversationEventToBranchSnapshots(nativeState('agent-other-attempt', 'running', 4, 'attempt-2'));

    const rootView = selectConversationRunningAgents(locator, [], true);
    expect(ids(rootView)).toEqual(['agent-new']);
    expect(rootView[0].execution).toBeNull();
    expect(selectConversationRunningAgents(locator, [], false)).toEqual([]);
  });

  it('keeps an ended Agent hidden after a root session reconcile with more Agents than the live cache holds', () => {
    // The root projection is fetched once and goes stale while Agents finish in
    // the background; only the live state knows that agent-a has ended.
    const projection = [
      agent('agent-a', 'running', 10),
      ...Array.from({ length: 150 }, (_, index) => agent(`agent-old-${index}`, 'completed', 1)),
    ];
    reconcileConversationBranchSession(locator, rootSession(projection).session!);
    applyConversationEventToBranchSnapshots(nativeState('agent-a', 'completed', 20));
    expect(selectConversationRunningAgents(locator, projection, true)).toEqual([]);

    reconcileConversationBranchSession(locator, rootSession(projection).session!);

    expect(selectConversationRunningAgents(locator, projection, true)).toEqual([]);
  });

  it('marks only Agents with a provider state sequence as native', () => {
    const projection = [agent('agent-native', 'running', 3), agent('agent-inferred', 'running')];
    applyConversationEventToBranchSnapshots(nativeState('agent-live', 'running', 4));

    const running = selectConversationRunningAgents(locator, projection, true);
    expect(running.map((entry) => [entry.agentExecutionId, entry.native])).toEqual([
      ['agent-native', true],
      ['agent-inferred', false],
      ['agent-live', true],
    ]);
  });

  it('lists a projected Agent once even when the projection repeats it', () => {
    const entry = agent('agent-a', 'running', 1);
    expect(ids(selectConversationRunningAgents(locator, [entry, entry], true))).toEqual(['agent-a']);
  });
});

describe('useConversationRunningAgents', () => {
  let host: HTMLDivElement;
  let root: Root;
  const renders: ConversationRunningAgent[][] = [];

  function Probe({ agents }: { agents: AcpAgentExecutionVm[] }) {
    renders.push(useConversationRunningAgents(locator, agents, true));
    return null;
  }

  beforeEach(() => {
    resetConversationEventRouterSnapshots();
    renders.length = 0;
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it('keeps its result identity until the running set changes', async () => {
    const projection = [agent('agent-a', 'running', 1), agent('agent-b', 'completed', 1)];
    await act(async () => root.render(<Probe agents={projection} />));
    const initial = renders.at(-1)!;
    expect(ids(initial)).toEqual(['agent-a']);

    // A status change of an Agent that stays out of the running set re-renders
    // the subscriber but must not hand out a new list.
    await act(async () => applyConversationEventToBranchSnapshots(nativeState('agent-b', 'failed', 2)));
    expect(renders.at(-1)).toBe(initial);

    await act(async () => applyConversationEventToBranchSnapshots(nativeState('agent-b', 'running', 3)));
    const resumed = renders.at(-1)!;
    expect(resumed).not.toBe(initial);
    expect(ids(resumed)).toEqual(['agent-a', 'agent-b']);

    await act(async () => applyConversationEventToBranchSnapshots(nativeState('agent-a', 'completed', 4)));
    expect(ids(renders.at(-1)!)).toEqual(['agent-b']);
  });
});
