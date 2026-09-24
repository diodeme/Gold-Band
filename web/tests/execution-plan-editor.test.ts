import { beforeEach, describe, expect, it } from 'vitest';
import {
  activeExecutionPlanDraft,
  applyExecutionPlanSave,
  executionPlanSaveExpectations,
  handoffExecutionPlanSide,
  cancelExecutionPlanUnsplit,
  confirmExecutionPlanUnsplit,
  executionPlanSessionIsDirty,
  revertExecutionPlanToPersisted,
  createExecutionPlanSession,
  executionPlanTargets,
  openExecutionPlanUnsplit,
  rebaseExecutionPlanSession,
  resolveExecutionPlanTarget,
  selectExecutionPlanSide,
  splitExecutionPlanSession,
  executionPlanIssueNotice,
  executionPlanThrownNotice,
  withActiveExecutionPlanDraft,
  withExecutionPlanPhase,
} from '@/lib/execution-plan-editor';
import { clearExecutionPlanDraftCache, readExecutionPlanDraft, writeExecutionPlanDraft } from '@/lib/execution-plan-draft-cache';
import { readExecutionPlanSaveTarget, writeExecutionPlanSaveTarget } from '@/lib/execution-plan-preferences';

const baselines = {
  current: 'current-plan',
  next: 'next-authoring',
  planRevision: 2,
  authoringRevision: 4,
  executionRevision: 9,
  currentEditable: true,
  diverged: false,
  runStatus: 'running',
  currentRound: 'round-001',
  currentNode: 'dev',
  currentAttempt: 'attempt-001',
};

describe('execution plan editor session', () => {
  beforeEach(() => clearExecutionPlanDraftCache());

  it('starts unified on the next draft and offers three save targets', () => {
    const session = createExecutionPlanSession(baselines, 'next');
    expect(session.split).toBe(false);
    expect(activeExecutionPlanDraft(session)).toBe('next-authoring');
    expect(executionPlanTargets(session)).toEqual(['current', 'next', 'current-and-next']);
    expect(session.executionRevision).toBe(9);
  });

  it('forces next when the current run cannot be edited', () => {
    const session = createExecutionPlanSession({ ...baselines, currentEditable: false, runStatus: 'completed' }, 'current');
    expect(resolveExecutionPlanTarget(session, 'current-and-next')).toBe('next');
    expect(executionPlanTargets(session)).toEqual(['next']);
  });

  it('stays unified when a single-side save leaves the two plans equal', () => {
    const session = createExecutionPlanSession(baselines);
    const saved = applyExecutionPlanSave(withActiveExecutionPlanDraft(session, 'same-plan'), {
      complete: true,
      diverged: false,
      planRevision: 3,
      authoringRevision: 4,
      executionRevision: 9,
      savedDraft: 'same-plan',
      targets: [{ target: 'current', committed: true }],
    });
    expect(saved.split).toBe(false);
    expect(saved.diverged).toBe(false);
    expect(saved.unifiedDraft).toBe('same-plan');
    expect(saved.currentDraft).toBe('same-plan');
    expect(saved.nextDraft).toBe('same-plan');
  });

  it('splits after a single-side save and stays unified after both sides commit', () => {
    const session = createExecutionPlanSession(baselines);
    const currentOnly = applyExecutionPlanSave(withActiveExecutionPlanDraft(session, 'edited'), {
      complete: true,
      diverged: true,
      planRevision: 3,
      authoringRevision: 4,
      executionRevision: 9,
      savedDraft: 'edited',
      targets: [{ target: 'current', committed: true }],
    });
    expect(currentOnly.split).toBe(true);
    expect(currentOnly.currentBaseline).toBe('edited');
    expect(currentOnly.nextDraft).toBe('next-authoring');

    const both = applyExecutionPlanSave(currentOnly, {
      complete: true,
      diverged: false,
      planRevision: 4,
      authoringRevision: 5,
      executionRevision: 9,
      savedDraft: 'synced',
      targets: [
        { target: 'current', committed: true },
        { target: 'next', committed: true },
      ],
    });
    expect(both.split).toBe(false);
    expect(both.currentDraft).toBe('synced');
    expect(both.nextDraft).toBe('synced');
    expect(both.executionRevision).toBe(9);
  });

  it('opens an already different plan on two tabs', () => {
    const session = createExecutionPlanSession({ ...baselines, diverged: true });
    expect(session.split).toBe(true);
    expect(session.activeSide).toBe('next');
    expect(session.currentDraft).toBe('current-plan');
    expect(session.nextDraft).toBe('next-authoring');
  });

  it('splits a diverged unified draft onto next without copying it over current', () => {
    const session = { ...createExecutionPlanSession({ ...baselines, diverged: true }), split: false };
    const split = splitExecutionPlanSession(session, 'edited-next');
    expect(split.split).toBe(true);
    expect(split.activeSide).toBe('next');
    expect(split.nextDraft).toBe('edited-next');
    expect(split.currentDraft).toBe('current-plan');
    expect(activeExecutionPlanDraft(split)).toBe('edited-next');
    expect(splitExecutionPlanSession(split, 'later').currentDraft).toBe('current-plan');
  });

  it('writes the leaving draft before the other side becomes active', () => {
    const session = selectExecutionPlanSide(createExecutionPlanSession(baselines), 'current');
    const next = handoffExecutionPlanSide(session, 'current-edited', 'next');
    expect(next.currentDraft).toBe('current-edited');
    expect(activeExecutionPlanDraft(next)).toBe('next-authoring');
    const back = handoffExecutionPlanSide(next, 'next-edited', 'current');
    expect(back.nextDraft).toBe('next-edited');
    expect(activeExecutionPlanDraft(back)).toBe('current-edited');
  });

  it('echoes the session locator on a save command', () => {
    const session = createExecutionPlanSession(baselines);
    expect(executionPlanSaveExpectations(session)).toEqual({
      expectedPlanRevision: 2,
      expectedAuthoringRevision: 4,
      expectedRunStatus: 'running',
      expectedCurrentRound: 'round-001',
      expectedCurrentNode: 'dev',
      expectedCurrentAttempt: 'attempt-001',
    });
  });

  it('keeps the other tab draft when one side is saved', () => {
    let session = selectExecutionPlanSide(createExecutionPlanSession(baselines), 'current');
    session = withActiveExecutionPlanDraft(session, 'current-edit');
    session = selectExecutionPlanSide(session, 'next');
    session = withActiveExecutionPlanDraft(session, 'next-edit');
    const saved = applyExecutionPlanSave(session, {
      complete: true,
      diverged: true,
      planRevision: 2,
      authoringRevision: 5,
      executionRevision: 9,
      savedDraft: 'next-edit',
      targets: [{ target: 'next', committed: true }],
    });
    expect(saved.currentDraft).toBe('current-edit');
    expect(saved.nextBaseline).toBe('next-edit');
    expect(saved.planRevision).toBe(2);
    expect(saved.authoringRevision).toBe(5);
  });

  it('keeps drafts when unsplit is cancelled and uses the chosen side when confirmed', () => {
    let session = selectExecutionPlanSide(createExecutionPlanSession(baselines), 'current');
    session = withActiveExecutionPlanDraft(session, 'keep-current');
    session = selectExecutionPlanSide(session, 'next');
    session = withActiveExecutionPlanDraft(session, 'keep-next');
    const opened = openExecutionPlanUnsplit(session);
    const cancelled = cancelExecutionPlanUnsplit(opened);
    expect(cancelled.split).toBe(true);
    expect(cancelled.currentDraft).toBe('keep-current');
    expect(cancelled.nextDraft).toBe('keep-next');
    const confirmed = confirmExecutionPlanUnsplit(opened, 'current');
    expect(confirmed.split).toBe(false);
    expect(confirmed.diverged).toBe(false);
    expect(confirmed.unifiedDraft).toBe('keep-current');
    expect(confirmed.currentDraft).toBe('keep-current');
    expect(confirmed.nextDraft).toBe('keep-current');
    expect(executionPlanSessionIsDirty(confirmed, 'keep-current')).toBe(true);
    const reverted = revertExecutionPlanToPersisted(confirmed);
    expect(reverted.split).toBe(true);
    expect(reverted.currentDraft).toBe('current-plan');
    expect(reverted.nextDraft).toBe('next-authoring');
  });

  it('does not treat a stored AUTO config with null fields as a draft', () => {
    const stored = {
      kind: 'auto' as const,
      config: {
        agentStrategy: 'fixed' as const,
        agentType: 'claude-acp',
        bootstrapAgentType: null,
        bootstrapModelId: null,
        acceptanceModelId: null,
        modelId: 'sonnet',
        permissionMode: 'bypassPermissions',
        configOptions: { effort: 'low' },
        modelBoundOverrides: { sonnet: { effort: 'low' } },
        availableAgents: null,
        routingPrompt: null,
        allowedWorkflows: [],
        allowedProfiles: ['pf-builtin-review'],
        globalGoal: null,
        control: { maxDynamicNodes: 20, maxFanout: 5, maxDepth: 6, maxParallel: 3, maxGroupDepth: 1, maxWorkflowInvocations: 10, allowNestedDynamic: false },
        activeTemplateId: 'auto-template-d18b52b26a404f96a5963c16cca4804f',
        activeTemplateName: '测试动态',
      },
    };
    const projected = {
      kind: 'auto' as const,
      config: {
        agentStrategy: 'fixed' as const,
        agentType: 'claude-acp',
        modelId: 'sonnet',
        permissionMode: 'bypassPermissions',
        configOptions: { effort: 'low' },
        modelBoundOverrides: { sonnet: { effort: 'low' } },
        allowedWorkflows: [],
        allowedProfiles: ['pf-builtin-review'],
        control: stored.config.control,
        activeTemplateId: stored.config.activeTemplateId,
        activeTemplateName: '测试动态',
      },
    };
    const session = createExecutionPlanSession({
      ...baselines,
      diverged: false,
      current: stored,
      next: stored,
    });
    expect(executionPlanSessionIsDirty(session, projected)).toBe(false);
    const edited = { ...projected, config: { ...projected.config, agentType: 'codebuddy-code' } };
    expect(executionPlanSessionIsDirty(session, edited)).toBe(true);
  });

  it('preserves drafts on conflict and lists partial targets', () => {
    const session = withActiveExecutionPlanDraft(createExecutionPlanSession(baselines), 'local-draft');
    const conflict = applyExecutionPlanSave(session, {
      complete: false,
      planRevision: 2,
      authoringRevision: 4,
      executionRevision: 9,
      savedDraft: 'local-draft',
      targets: [{ target: 'current', committed: false, errorCode: 'conversation.execution-plan.revision-conflict' }],
    });
    expect(conflict.phase).toBe('conflict');
    expect(activeExecutionPlanDraft(conflict)).toBe('local-draft');

    const partial = applyExecutionPlanSave(session, {
      complete: false,
      diverged: true,
      planRevision: 3,
      authoringRevision: 4,
      executionRevision: 9,
      savedDraft: 'local-draft',
      targets: [
        { target: 'current', committed: true },
        { target: 'next', committed: false, errorCode: 'conversation.execution-plan.partial-commit' },
      ],
    });
    expect(partial.phase).toBe('partial');
    expect(partial.partialTargets.map((target) => `${target.target}:${target.committed}`)).toEqual(['current:true', 'next:false']);
    expect(partial.nextDraft).toBe('local-draft');
  });

  it('restores an in-memory draft after close and drops it on restart', () => {
    const locator = { projectId: 'p', taskId: 't', taskUuid: 'uuid', runId: 'run' };
    writeExecutionPlanDraft(locator, 'draft');
    expect(readExecutionPlanDraft(locator)).toBe('draft');
    clearExecutionPlanDraftCache();
    expect(readExecutionPlanDraft(locator)).toBeUndefined();
  });

  it('collects node ids for the first blocking execution-plan issue', () => {
    expect(executionPlanIssueNotice([
      { code: 'conversation.execution-plan.agent-identity-changed', context: { nodeId: 'dev' } },
      { code: 'conversation.execution-plan.agent-identity-changed', context: { nodeId: 'review' } },
      { code: 'conversation.execution-plan.continue-unsupported', context: { nodeId: 'test' } },
    ])).toEqual({
      code: 'conversation.execution-plan.agent-identity-changed',
      nodeIds: ['dev', 'review'],
    });
    expect(executionPlanThrownNotice({
      code: 'conversation.execution-plan.agent-identity-changed',
      params: { nodeId: 'plan' },
    })).toEqual({
      code: 'conversation.execution-plan.agent-identity-changed',
      nodeIds: ['plan'],
    });
    const session = withExecutionPlanPhase(
      createExecutionPlanSession(baselines),
      'error',
      'conversation.execution-plan.agent-identity-changed',
      ['dev'],
    );
    expect(session.errorNodeIds).toEqual(['dev']);
    expect(withExecutionPlanPhase(session, 'ready').errorNodeIds).toEqual([]);
  });

  it('adopts a newer shared next draft when the local next draft is unchanged', () => {
    const session = createExecutionPlanSession({
      ...baselines,
      current: { workflow: 'current-graph', bindings: 'current-agent' },
      next: { workflow: 'next-with-review', bindings: 'review-agent' },
    });
    const rebased = rebaseExecutionPlanSession(session, {
      ...baselines,
      authoringRevision: 8,
      current: { workflow: 'current-graph', bindings: 'current-agent' },
      next: { workflow: 'next-without-review', bindings: 'dev-agent' },
    });
    expect(activeExecutionPlanDraft(rebased)).toEqual({ workflow: 'next-without-review', bindings: 'dev-agent' });
    expect(rebased.nextBaseline).toEqual({ workflow: 'next-without-review', bindings: 'dev-agent' });
    expect(rebased.currentDraft).toEqual({ workflow: 'next-without-review', bindings: 'dev-agent' });
  });

  it('keeps a dirty next draft workflow and bindings together when shared authoring moves', () => {
    const session = createExecutionPlanSession({
      ...baselines,
      current: { workflow: 'current-graph', bindings: 'current-agent' },
      next: { workflow: 'next-with-review', bindings: 'review-agent' },
    });
    const dirty = withActiveExecutionPlanDraft(session, { workflow: 'local-edit', bindings: 'local-agent' });
    const rebased = rebaseExecutionPlanSession(dirty, {
      ...baselines,
      authoringRevision: 8,
      current: { workflow: 'current-graph', bindings: 'current-agent' },
      next: { workflow: 'next-without-review', bindings: '' },
    });
    expect(activeExecutionPlanDraft(rebased)).toEqual({ workflow: 'local-edit', bindings: 'local-agent' });
    expect(rebased.nextDraft).toEqual({ workflow: 'local-edit', bindings: 'local-agent' });
    expect(rebased.currentDraft).toEqual({ workflow: 'local-edit', bindings: 'local-agent' });
  });

  it('stores the save target preference with a versioned schema and defaults to next', () => {
    const memory = new Map<string, string>();
    const storage = {
      getItem: (key: string) => memory.get(key) ?? null,
      setItem: (key: string, value: string) => { memory.set(key, value); },
    };
    expect(readExecutionPlanSaveTarget(storage)).toBe('next');
    writeExecutionPlanSaveTarget('current', storage);
    expect(readExecutionPlanSaveTarget(storage)).toBe('current');
    expect(JSON.parse(memory.values().next().value!).schemaVersion).toBe(1);
  });
});
