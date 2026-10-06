import type { AcpExecutionErrorVm, ConversationAttemptLifecycleVm } from '@/types';

type AcpFacet = ConversationAttemptLifecycleVm['acp'];

/** The observation never advances or replaces the durable ACP facet. */
export function mergeAcpExecutionError(
  acp: AcpFacet | null | undefined,
  ...candidates: Array<AcpExecutionErrorVm | null | undefined>
): AcpExecutionErrorVm | null {
  for (const failure of candidates) {
    if (!failure) continue;
    if (!acp || (acp.revision ?? 0) < failure.owner.revision) return failure;
    if (acp.turnId === failure.owner.turnId && acp.operationId === failure.owner.operationId
      && acp.latestTurnStatus === 'none') return failure;
  }
  return null;
}

export function currentAcpExecutionError(
  lifecycle: Pick<ConversationAttemptLifecycleVm, 'acp' | 'executionError'> | null | undefined,
): AcpExecutionErrorVm | null {
  const failure = lifecycle?.executionError;
  if (!lifecycle || !failure || lifecycle.acp.turnId !== failure.owner.turnId
    || lifecycle.acp.operationId !== failure.owner.operationId || lifecycle.acp.latestTurnStatus !== 'none') return null;
  return failure;
}
