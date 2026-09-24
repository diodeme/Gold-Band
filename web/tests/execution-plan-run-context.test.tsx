/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ExecutionPlanSaveCommandVm, ExecutionPlanViewVm } from '@/types';
import { clearExecutionPlanDraftCache, writeExecutionPlanDraft } from '@/lib/execution-plan-draft-cache';
import { createExecutionPlanSession } from '@/lib/execution-plan-editor';
import type { RunExecutionPlanDraft } from '@/components/conversation/RunContextExecutionPlanController';

const api = vi.hoisted(() => ({
  getConversationExecutionPlan: vi.fn(),
  preflightConversationExecutionPlanSave: vi.fn(),
  saveConversationExecutionPlan: vi.fn(),
  recoverConversationExecutionPlanOperation: vi.fn(),
}));

vi.mock('@/api', () => api);

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

import { RunContextExecutionPlanController } from '@/components/conversation/RunContextExecutionPlanController';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const plan = {
  projectId: 'project-1',
  taskId: 'task-1',
  taskUuid: 'uuid-1',
  runId: 'run-1',
  runMode: 'auto',
  runStatus: 'running',
  planRevision: 2,
  authoringRevision: 5,
  executionRevision: 3,
  currentEditable: true,
  diverged: false,
  currentAutoConfig: { agentType: 'current-agent' },
  nextAutoConfig: { agentType: 'next-agent' },
} as ExecutionPlanViewVm;
Object.assign(plan, {
  currentRound: 'round-001',
  currentNode: 'dev',
  currentAttempt: 'attempt-001',
});

function preflightOk() {
  return {
    planRevision: plan.planRevision,
    authoringRevision: plan.authoringRevision,
    executionRevision: plan.executionRevision,
    runStatus: plan.runStatus,
    currentEditable: true,
    diverged: false,
    blocking: [],
    affectedNodeIds: [],
    resumeIdentityRisks: [],
  };
}

describe('run context execution plan controller', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    clearExecutionPlanDraftCache();
    window.localStorage.clear();
    api.getConversationExecutionPlan.mockResolvedValue(plan);
    api.preflightConversationExecutionPlanSave.mockResolvedValue(preflightOk());
    api.saveConversationExecutionPlan.mockResolvedValue({
      complete: true,
      planRevision: 3,
      authoringRevision: 5,
      executionRevision: 3,
      targets: [{ target: 'next', committed: true }],
    });
    api.recoverConversationExecutionPlanOperation.mockResolvedValue({
      complete: true,
      planRevision: 4,
      authoringRevision: 9,
      executionRevision: 3,
      targets: [
        { target: 'current', committed: true },
        { target: 'next', committed: true },
      ],
    });
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  async function renderController() {
    await act(async () => {
      root.render(
        <RunContextExecutionPlanController
          projectId="project-1"
          taskId="task-1"
          taskUuid="uuid-1"
          runId="run-1"
          getDraft={() => ({ kind: 'auto', config: { agentType: 'draft-agent' } })}
          applyDraft={() => undefined}
          onRunMode={() => undefined}
        />,
      );
    });
  }

  it('echoes the current locator from the execution plan view on save', async () => {
    await renderController();
    const save = [...container.querySelectorAll('button')].find((button) => button.textContent?.includes('executionPlan.save'));
    expect(save).toBeTruthy();
    await act(async () => save?.click());
    const command = api.preflightConversationExecutionPlanSave.mock.calls[0]?.[0] as ExecutionPlanSaveCommandVm;
    expect(command.expectedCurrentRound).toBe('round-001');
    expect(command.expectedCurrentNode).toBe('dev');
    expect(command.expectedCurrentAttempt).toBe('attempt-001');
    expect(command.expectedRunStatus).toBe('running');
  });

  it('writes a recovered operation back onto the session baseline', async () => {
    api.saveConversationExecutionPlan.mockResolvedValueOnce({
      complete: false,
      planRevision: 3,
      authoringRevision: 5,
      executionRevision: 3,
      targets: [
        { target: 'current', committed: true },
        { target: 'next', committed: false, error: { code: 'conversation.execution-plan.partial-commit' } },
      ],
    });
    api.recoverConversationExecutionPlanOperation.mockResolvedValue({
      complete: true,
      planRevision: 8,
      authoringRevision: 9,
      executionRevision: 3,
      targets: [
        { target: 'current', committed: true },
        { target: 'next', committed: true },
      ],
    });
    await renderController();
    const save = () => [...container.querySelectorAll('button')].find((button) => button.textContent === 'executionPlan.save');
    await act(async () => save()?.click());
    const recover = [...container.querySelectorAll('button')].find((button) => button.textContent === 'executionPlan.recover');
    expect(recover).toBeTruthy();
    await act(async () => recover?.click());
    expect(container.querySelector('[data-execution-plan-save-bar]')?.getAttribute('data-execution-plan-save-bar')).toBe('ready');
    await act(async () => save()?.click());
    const command = api.preflightConversationExecutionPlanSave.mock.calls.at(-1)?.[0] as ExecutionPlanSaveCommandVm;
    expect(command.expectedPlanRevision).toBe(8);
    expect(command.expectedAuthoringRevision).toBe(9);
    expect(command.expectedCurrentRound).toBe('round-001');
    expect(command.expectedCurrentNode).toBe('dev');
    expect(command.expectedCurrentAttempt).toBe('attempt-001');
  });

  it('keeps a saved AUTO template when the open form was reset before the plan returns', async () => {
    const savedConfig = { agentType: 'claude', activeTemplateId: 'auto-template-test', activeTemplateName: '测试动态' };
    writeExecutionPlanDraft(
      { projectId: 'project-1', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-1' },
      createExecutionPlanSession({
        current: { kind: 'auto', config: savedConfig },
        next: { kind: 'auto', config: savedConfig },
        planRevision: plan.planRevision,
        authoringRevision: plan.authoringRevision,
        executionRevision: plan.executionRevision,
        currentEditable: true,
        diverged: false,
        runStatus: plan.runStatus,
        currentRound: 'round-001',
        currentNode: 'dev',
        currentAttempt: 'attempt-001',
      }),
    );
    api.getConversationExecutionPlan.mockResolvedValue({
      ...plan,
      currentAutoConfig: savedConfig,
      nextAutoConfig: savedConfig,
    });
    const applied: RunExecutionPlanDraft[] = [];
    await act(async () => {
      root.render(
        <RunContextExecutionPlanController
          projectId="project-1"
          taskId="task-1"
          taskUuid="uuid-1"
          runId="run-1"
          getDraft={() => ({ kind: 'auto', config: { agentType: '' } })}
          applyDraft={(draft) => { applied.push(draft); }}
          onRunMode={() => undefined}
        />,
      );
    });
    const last = applied.at(-1);
    expect(last?.kind).toBe('auto');
    if (last?.kind === 'auto') expect(last.config.activeTemplateId).toBe('auto-template-test');
  });

  it('adopts a shared next AUTO template saved from another run when the local draft is clean', async () => {
    const oldConfig = { agentType: 'claude' };
    const nextConfig = { agentType: 'claude', activeTemplateId: 'auto-template-test', activeTemplateName: '测试动态' };
    writeExecutionPlanDraft(
      { projectId: 'project-1', taskId: 'task-1', taskUuid: 'uuid-1', runId: 'run-1' },
      createExecutionPlanSession({
        current: { kind: 'auto', config: oldConfig },
        next: { kind: 'auto', config: oldConfig },
        planRevision: plan.planRevision,
        authoringRevision: plan.authoringRevision,
        executionRevision: plan.executionRevision,
        currentEditable: true,
        diverged: false,
        runStatus: plan.runStatus,
        currentRound: 'round-001',
        currentNode: 'dev',
        currentAttempt: 'attempt-001',
      }),
    );
    api.getConversationExecutionPlan.mockResolvedValue({
      ...plan,
      authoringRevision: 8,
      diverged: true,
      currentAutoConfig: oldConfig,
      nextAutoConfig: nextConfig,
    });
    const applied: RunExecutionPlanDraft[] = [];
    await act(async () => {
      root.render(
        <RunContextExecutionPlanController
          projectId="project-1"
          taskId="task-1"
          taskUuid="uuid-1"
          runId="run-1"
          getDraft={() => ({ kind: 'auto', config: oldConfig })}
          applyDraft={(draft) => { applied.push(draft); }}
          onRunMode={() => undefined}
        />,
      );
    });
    const last = applied.at(-1);
    expect(last?.kind).toBe('auto');
    if (last?.kind === 'auto') expect(last.config.activeTemplateId).toBe('auto-template-test');
  });

  it('does not reuse an operation id after a blocking preflight', async () => {
    let tick = 1_000;
    const nowSpy = vi.spyOn(Date, 'now').mockImplementation(() => tick++);
    api.preflightConversationExecutionPlanSave
      .mockResolvedValueOnce({
        ...preflightOk(),
        blocking: [{ code: 'conversation.execution-plan.validation-failed' }],
      })
      .mockResolvedValueOnce(preflightOk());
    await renderController();
    const save = () => [...container.querySelectorAll('button')].find((button) => button.textContent === 'executionPlan.save');
    await act(async () => save()?.click());
    await act(async () => save()?.click());

    const first = api.preflightConversationExecutionPlanSave.mock.calls[0]?.[0] as ExecutionPlanSaveCommandVm;
    const second = api.preflightConversationExecutionPlanSave.mock.calls[1]?.[0] as ExecutionPlanSaveCommandVm;
    expect(first.operationId).toMatch(/^plan-run-1-\d+$/);
    expect(second.operationId).toMatch(/^plan-run-1-\d+$/);
    expect(second.operationId).not.toBe(first.operationId);
    nowSpy.mockRestore();
  });
});
