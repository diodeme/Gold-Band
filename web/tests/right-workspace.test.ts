import { describe, expect, it } from 'vitest';

import {
  agentTranscriptResourceKey,
  createInitialRightWorkspaceState,
  rightWorkspaceReducer,
  type AgentTranscriptLocator,
  type RightWorkspaceResource,
} from '@/components/workspace/right-workspace-context';

const locator = (branchId: string): AgentTranscriptLocator => ({
  projectId: 'project-1',
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
  branchId,
});

const agent = (branchId: string): RightWorkspaceResource => ({
  kind: 'agent-transcript',
  key: agentTranscriptResourceKey(locator(branchId)),
  title: branchId,
  status: 'running',
  attention: false,
  locator: locator(branchId),
});

describe('right workspace resource model', () => {
  it('opens, activates, and deduplicates resources by stable key', () => {
    let state = createInitialRightWorkspaceState();
    state = rightWorkspaceReducer(state, { type: 'open', resource: agent('agent-a') });
    state = rightWorkspaceReducer(state, { type: 'open', resource: agent('agent-b') });
    state = rightWorkspaceReducer(state, {
      type: 'open',
      resource: { ...agent('agent-a'), title: 'Agent A updated', attention: true },
    });

    expect(state.tabs).toHaveLength(2);
    expect(state.activeTabKey).toBe(agent('agent-a').key);
    expect(state.tabs[0]).toMatchObject({ title: 'Agent A updated', attention: true });
    expect(state.requestedOpen).toBe(true);
    expect(state.openRevision).toBe(3);
  });

  it('closes the active tab to its adjacent tab and leaves the empty workspace open', () => {
    let state = createInitialRightWorkspaceState();
    for (const branch of ['agent-a', 'agent-b', 'agent-c']) {
      state = rightWorkspaceReducer(state, { type: 'open', resource: agent(branch) });
    }
    state = rightWorkspaceReducer(state, { type: 'activate', key: agent('agent-b').key });
    state = rightWorkspaceReducer(state, { type: 'close', key: agent('agent-b').key });
    expect(state.activeTabKey).toBe(agent('agent-c').key);

    state = rightWorkspaceReducer(state, { type: 'close', key: agent('agent-c').key });
    state = rightWorkspaceReducer(state, { type: 'close', key: agent('agent-a').key });
    expect(state).toMatchObject({ tabs: [], activeTabKey: null, requestedOpen: true });
  });

  it('opens and closes an empty workspace independently from its resources', () => {
    let state = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open-workspace' });
    expect(state).toMatchObject({ tabs: [], activeTabKey: null, requestedOpen: true, openRevision: 1 });
    state = rightWorkspaceReducer(state, { type: 'close-workspace' });
    expect(state).toMatchObject({ tabs: [], activeTabKey: null, requestedOpen: false });
  });

  it('hides the workspace without discarding tabs and reopens the existing tab', () => {
    const resource = agent('agent-a');
    let state = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource });
    state = rightWorkspaceReducer(state, { type: 'close-workspace' });
    expect(state.tabs).toEqual([resource]);
    expect(state.requestedOpen).toBe(false);

    state = rightWorkspaceReducer(state, { type: 'activate', key: resource.key });
    expect(state.tabs).toEqual([resource]);
    expect(state.requestedOpen).toBe(true);
  });

  it('supports future resource kinds without storing resource contents in tab state', () => {
    const file: RightWorkspaceResource = {
      kind: 'file',
      key: 'file:D:/repo/src/main.rs',
      title: 'main.rs',
      path: 'D:/repo/src/main.rs',
      attention: false,
    };
    const state = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource: file });
    expect(state.tabs).toEqual([file]);
    expect(state.tabs[0]).not.toHaveProperty('content');
  });
});
