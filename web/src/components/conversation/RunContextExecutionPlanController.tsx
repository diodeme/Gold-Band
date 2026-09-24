import { useCallback, useEffect, useRef, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getConversationExecutionPlan, preflightConversationExecutionPlanSave, recoverConversationExecutionPlanOperation, saveConversationExecutionPlan } from '@/api';
import { ExecutionPlanSaveBar } from '@/components/conversation/ExecutionPlanSaveBar';
import {
  activeExecutionPlanDraft,
  applyExecutionPlanSave,
  cancelExecutionPlanUnsplit,
  confirmExecutionPlanUnsplit,
  createExecutionPlanSession,
  executionPlanSessionIsDirty,
  revertExecutionPlanToPersisted,
  executionPlanBaselinesFromView,
  executionPlanIssueNotice,
  executionPlanSaveExpectations,
  executionPlanThrownNotice,
  openExecutionPlanUnsplit,
  rebaseExecutionPlanSession,
  resolveExecutionPlanTarget,
  selectExecutionPlanSide,
  splitExecutionPlanSession,
  withActiveExecutionPlanDraft,
  withExecutionPlanPhase,
  type ExecutionPlanBaselines,
  type ExecutionPlanSession,
} from '@/lib/execution-plan-editor';
import { readExecutionPlanDraft, writeExecutionPlanDraft } from '@/lib/execution-plan-draft-cache';
import { readExecutionPlanSaveTarget, writeExecutionPlanSaveTarget } from '@/lib/execution-plan-preferences';
import type { ConversationAutoConfigVm, ExecutionPlanSaveResultVm, ExecutionPlanSaveTarget, ExecutionPlanViewVm, WorkflowDsl, WorkflowModelBindings } from '@/types';

export type RunExecutionPlanDraft =
  | { kind: 'auto'; config: ConversationAutoConfigVm }
  | { kind: 'workflow'; workflow: WorkflowDsl; modelBindings: WorkflowModelBindings };

type RunPlanSession = ExecutionPlanSession<RunExecutionPlanDraft>;

interface RunContextExecutionPlanControllerProps {
  projectId: string;
  taskId: string;
  taskUuid: string;
  runId: string;
  getDraft: () => RunExecutionPlanDraft;
  applyDraft: (draft: RunExecutionPlanDraft) => void;
  onRunMode: (mode: 'auto' | 'workflow') => void;
  onSaved?: (saved: ExecutionPlanSaveResultVm) => void;
}

const EMPTY_MODEL_BINDINGS: WorkflowModelBindings = { definitionRevision: '', bindingRevision: 0, bindings: [] };
const EMPTY_AUTO_CONFIG: ConversationAutoConfigVm = { agentType: '' };

export function runExecutionPlanBaselines(plan: ExecutionPlanViewVm): ExecutionPlanBaselines<RunExecutionPlanDraft> {
  if (plan.runMode === 'workflow') {
    const current = plan.currentWorkflow ?? plan.nextWorkflow;
    const next = plan.nextWorkflow ?? plan.currentWorkflow;
    if (!current || !next) throw new Error('conversation.execution-plan.not-found');
    return executionPlanBaselinesFromView(
      plan,
      { kind: 'workflow', workflow: current, modelBindings: plan.currentModelBindings ?? EMPTY_MODEL_BINDINGS },
      { kind: 'workflow', workflow: next, modelBindings: plan.nextModelBindings ?? EMPTY_MODEL_BINDINGS },
    );
  }
  return executionPlanBaselinesFromView(
    plan,
    { kind: 'auto', config: plan.currentAutoConfig ?? plan.nextAutoConfig ?? EMPTY_AUTO_CONFIG },
    { kind: 'auto', config: plan.nextAutoConfig ?? plan.currentAutoConfig ?? EMPTY_AUTO_CONFIG },
  );
}

function errorCodeOf(error: unknown): string | null {
  return typeof error === 'object' && error && 'code' in error ? String((error as { code: string }).code) : null;
}

export function RunContextExecutionPlanController({
  projectId,
  taskId,
  taskUuid,
  runId,
  getDraft,
  applyDraft,
  onRunMode,
  onSaved,
}: RunContextExecutionPlanControllerProps) {
  const { t } = useTranslation();
  const applyDraftRef = useRef(applyDraft);
  const getDraftRef = useRef(getDraft);
  const onRunModeRef = useRef(onRunMode);
  const onSavedRef = useRef(onSaved);
  applyDraftRef.current = applyDraft;
  getDraftRef.current = getDraft;
  onRunModeRef.current = onRunMode;
  onSavedRef.current = onSaved;
  const [session, setSession] = useState<RunPlanSession | null>(null);
  const sessionRef = useRef<RunPlanSession | null>(null);
  sessionRef.current = session;
  const [operationId, setOperationId] = useState<string | null>(null);
  const [reloadNonce, setReloadNonce] = useState(0);
  const [loadErrorCode, setLoadErrorCode] = useState<string | null>(null);

  const commit = useCallback((next: RunPlanSession, refocusForm: boolean) => {
    sessionRef.current = next;
    setSession(next);
    writeExecutionPlanDraft({ projectId, taskId, taskUuid, runId }, next);
    if (refocusForm) applyDraftRef.current(activeExecutionPlanDraft(next));
  }, [projectId, runId, taskId, taskUuid]);

  useEffect(() => {
    let active = true;
    const liveAtStart = sessionRef.current;
    const currentLocator = { projectId, taskId, taskUuid, runId };
    const remembered = liveAtStart ?? readExecutionPlanDraft<RunPlanSession>(currentLocator) ?? null;
    if (remembered && !liveAtStart) {
      setSession(remembered);
      applyDraftRef.current(activeExecutionPlanDraft(remembered));
    }
    setLoadErrorCode(null);
    void getConversationExecutionPlan(projectId, taskId, taskUuid, runId).then((plan) => {
      if (!active) return;
      onRunModeRef.current(plan.runMode === 'workflow' ? 'workflow' : 'auto');
      const baselines = runExecutionPlanBaselines(plan);
      const previous = sessionRef.current ?? remembered;
      // The first load must not fold the form. A run-context page reset can
      // clear it before this response arrives, and that empty form is not an edit.
      const seeded = previous && liveAtStart
        ? withActiveExecutionPlanDraft(previous, getDraftRef.current())
        : previous;
      const next = seeded
        ? rebaseExecutionPlanSession(seeded, baselines)
        : createExecutionPlanSession(baselines, readExecutionPlanSaveTarget());
      sessionRef.current = next;
      setSession(next);
      writeExecutionPlanDraft(currentLocator, next);
      applyDraftRef.current(activeExecutionPlanDraft(next));
    }).catch((error) => {
      if (!active) return;
      setLoadErrorCode(errorCodeOf(error) ?? 'conversation.execution-plan.not-found');
    });
    return () => { active = false; };
  }, [projectId, reloadNonce, runId, taskId, taskUuid]);

  const applyOutcome = useCallback((base: RunPlanSession, saved: ExecutionPlanSaveResultVm, savedDraft: RunExecutionPlanDraft) => {
    const next = applyExecutionPlanSave(base, {
      complete: saved.complete,
      diverged: saved.diverged,
      planRevision: saved.planRevision,
      authoringRevision: saved.authoringRevision,
      executionRevision: saved.executionRevision,
      savedDraft,
      targets: saved.targets.map((item) => ({ target: item.target, committed: item.committed, errorCode: item.error?.code ?? null })),
    });
    commit(next, false);
    if (saved.complete) {
      setOperationId(null);
      onSavedRef.current?.(saved);
    }
  }, [commit]);

  if (!session) {
    return (
      <div className="flex items-center gap-2 px-1 text-xs text-muted-foreground" data-execution-plan-save-bar={loadErrorCode ? 'error' : 'loading'}>
        {loadErrorCode ? (
          <>
            <span className="text-destructive">{t(`errors.${loadErrorCode}`, { defaultValue: t('executionPlan.saveFailed') })}</span>
            <button type="button" className="underline" onClick={() => setReloadNonce((value) => value + 1)}>{t('executionPlan.reload')}</button>
          </>
        ) : (
          <>
            <Loader2 className="size-3.5 animate-spin" />
            {t('executionPlan.loading')}
          </>
        )}
      </div>
    );
  }

  const save = async (target: ExecutionPlanSaveTarget) => {
    const resolved = resolveExecutionPlanTarget(session, target);
    writeExecutionPlanSaveTarget(resolved);
    const draft = getDraft();
    const base = withActiveExecutionPlanDraft({ ...session, preferredTarget: resolved }, draft);
    const operation = operationId ?? `plan-${runId}-${Date.now()}`;
    setOperationId(operation);
    const savingSession = withExecutionPlanPhase(base, 'saving');
    commit(savingSession, false);
    const command = {
      projectId,
      taskId,
      taskUuid,
      runId,
      operationId: operation,
      target: resolved,
      ...executionPlanSaveExpectations(session),
      workflow: draft.kind === 'workflow' ? { workflow: draft.workflow, modelBindings: draft.modelBindings } : null,
      autoConfig: draft.kind === 'auto' ? draft.config : null,
    };
    let preflightPassed = false;
    try {
      const preflight = await preflightConversationExecutionPlanSave(command);
      if (preflight.blocking.length > 0) {
        // No journal is staged until preflight passes; this operation id must
        // not be reused for a later attempt with a different command.
        setOperationId(null);
        const notice = executionPlanIssueNotice(preflight.blocking);
        commit(withExecutionPlanPhase(base, 'error', notice.code, notice.nodeIds), false);
        return;
      }
      preflightPassed = true;
      const saved = await saveConversationExecutionPlan(command);
      applyOutcome(savingSession, saved, draft);
    } catch (error) {
      if (!preflightPassed) setOperationId(null);
      const notice = executionPlanThrownNotice(error);
      commit(withExecutionPlanPhase(base, notice.code?.includes('conflict') ? 'conflict' : 'error', notice.code, notice.nodeIds), false);
    }
  };

  const recover = async () => {
    if (!operationId) return;
    const draft = getDraft();
    const base = withActiveExecutionPlanDraft(session, draft);
    commit(withExecutionPlanPhase(base, 'saving'), false);
    try {
      const recovered = await recoverConversationExecutionPlanOperation(projectId, taskId, taskUuid, runId, operationId);
      applyOutcome(base, recovered, draft);
    } catch (error) {
      commit(withExecutionPlanPhase(base, 'partial', errorCodeOf(error)), false);
    }
  };

  return (
    <ExecutionPlanSaveBar
      className="px-0"
      session={session}
      onTargetChange={(target) => {
        writeExecutionPlanSaveTarget(target);
        commit({ ...session, preferredTarget: target }, false);
      }}
      onSave={(target) => { void save(target); }}
      onSelectSide={(side) => commit(selectExecutionPlanSide(withActiveExecutionPlanDraft(session, getDraft()), side), true)}
      onSplit={() => commit(splitExecutionPlanSession(session, getDraft()), true)}
      onOpenUnsplit={() => commit(openExecutionPlanUnsplit(withActiveExecutionPlanDraft(session, getDraft())), false)}
      onCancelUnsplit={() => commit(cancelExecutionPlanUnsplit(session), false)}
      onConfirmUnsplit={(side) => commit(confirmExecutionPlanUnsplit(withActiveExecutionPlanDraft(session, getDraft()), side), true)}
      dirty={executionPlanSessionIsDirty(session, getDraft())}
      onRevert={() => commit(revertExecutionPlanToPersisted(session), true)}
      onReload={() => {
        commit(withExecutionPlanPhase(withActiveExecutionPlanDraft(session, getDraft()), 'loading'), false);
        setReloadNonce((value) => value + 1);
      }}
      onRecover={operationId ? () => { void recover(); } : undefined}
    />
  );
}
