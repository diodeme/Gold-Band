/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createExecutionPlanSession } from '@/lib/execution-plan-editor';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string, options?: { nodeId?: string }) => options?.nodeId ? `${key}:${options.nodeId}` : key,
  }),
}));

import { ExecutionPlanSaveBar } from '@/components/conversation/ExecutionPlanSaveBar';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const baselines = {
  current: 'current-plan',
  next: 'next-plan',
  planRevision: 1,
  authoringRevision: 1,
  executionRevision: 1,
  currentEditable: true,
  diverged: true,
  runStatus: 'running',
  currentRound: null,
  currentNode: null,
  currentAttempt: null,
};

describe('execution plan save bar', () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement('div');
    document.body.appendChild(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
  });

  it('places the split action and save action on one aligned row when the runs differ', async () => {
    const onSplit = vi.fn();
    const session = { ...createExecutionPlanSession(baselines, 'next'), split: false };
    await act(async () => {
      root.render(
        <ExecutionPlanSaveBar
          session={session}
          onTargetChange={() => undefined}
          onSave={() => undefined}
          onSelectSide={() => undefined}
          onSplit={onSplit}
          onOpenUnsplit={() => undefined}
          onCancelUnsplit={() => undefined}
          onConfirmUnsplit={() => undefined}
          onReload={() => undefined}
        />,
      );
    });

    const actions = container.querySelector('[data-execution-plan-actions]');
    const split = container.querySelector<HTMLButtonElement>('[data-execution-plan-split]');
    const save = [...container.querySelectorAll('button')].find((button) => button.textContent === 'executionPlan.save');
    const target = container.querySelector('[data-slot="select-trigger"]');

    expect(actions).not.toBeNull();
    expect(split?.textContent).toBe('executionPlan.split');
    expect(actions?.contains(split ?? null)).toBe(true);
    expect(actions?.contains(save ?? null)).toBe(true);
    expect(split?.getAttribute('data-size')).toBe('sm');
    expect(save?.getAttribute('data-size')).toBe('sm');
    expect(target?.getAttribute('data-size')).toBe('sm');
    const saveActions = container.querySelector('[data-execution-plan-save-actions]');
    expect(actions?.className).toContain('flex-col');
    expect(saveActions?.className).toContain('min-w-0');
    expect(saveActions?.className).toContain('w-full');
    expect(saveActions?.className).not.toContain('ml-auto');
    expect(target?.className).toContain('min-w-0');
    expect(saveActions?.contains(split ?? null)).toBe(true);
    expect(saveActions?.contains(save ?? null)).toBe(true);

    await act(async () => {
      split?.click();
    });
    expect(onSplit).toHaveBeenCalledTimes(1);
  });

  async function renderNotice(session: ReturnType<typeof createExecutionPlanSession>, extras: {
    dirty?: boolean;
    onViewIssues?: () => void;
    onRevert?: () => void;
  } = {}) {
    const onReload = vi.fn();
    await act(async () => {
      root.render(
        <ExecutionPlanSaveBar
          session={session}
          onTargetChange={() => undefined}
          onSave={() => undefined}
          onSelectSide={() => undefined}
          onSplit={() => undefined}
          onOpenUnsplit={() => undefined}
          onCancelUnsplit={() => undefined}
          onConfirmUnsplit={() => undefined}
          onReload={onReload}
          dirty={extras.dirty}
          onRevert={extras.onRevert}
          onViewIssues={extras.onViewIssues}
        />,
      );
    });
    return { onReload };
  }

  it('points a validation failure at the inspector instead of reloading', async () => {
    const onViewIssues = vi.fn();
    const session = {
      ...createExecutionPlanSession(baselines, 'current'),
      phase: 'error' as const,
      errorCode: 'conversation.execution-plan.validation-failed',
    };
    const { onReload } = await renderNotice(session, { onViewIssues });
    expect(container.textContent).toContain('errors.conversation.execution-plan.validation-failed');
    expect(container.textContent).not.toContain('executionPlan.reload');
    const viewIssues = container.querySelector<HTMLButtonElement>('[data-execution-plan-view-issues]');
    expect(viewIssues?.textContent).toBe('executionPlan.viewIssues');
    await act(async () => viewIssues?.click());
    expect(onViewIssues).toHaveBeenCalledTimes(1);
    expect(onReload).not.toHaveBeenCalled();
  });

  it('hides the validation reason action when the inspector is already visible', async () => {
    const session = {
      ...createExecutionPlanSession(baselines, 'current'),
      phase: 'error' as const,
      errorCode: 'conversation.execution-plan.validation-failed',
    };
    await renderNotice(session);
    expect(container.querySelector('[data-execution-plan-view-issues]')).toBeNull();
    expect(container.textContent).not.toContain('executionPlan.reload');
  });

  it('names the node that can still be continued and does not offer reload', async () => {
    const session = {
      ...createExecutionPlanSession(baselines, 'current'),
      phase: 'error' as const,
      errorCode: 'conversation.execution-plan.agent-identity-changed',
      errorNodeIds: ['dev', 'review'],
    };
    await renderNotice(session);
    expect(container.textContent).toContain('errors.conversation.execution-plan.agent-identity-changed-node:dev, review');
    expect(container.textContent).not.toContain('executionPlan.reload');
  });

  it('keeps reload for a revision conflict', async () => {
    const session = {
      ...createExecutionPlanSession(baselines, 'current'),
      phase: 'conflict' as const,
      errorCode: 'conversation.execution-plan.revision-conflict',
    };
    const { onReload } = await renderNotice(session);
    const reload = [...container.querySelectorAll('button')].find((button) => button.textContent === 'executionPlan.reload');
    expect(reload).toBeTruthy();
    await act(async () => reload?.click());
    expect(onReload).toHaveBeenCalledTimes(1);
  });

  it('shows revert beside save only after the draft changes', async () => {
    const onRevert = vi.fn();
    const session = createExecutionPlanSession(baselines, 'next');
    await renderNotice(session, { onRevert });
    expect(container.querySelector('[data-execution-plan-revert]')).toBeNull();
    await renderNotice(session, { dirty: true, onRevert });
    const revert = container.querySelector<HTMLButtonElement>('[data-execution-plan-revert]');
    const save = [...container.querySelectorAll('button')].find((button) => button.textContent === 'executionPlan.save');
    expect(revert?.textContent).toBe('executionPlan.revert');
    expect(container.querySelector('[data-execution-plan-save-actions]')?.contains(revert)).toBe(true);
    expect(save?.compareDocumentPosition(revert!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await act(async () => revert?.click());
    expect(onRevert).toHaveBeenCalledTimes(1);
  });
});
