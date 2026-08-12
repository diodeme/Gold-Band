import { describe, expect, it } from 'vitest';

import {
  acpAttemptWorkspaceResourceKey,
  agentTranscriptResourceKey,
  CONVERSATION_WORKSPACE_LRU_LIMIT,
  ConversationWorkspaceStore,
  conversationRunWorkspaceResourceKey,
  createConversationWorkspaceScope,
  createDraftConversationWorkspaceScope,
  createInitialRightWorkspaceState,
  fileBrowserWorkspaceResourceKey,
  gitFileComparisonWorkspaceResourceKey,
  rightWorkspaceReducer,
  scheduledTaskConfigWorkspaceResourceKey,
  type AgentTranscriptLocator,
  type FileWorkspaceResource,
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
  scopeKey: 'draft:default',
  title: branchId,
  status: 'running',
  attention: false,
  locator: locator(branchId),
});

describe('right workspace resource model', () => {
  it('uses stable locator-only keys for conversation and ACP resources', () => {
    const runLocator = { projectId: 'project-1', taskId: 'task-1', runId: 'run-1' };
    expect(conversationRunWorkspaceResourceKey('workflow-view', runLocator)).toBe(
      'workflow-view:project-1:task-1:run-1',
    );
    expect(acpAttemptWorkspaceResourceKey('raw-frames', locator('agent-a'))).toBe(
      'raw-frames:project-1:task-1:run-1:round-1:node-1:attempt-1:::agent-a',
    );
  });

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

  it('models scheduled authoring as one stable tab per draft scope', () => {
    const scopeKey = 'draft:project-1';
    const key = scheduledTaskConfigWorkspaceResourceKey(scopeKey);
    const resource: RightWorkspaceResource = {
      kind: 'scheduled-task-config',
      key,
      scopeKey,
      title: 'Scheduled task settings',
      attention: false,
    };
    let state = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource });
    state = rightWorkspaceReducer(state, { type: 'open', resource: { ...resource, title: 'Updated settings' } });

    expect(key).toBe('scheduled-task-config:draft:project-1');
    expect(state.tabs).toEqual([{ ...resource, title: 'Updated settings' }]);
    expect(state).toMatchObject({ activeTabKey: key, requestedOpen: true });
  });

  it('closes the active tab to its adjacent tab and collapses after the last tab closes', () => {
    let state = createInitialRightWorkspaceState();
    for (const branch of ['agent-a', 'agent-b', 'agent-c']) {
      state = rightWorkspaceReducer(state, { type: 'open', resource: agent(branch) });
    }
    state = rightWorkspaceReducer(state, { type: 'activate', key: agent('agent-b').key });
    state = rightWorkspaceReducer(state, { type: 'close', key: agent('agent-b').key });
    expect(state.activeTabKey).toBe(agent('agent-c').key);

    state = rightWorkspaceReducer(state, { type: 'close', key: agent('agent-c').key });
    state = rightWorkspaceReducer(state, { type: 'close', key: agent('agent-a').key });
    expect(state).toMatchObject({ tabs: [], activeTabKey: null, requestedOpen: false });
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

  it('normalizes project files into one locator-only file browser tab', () => {
    const file: FileWorkspaceResource = {
      kind: 'file',
      key: 'file:D:/repo/src/main.rs',
      scopeKey: 'draft:default',
      projectId: 'default',
      title: 'main.rs',
      attention: false,
      locator: {
        projectId: 'default',
        canonicalPath: 'D:/repo/src/main.rs',
        relativePath: 'src/main.rs',
        scope: 'workspace',
      },
      target: null,
      targetRevision: 1,
    };
    const state = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource: file });
    expect(state.tabs).toHaveLength(1);
    expect(state.tabs[0]).toMatchObject({
      kind: 'file-browser',
      key: fileBrowserWorkspaceResourceKey('default'),
      selectedFile: file,
    });
    expect(state.tabs[0]).not.toHaveProperty('content');
  });

  it('keeps Git comparison tabs isolated by worktree and GitHub pull request', () => {
    const first = gitFileComparisonWorkspaceResourceKey('project-1', {
      kind: 'workspace',
      workspacePath: 'D:/repo/worktree-a',
      path: 'src/main.rs',
      area: 'unstaged',
    });
    const second = gitFileComparisonWorkspaceResourceKey('project-1', {
      kind: 'workspace',
      workspacePath: 'D:/repo/worktree-b',
      path: 'src/main.rs',
      area: 'unstaged',
    });
    const pullRequest = gitFileComparisonWorkspaceResourceKey('project-1', {
      kind: 'github-pr',
      workspacePath: 'D:/repo/worktree-a',
      host: 'github.com',
      repository: 'acme/widgets',
      prNumber: 42,
      baseOid: '1111111111111111111111111111111111111111',
      headOid: '2222222222222222222222222222222222222222',
      path: 'src/main.rs',
    });

    expect(first).not.toBe(second);
    expect(pullRequest).toContain('github-pr:github.com:acme/widgets:42:1111111111111111111111111111111111111111:2222222222222222222222222222222222222222:src/main.rs');
  });

  it('isolates lightweight workspace state by conversation scope', () => {
    const store = new ConversationWorkspaceStore();
    const first = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', runId: 'run-1' });
    const second = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-2', runId: 'run-1' });
    store.save(first, { ...createInitialRightWorkspaceState(), requestedOpen: true });
    store.save(second, {
      ...createInitialRightWorkspaceState(),
      tabs: [{ ...agent('agent-b'), scopeKey: second.key }],
      activeTabKey: agent('agent-b').key,
    });

    expect(store.restore(first)).toMatchObject({ requestedOpen: true, tabs: [] });
    expect(store.restore(second)).toMatchObject({ requestedOpen: false, activeTabKey: agent('agent-b').key });
  });

  it('evicts the least recently used conversation workspace after 24 stateful scopes', () => {
    const store = new ConversationWorkspaceStore();
    const scopes = Array.from({ length: CONVERSATION_WORKSPACE_LRU_LIMIT + 1 }, (_, index) => (
      createConversationWorkspaceScope({ projectId: 'project-1', taskId: `task-${index}`, runId: 'run-1' })
    ));
    for (const scope of scopes.slice(0, CONVERSATION_WORKSPACE_LRU_LIMIT)) {
      store.save(scope, { ...createInitialRightWorkspaceState(), requestedOpen: true });
    }
    store.restore(scopes[0]);
    store.save(scopes.at(-1)!, { ...createInitialRightWorkspaceState(), requestedOpen: true });

    expect(store.has(scopes[0])).toBe(true);
    expect(store.has(scopes[1])).toBe(false);
    expect(store.restore(scopes[1])).toEqual(createInitialRightWorkspaceState());
  });

  it('promotes only the draft open state into a newly created conversation', () => {
    const store = new ConversationWorkspaceStore();
    const draft = createDraftConversationWorkspaceScope('project-1');
    const conversation = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', runId: 'run-1' });
    store.save(draft, {
      ...createInitialRightWorkspaceState(),
      tabs: [{ ...agent('draft-agent'), scopeKey: draft.key }],
      activeTabKey: agent('draft-agent').key,
      requestedOpen: true,
    });
    store.promoteDraft(draft, conversation);

    expect(store.has(draft)).toBe(false);
    expect(store.restore(conversation)).toMatchObject({ tabs: [], activeTabKey: null, requestedOpen: true });
  });
});
