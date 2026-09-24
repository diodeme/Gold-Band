import type { ExecutionPlanSaveTarget, ExecutionPlanViewVm } from '@/types';

export type ExecutionPlanSide = 'current' | 'next';
export type ExecutionPlanPhase = 'ready' | 'loading' | 'preflight' | 'saving' | 'conflict' | 'partial' | 'error';

export interface ExecutionPlanBaselines<T> {
  current: T;
  next: T;
  planRevision: number;
  authoringRevision: number;
  executionRevision: number;
  currentEditable: boolean;
  diverged: boolean;
  runStatus: string;
  currentRound: string | null;
  currentNode: string | null;
  currentAttempt: string | null;
}

export interface ExecutionPlanTargetOutcome {
  target: ExecutionPlanSaveTarget;
  committed: boolean;
  errorCode?: string | null;
}

export interface ExecutionPlanSaveOutcome<T> {
  complete: boolean;
  /** Post-commit comparison of Current and Next. Missing values are treated as still equal. */
  diverged?: boolean;
  planRevision: number;
  authoringRevision: number;
  executionRevision: number;
  targets: ExecutionPlanTargetOutcome[];
  savedDraft: T;
}

export interface ExecutionPlanSession<T> {
  split: boolean;
  activeSide: ExecutionPlanSide;
  unifiedDraft: T;
  currentDraft: T;
  nextDraft: T;
  currentBaseline: T;
  nextBaseline: T;
  planRevision: number;
  authoringRevision: number;
  executionRevision: number;
  currentEditable: boolean;
  diverged: boolean;
  runStatus: string;
  currentRound: string | null;
  currentNode: string | null;
  currentAttempt: string | null;
  preferredTarget: ExecutionPlanSaveTarget;
  phase: ExecutionPlanPhase;
  errorCode: string | null;
  errorNodeIds: string[];
  partialTargets: ExecutionPlanTargetOutcome[];
  unsplitOpen: boolean;
}

export function executionPlanBaselinesFromView<T>(
  view: Pick<ExecutionPlanViewVm, 'planRevision' | 'authoringRevision' | 'executionRevision' | 'currentEditable' | 'diverged' | 'runStatus' | 'currentRound' | 'currentNode' | 'currentAttempt'>,
  current: T,
  next: T,
): ExecutionPlanBaselines<T> {
  return {
    current,
    next,
    planRevision: view.planRevision,
    authoringRevision: view.authoringRevision,
    executionRevision: view.executionRevision,
    currentEditable: view.currentEditable,
    diverged: view.diverged,
    runStatus: view.runStatus,
    currentRound: view.currentRound ?? null,
    currentNode: view.currentNode ?? null,
    currentAttempt: view.currentAttempt ?? null,
  };
}

export function executionPlanSaveExpectations(session: Pick<ExecutionPlanSession<unknown>, 'planRevision' | 'authoringRevision' | 'runStatus' | 'currentRound' | 'currentNode' | 'currentAttempt'>) {
  return {
    expectedPlanRevision: session.planRevision,
    expectedAuthoringRevision: session.authoringRevision,
    expectedRunStatus: session.runStatus,
    expectedCurrentRound: session.currentRound ?? null,
    expectedCurrentNode: session.currentNode ?? null,
    expectedCurrentAttempt: session.currentAttempt ?? null,
  };
}

export function createExecutionPlanSession<T>(
  baselines: ExecutionPlanBaselines<T>,
  preferredTarget: ExecutionPlanSaveTarget = 'next',
): ExecutionPlanSession<T> {
  const target = baselines.currentEditable ? preferredTarget : 'next';
  return {
    split: baselines.diverged,
    activeSide: 'next',
    unifiedDraft: baselines.next,
    currentDraft: baselines.current,
    nextDraft: baselines.next,
    currentBaseline: baselines.current,
    nextBaseline: baselines.next,
    planRevision: baselines.planRevision,
    authoringRevision: baselines.authoringRevision,
    executionRevision: baselines.executionRevision,
    currentEditable: baselines.currentEditable,
    diverged: baselines.diverged,
    runStatus: baselines.runStatus,
    currentRound: baselines.currentRound,
    currentNode: baselines.currentNode,
    currentAttempt: baselines.currentAttempt,
    preferredTarget: target,
    phase: 'ready',
    errorCode: null,
    errorNodeIds: [],
    partialTargets: [],
    unsplitOpen: false,
  };
}

export function executionPlanIssueNotice(
  issues: ReadonlyArray<{ code?: string | null; context?: Record<string, unknown> | null }>,
): { code: string | null; nodeIds: string[] } {
  const code = issues.find((issue) => typeof issue.code === 'string' && issue.code.length > 0)?.code ?? null;
  if (!code) return { code: null, nodeIds: [] };
  const nodeIds: string[] = [];
  for (const issue of issues) {
    if (issue.code !== code) continue;
    const nodeId = issue.context?.nodeId;
    if (typeof nodeId === 'string' && nodeId.length > 0 && !nodeIds.includes(nodeId)) nodeIds.push(nodeId);
  }
  return { code, nodeIds };
}

export function executionPlanThrownNotice(error: unknown): { code: string | null; nodeIds: string[] } {
  if (!error || typeof error !== 'object' || !('code' in error)) return { code: null, nodeIds: [] };
  const code = String((error as { code: unknown }).code);
  const record = error as { params?: unknown; context?: unknown };
  const source = record.params && typeof record.params === 'object'
    ? record.params as Record<string, unknown>
    : record.context && typeof record.context === 'object'
      ? record.context as Record<string, unknown>
      : null;
  const nodeId = source && typeof source.nodeId === 'string' ? source.nodeId : null;
  return { code, nodeIds: nodeId ? [nodeId] : [] };
}

export function executionPlanTargets<T>(session: ExecutionPlanSession<T>): ExecutionPlanSaveTarget[] {
  if (!session.currentEditable) return ['next'];
  if (!session.split) return ['current', 'next', 'current-and-next'];
  if (session.activeSide === 'current') return ['current', 'current-and-next'];
  return ['next', 'current-and-next'];
}

export function resolveExecutionPlanTarget<T>(
  session: ExecutionPlanSession<T>,
  requested: ExecutionPlanSaveTarget,
): ExecutionPlanSaveTarget {
  const available = executionPlanTargets(session);
  if (available.includes(requested)) return requested;
  return available[0] ?? 'next';
}

export function activeExecutionPlanDraft<T>(session: ExecutionPlanSession<T>): T {
  if (!session.split) return session.unifiedDraft;
  return session.activeSide === 'current' ? session.currentDraft : session.nextDraft;
}

export function activeExecutionPlanBaseline<T>(session: ExecutionPlanSession<T>): T {
  if (session.split) return session.activeSide === 'current' ? session.currentBaseline : session.nextBaseline;
  return session.preferredTarget === 'current' ? session.currentBaseline : session.nextBaseline;
}

function executionPlanDraftsMatch<T>(left: T, right: T): boolean {
  return JSON.stringify(left) === JSON.stringify(right)
    || JSON.stringify(canonicalizePlanDraft(left)) === JSON.stringify(canonicalizePlanDraft(right));
}

/** Drops null, empty-string, and empty-object fields so a form projection matches a stored AUTO config. */
function canonicalizePlanDraft(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalizePlanDraft);
  if (!value || typeof value !== 'object') return value;
  const record = value as Record<string, unknown>;
  if ('nodes' in record && 'edges' in record) return value;
  const canonical: Record<string, unknown> = {};
  for (const key of Object.keys(record).sort()) {
    const child = canonicalizePlanDraft(record[key]);
    if (child == null || child === '') continue;
    if (key === 'autoAccept' && child === false) continue;
    if (typeof child === 'object' && !Array.isArray(child) && Object.keys(child).length === 0) continue;
    canonical[key] = child;
  }
  return canonical;
}

/**
 * Replaces baselines and canonical revisions with freshly loaded facts.
 * A side whose draft still matches its previous baseline adopts the new fact,
 * so every run follows the shared next authoring. A dirty draft stays whole.
 */
export function rebaseExecutionPlanSession<T>(
  session: ExecutionPlanSession<T>,
  baselines: ExecutionPlanBaselines<T>,
): ExecutionPlanSession<T> {
  const nextDraft = executionPlanDraftsMatch(session.nextDraft, session.nextBaseline)
    ? baselines.next
    : session.nextDraft;
  const currentDraft = executionPlanDraftsMatch(session.currentDraft, session.currentBaseline)
    ? baselines.current
    : session.currentDraft;
  const unifiedDraft = session.split
    ? session.unifiedDraft
    : executionPlanDraftsMatch(session.unifiedDraft, session.nextBaseline)
      ? baselines.next
      : session.unifiedDraft;
  const userKeptOneCopy = !session.split
    && executionPlanDraftsMatch(session.currentDraft, session.nextDraft)
    && executionPlanDraftsMatch(session.unifiedDraft, session.nextDraft);
  return {
    ...session,
    unifiedDraft,
    currentDraft: session.split ? currentDraft : unifiedDraft,
    nextDraft: session.split ? nextDraft : unifiedDraft,
    currentBaseline: baselines.current,
    nextBaseline: baselines.next,
    planRevision: baselines.planRevision,
    authoringRevision: baselines.authoringRevision,
    executionRevision: baselines.executionRevision,
    currentEditable: baselines.currentEditable,
    diverged: userKeptOneCopy ? false : baselines.diverged,
    runStatus: baselines.runStatus,
    currentRound: baselines.currentRound,
    currentNode: baselines.currentNode,
    currentAttempt: baselines.currentAttempt,
    preferredTarget: baselines.currentEditable ? session.preferredTarget : 'next',
    phase: 'ready',
    errorCode: null,
    errorNodeIds: [],
    partialTargets: [],
  };
}

export function withActiveExecutionPlanDraft<T>(session: ExecutionPlanSession<T>, draft: T): ExecutionPlanSession<T> {
  if (!session.split) {
    return { ...session, unifiedDraft: draft, currentDraft: draft, nextDraft: draft };
  }
  if (session.activeSide === 'current') return { ...session, currentDraft: draft };
  return { ...session, nextDraft: draft };
}

export function withExecutionPlanPhase<T>(
  session: ExecutionPlanSession<T>,
  phase: ExecutionPlanPhase,
  errorCode: string | null = null,
  errorNodeIds: readonly string[] = [],
): ExecutionPlanSession<T> {
  return { ...session, phase, errorCode, errorNodeIds: [...errorNodeIds] };
}

export function applyExecutionPlanSave<T>(
  session: ExecutionPlanSession<T>,
  outcome: ExecutionPlanSaveOutcome<T>,
): ExecutionPlanSession<T> {
  const conflict = outcome.targets.find((target) => target.errorCode?.includes('conflict'));
  if (conflict && !outcome.complete) {
    return {
      ...session,
      phase: 'conflict',
      errorCode: conflict.errorCode ?? 'conversation.execution-plan.revision-conflict',
      errorNodeIds: [],
      partialTargets: outcome.targets,
      planRevision: outcome.planRevision,
      authoringRevision: outcome.authoringRevision,
      executionRevision: outcome.executionRevision,
    };
  }
  const currentCommitted = outcome.targets.some((target) => target.target === 'current' && target.committed);
  const nextCommitted = outcome.targets.some((target) => target.target === 'next' && target.committed);
  const next: ExecutionPlanSession<T> = {
    ...session,
    planRevision: outcome.planRevision,
    authoringRevision: outcome.authoringRevision,
    executionRevision: outcome.executionRevision,
    partialTargets: outcome.targets,
    phase: outcome.complete ? 'ready' : 'partial',
    errorCode: outcome.complete ? null : outcome.targets.find((target) => target.errorCode)?.errorCode ?? 'conversation.execution-plan.partial-commit',
    errorNodeIds: [],
    unsplitOpen: false,
  };
  if (!outcome.diverged) {
    return {
      ...next,
      split: false,
      activeSide: 'next',
      unifiedDraft: outcome.savedDraft,
      currentDraft: outcome.savedDraft,
      nextDraft: outcome.savedDraft,
      currentBaseline: outcome.savedDraft,
      nextBaseline: outcome.savedDraft,
      diverged: false,
    };
  }
  if (currentCommitted && nextCommitted) {
    return {
      ...next,
      split: true,
      activeSide: session.split ? session.activeSide : 'next',
      unifiedDraft: outcome.savedDraft,
      currentDraft: outcome.savedDraft,
      nextDraft: outcome.savedDraft,
      currentBaseline: outcome.savedDraft,
      nextBaseline: outcome.savedDraft,
      diverged: true,
    };
  }
  if (currentCommitted) {
    const submittedToNext = outcome.targets.some((target) => target.target === 'next');
    return {
      ...next,
      split: true,
      activeSide: 'current',
      currentDraft: outcome.savedDraft,
      currentBaseline: outcome.savedDraft,
      nextDraft: submittedToNext || session.split ? session.nextDraft : session.nextBaseline,
      diverged: true,
    };
  }
  if (nextCommitted) {
    const submittedToCurrent = outcome.targets.some((target) => target.target === 'current');
    return {
      ...next,
      split: true,
      activeSide: 'next',
      nextDraft: outcome.savedDraft,
      nextBaseline: outcome.savedDraft,
      currentDraft: submittedToCurrent || session.split ? session.currentDraft : session.currentBaseline,
      diverged: true,
    };
  }
  return { ...next, split: true, diverged: true };
}

export function handoffExecutionPlanSide<T>(
  session: ExecutionPlanSession<T>,
  leavingDraft: T,
  side: ExecutionPlanSide,
): ExecutionPlanSession<T> {
  const stored = withActiveExecutionPlanDraft(session, leavingDraft);
  if (stored.split && stored.activeSide === side) return stored;
  return selectExecutionPlanSide(stored, side);
}

export function selectExecutionPlanSide<T>(
  session: ExecutionPlanSession<T>,
  side: ExecutionPlanSide,
): ExecutionPlanSession<T> {
  return { ...session, split: true, activeSide: side, unsplitOpen: false };
}

/**
 * Opens the two sides without copying the unified editor onto Current.
 * Unsplit editing follows Next, so the live draft stays on that side.
 */
export function splitExecutionPlanSession<T>(session: ExecutionPlanSession<T>, unifiedDraft: T): ExecutionPlanSession<T> {
  if (session.split) return session;
  return {
    ...session,
    split: true,
    activeSide: 'next',
    unifiedDraft,
    nextDraft: unifiedDraft,
    unsplitOpen: false,
  };
}

export function openExecutionPlanUnsplit<T>(session: ExecutionPlanSession<T>): ExecutionPlanSession<T> {
  return { ...session, unsplitOpen: true };
}

export function cancelExecutionPlanUnsplit<T>(session: ExecutionPlanSession<T>): ExecutionPlanSession<T> {
  return { ...session, unsplitOpen: false };
}

export function executionPlanSessionIsDirty<T>(session: ExecutionPlanSession<T>, liveDraft: T) {
  const withLive = withActiveExecutionPlanDraft(session, liveDraft);
  if (withLive.split) {
    return !executionPlanDraftsMatch(withLive.currentDraft, session.currentBaseline)
      || !executionPlanDraftsMatch(withLive.nextDraft, session.nextBaseline);
  }
  return !executionPlanDraftsMatch(withLive.unifiedDraft, session.currentBaseline)
    || !executionPlanDraftsMatch(withLive.unifiedDraft, session.nextBaseline);
}

/** Puts both sides back on their persisted baselines. Different baselines open as two tabs again. */
export function revertExecutionPlanToPersisted<T>(session: ExecutionPlanSession<T>): ExecutionPlanSession<T> {
  const diverged = !executionPlanDraftsMatch(session.currentBaseline, session.nextBaseline);
  const keepPhase = session.phase === 'conflict' || session.phase === 'partial';
  return {
    ...session,
    split: diverged,
    diverged,
    activeSide: session.preferredTarget === 'current' ? 'current' : 'next',
    unifiedDraft: session.nextBaseline,
    currentDraft: session.currentBaseline,
    nextDraft: session.nextBaseline,
    unsplitOpen: false,
    phase: keepPhase ? session.phase : 'ready',
    errorCode: keepPhase ? session.errorCode : null,
    errorNodeIds: keepPhase ? session.errorNodeIds : [],
  };
}

export function confirmExecutionPlanUnsplit<T>(
  session: ExecutionPlanSession<T>,
  keep: ExecutionPlanSide,
): ExecutionPlanSession<T> {
  const draft = keep === 'current' ? session.currentDraft : session.nextDraft;
  return {
    ...session,
    unsplitOpen: false,
    split: false,
    diverged: false,
    activeSide: 'next',
    unifiedDraft: draft,
    currentDraft: draft,
    nextDraft: draft,
    preferredTarget: keep === 'current' ? 'current' : 'next',
  };
}
