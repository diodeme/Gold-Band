import { describe, expect, it } from 'vitest';

import {
  acpAttemptWorkspaceResourceKey,
  agentTranscriptResourceKey,
  CONVERSATION_WORKSPACE_LRU_LIMIT,
  ConversationWorkspaceStore,
  conversationDirectoryWorkspaceResourceKey,
  conversationRunWorkspaceResourceKey,
  createHiddenPromptSectionWorkspaceResource,
  createDraftAttachmentWorkspaceResource,
  createConversationWorkspaceScope,
  createDraftConversationWorkspaceScope,
  createInitialRightWorkspaceState,
  fileBrowserWorkspaceResourceKey,
  gitFileComparisonWorkspaceResourceKey,
  hiddenPromptSectionWorkspaceResourceKey,
  projectSourceControlWorkspaceState,
  rightWorkspaceReducer,
  scheduledTaskConfigWorkspaceResourceKey,
  sourceControlWorkspaceResourceKey,
  draftAttachmentWorkspaceResourceKey,
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
  it('keeps one source-control tab and only projects a different workspace identity', () => {
    const resource: RightWorkspaceResource = {
      kind: 'source-control',
      key: sourceControlWorkspaceResourceKey('project-1'),
      scopeKey: 'conversation:project-1:task-1:run-1',
      projectId: 'project-1',
      workspacePath: null,
      title: 'Source control',
      attention: false,
    };
    const main = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource });

    expect(sourceControlWorkspaceResourceKey('project-1')).toBe('source-control:project-1');
    expect(projectSourceControlWorkspaceState(main, null)).toBe(main);

    const worktree = projectSourceControlWorkspaceState(main, 'D:/repo/worktrees/worker-a');
    expect(worktree.tabs).toHaveLength(1);
    expect(worktree.activeTabKey).toBe(resource.key);
    expect(worktree.tabs[0]).toMatchObject({
      key: resource.key,
      workspacePath: 'D:/repo/worktrees/worker-a',
    });
    expect(projectSourceControlWorkspaceState(worktree, 'd:\\REPO\\worktrees\\worker-a\\')).toBe(worktree);
  });

  it('uses stable locator-only keys for conversation and ACP resources', () => {
    const runLocator = { projectId: 'project-1', taskId: 'task-1', runId: 'run-1' };
    expect(conversationRunWorkspaceResourceKey('workflow-view', runLocator)).toBe(
      'workflow-view:project-1:task-1:run-1',
    );
    expect(acpAttemptWorkspaceResourceKey('raw-frames', locator('agent-a'))).toBe(
      'raw-frames:project-1:task-1:run-1:round-1:node-1:attempt-1:::agent-a',
    );
    expect(hiddenPromptSectionWorkspaceResourceKey({
      ...locator('agent-a'),
      eventId: 'prompt-1',
      eventSeq: 42,
      partIndex: 2,
    })).toBe(
      'hidden-prompt-section:project-1:task-1:run-1:round-1:node-1:attempt-1:::agent-a:prompt-1:42:2',
    );
    expect(hiddenPromptSectionWorkspaceResourceKey({
      ...locator('agent-b'),
      eventId: 'prompt-1',
      eventSeq: 42,
      partIndex: 2,
    })).not.toBe(hiddenPromptSectionWorkspaceResourceKey({
      ...locator('agent-a'),
      eventId: 'prompt-1',
      eventSeq: 42,
      partIndex: 2,
    }));

    expect(createHiddenPromptSectionWorkspaceResource({
      scopeKey: 'conversation:project-1:task-1:run-1',
      title: 'Hidden runtime context',
      locator: locator('agent-a'),
      eventId: 'prompt-1',
      eventSeq: 42,
      partIndex: 2,
    })).toEqual({
      kind: 'hidden-prompt-section',
      key: 'hidden-prompt-section:project-1:task-1:run-1:round-1:node-1:attempt-1:::agent-a:prompt-1:42:2',
      scopeKey: 'conversation:project-1:task-1:run-1',
      title: 'Hidden runtime context',
      description: null,
      attention: false,
      locator: {
        ...locator('agent-a'),
        eventId: 'prompt-1',
        eventSeq: 42,
        partIndex: 2,
      },
    });
  });

  it('keeps one run-directory tab bound to the selected attempt without activating unrelated resources', () => {
    const scopeKey = 'conversation:project-1:task-1:run-1';
    const locator = {
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
      roundId: 'round-1',
      nodeId: 'node-1',
      attemptId: 'attempt-1',
    };
    const firstDirectory: RightWorkspaceResource = {
      kind: 'conversation-directory',
      key: conversationDirectoryWorkspaceResourceKey(locator),
      scopeKey,
      title: 'Run directory',
      attention: false,
      locator,
    };
    const workspace: RightWorkspaceResource = {
      kind: 'file-browser',
      key: fileBrowserWorkspaceResourceKey('project-1'),
      scopeKey,
      projectId: 'project-1',
      title: 'Workspace',
      attention: false,
    };
    const nextDirectory: RightWorkspaceResource = {
      ...firstDirectory,
      locator: { ...locator, nodeId: 'node-2', attemptId: 'attempt-2' },
    };

    expect(conversationDirectoryWorkspaceResourceKey(nextDirectory.locator)).toBe(firstDirectory.key);
    let state = rightWorkspaceReducer(createInitialRightWorkspaceState(), { type: 'open', resource: firstDirectory });
    state = rightWorkspaceReducer(state, { type: 'open', resource: workspace });
    state = rightWorkspaceReducer(state, { type: 'synchronize', resource: nextDirectory });

    expect(state.tabs).toHaveLength(2);
    expect(state.activeTabKey).toBe(workspace.key);
    expect(state.tabs.find((tab) => tab.kind === 'conversation-directory')).toEqual(nextDirectory);
    expect(state.tabs.find((tab) => tab.kind === 'file-browser')).toBe(workspace);
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
    expect(state).toMatchObject({ activeTabKey: key });
  });

  it('keys draft attachment previews by draft scope and stable attachment identity', () => {
    expect(draftAttachmentWorkspaceResourceKey('draft:project-1', 'attachment-1')).toBe(
      'draft-attachment:draft:project-1:attachment-1',
    );
    expect(draftAttachmentWorkspaceResourceKey('draft:project-2', 'attachment-1')).not.toBe(
      draftAttachmentWorkspaceResourceKey('draft:project-1', 'attachment-1'),
    );
  });

  it('maps every draft attachment type to the same workspace resource contract', () => {
    const attachment = {
      id: 'attachment-1',
      name: 'notes.md',
      size: 128,
      mime: 'text/markdown',
      path: 'D:/notes.md',
      contentUrl: 'asset://notes',
      source: 'dialog' as const,
    };

    expect(createDraftAttachmentWorkspaceResource({
      scopeKey: 'draft:project-1',
      projectId: 'project-1',
      attachment,
    })).toEqual({
      kind: 'draft-attachment',
      key: 'draft-attachment:draft:project-1:attachment-1',
      scopeKey: 'draft:project-1',
      projectId: 'project-1',
      title: 'notes.md',
      description: 'D:/notes.md',
      attention: false,
      attachment,
    });
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
    expect(state).toMatchObject({ tabs: [], activeTabKey: null });
  });

  it('stores open intent per scope while sharing the width preference', () => {
    const store = new ConversationWorkspaceStore();
    const draft = createDraftConversationWorkspaceScope('project-1');
    const conversation = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', runId: 'run-1' });
    expect(store.peekShellState(draft)).toMatchObject({ requestedOpen: false, openRevision: 0 });

    store.openWorkspace(draft, { explicit: true });
    expect(store.peekShellState(draft)).toMatchObject({ requestedOpen: true, openRevision: 1 });
    expect(store.peekShellState(conversation)).toMatchObject({ requestedOpen: false, openRevision: 0 });
    expect(store.hydrateWidth(720)).toBe(false);
    expect(store.setWidth(760)).toBe(true);
    expect(store.hydrateWidth(800)).toBe(false);
    expect(store.peekShellState(draft)).toMatchObject({ requestedOpen: true, width: 760 });
    expect(store.peekShellState(conversation)).toMatchObject({ requestedOpen: false, width: 760 });

    store.closeWorkspace(draft);
    expect(store.peekShellState(draft)).toMatchObject({ requestedOpen: false, width: 760 });
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
    expect(pullRequest).toContain('github-pr:github.com:acme/widgets:42:1111111111111111111111111111111111111111:2222222222222222222222222222222222222222::src/main.rs');
  });

  it('isolates resource and open state by conversation scope', () => {
    const store = new ConversationWorkspaceStore();
    const first = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', runId: 'run-1' });
    const second = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-2', runId: 'run-1' });
    store.save(first, createInitialRightWorkspaceState());
    store.save(second, {
      ...createInitialRightWorkspaceState(),
      tabs: [{ ...agent('agent-b'), scopeKey: second.key }],
      activeTabKey: agent('agent-b').key,
    });
    store.openWorkspace(second, { explicit: true });

    expect(store.restore(first)).toMatchObject({ tabs: [] });
    expect(store.restore(second)).toMatchObject({ activeTabKey: agent('agent-b').key });
    expect(store.peekShellState(first).requestedOpen).toBe(false);
    expect(store.peekShellState(second).requestedOpen).toBe(true);

    store.closeWorkspace(second);
    expect(store.peekShellState(first).requestedOpen).toBe(false);
    expect(store.peekShellState(second).requestedOpen).toBe(false);
  });

  it('evicts the least recently used conversation workspace after 24 stateful scopes', () => {
    const store = new ConversationWorkspaceStore();
    const scopes = Array.from({ length: CONVERSATION_WORKSPACE_LRU_LIMIT + 1 }, (_, index) => (
      createConversationWorkspaceScope({ projectId: 'project-1', taskId: `task-${index}`, runId: 'run-1' })
    ));
    for (const scope of scopes.slice(0, CONVERSATION_WORKSPACE_LRU_LIMIT)) {
      store.save(scope, createInitialRightWorkspaceState());
    }
    store.restore(scopes[0]);
    store.save(scopes.at(-1)!, createInitialRightWorkspaceState());

    expect(store.has(scopes[0])).toBe(true);
    expect(store.has(scopes[1])).toBe(false);
    expect(store.restore(scopes[1])).toEqual(createInitialRightWorkspaceState());
  });

  it('promotes draft tabs and their content locators when a conversation is created', () => {
    const store = new ConversationWorkspaceStore();
    const draft = createDraftConversationWorkspaceScope('project-1');
    const conversation = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', runId: 'run-1' });
    const attachment = {
      id: 'attachment-1',
      name: 'preview.png',
      size: 128,
      mime: 'image/png',
      previewUrl: 'blob:preview',
      source: 'paste' as const,
    };
    const draftAttachment: RightWorkspaceResource = {
      kind: 'draft-attachment',
      key: draftAttachmentWorkspaceResourceKey(draft.key, attachment.id),
      scopeKey: draft.key,
      projectId: draft.projectId,
      title: attachment.name,
      attention: false,
      attachment,
    };
    store.save(draft, {
      ...createInitialRightWorkspaceState(),
      tabs: [{ ...agent('draft-agent'), scopeKey: draft.key }, draftAttachment],
      activeTabKey: draftAttachment.key,
    });
    store.openWorkspace(draft, { explicit: true });
    store.promoteDraft(draft, conversation);

    expect(store.has(draft)).toBe(false);
    expect(store.restore(conversation)).toMatchObject({
      tabs: [
        { key: agent('draft-agent').key, scopeKey: conversation.key },
        {
          key: draftAttachmentWorkspaceResourceKey(conversation.key, attachment.id),
          scopeKey: conversation.key,
          attachment,
        },
      ],
      activeTabKey: draftAttachmentWorkspaceResourceKey(conversation.key, attachment.id),
    });
    expect(store.peekShellState(conversation).requestedOpen).toBe(true);
  });

  it('keeps draft promotion isolated to the same project and idempotent after success', () => {
    const store = new ConversationWorkspaceStore();
    const draft = createDraftConversationWorkspaceScope('project-1');
    const conversation = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-1', runId: 'run-1' });
    const siblingConversation = createConversationWorkspaceScope({ projectId: 'project-1', taskId: 'task-2', runId: 'run-1' });
    const otherProjectConversation = createConversationWorkspaceScope({ projectId: 'project-2', taskId: 'task-1', runId: 'run-1' });
    store.save(draft, {
      tabs: [{ ...agent('draft-agent'), scopeKey: draft.key }],
      activeTabKey: agent('draft-agent').key,
    });
    store.save(siblingConversation, {
      tabs: [{ ...agent('sibling-agent'), scopeKey: siblingConversation.key }],
      activeTabKey: agent('sibling-agent').key,
    });
    store.openWorkspace(draft, { explicit: true });

    store.promoteDraft(draft, otherProjectConversation);
    expect(store.has(draft)).toBe(true);
    expect(store.has(otherProjectConversation)).toBe(false);

    store.promoteDraft(draft, conversation);
    store.promoteDraft(draft, conversation);
    expect(store.restore(conversation)).toMatchObject({
      tabs: [{ key: agent('draft-agent').key, scopeKey: conversation.key }],
      activeTabKey: agent('draft-agent').key,
    });
    expect(store.restore(siblingConversation)).toMatchObject({
      tabs: [{ key: agent('sibling-agent').key, scopeKey: siblingConversation.key }],
      activeTabKey: agent('sibling-agent').key,
    });
    expect(store.peekShellState(conversation).requestedOpen).toBe(true);
    expect(store.peekShellState(siblingConversation).requestedOpen).toBe(false);
  });
});
