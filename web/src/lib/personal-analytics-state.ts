import type { PersonalAnalyticsSnapshotVm } from '@/types';

export function mergePersonalAnalyticsSnapshot(
  current: PersonalAnalyticsSnapshotVm | null,
  incoming: PersonalAnalyticsSnapshotVm,
): PersonalAnalyticsSnapshotVm {
  const fallback = current ?? incoming;
  let operation = incoming.operation ?? current?.operation ?? null;
  if (current?.operation && operation && operation.operationId === current.operation.operationId
    && operation.revision < current.operation.revision) {
    operation = current.operation;
  }
  let insightOperation = incoming.insightOperation ?? current?.insightOperation ?? null;
  if (current?.insightOperation && insightOperation
    && insightOperation.operationId === current.insightOperation.operationId
    && insightOperation.revision < current.insightOperation.revision) {
    insightOperation = current.insightOperation;
  }
  const incomingReport = incoming.latestReport;
  const currentReport = current?.latestReport ?? null;
  const incomingReportIsStale = incomingReport !== null
    && currentReport !== null
    && sameRange(incomingReport.range, currentReport.range)
    && incomingReport.indexRevision < currentReport.indexRevision;
  const latestReport = incomingReport
    && currentReport
    && (!sameRange(incomingReport.range, currentReport.range) || incomingReportIsStale)
    ? currentReport
    : incomingReport ?? fallback.latestReport;
  return { operation, insightOperation, latestReport };
}

function sameRange(
  left: { start: string | null; end: string | null } | null | undefined,
  right: { start: string | null; end: string | null } | null | undefined,
) {
  return left?.start === right?.start && left?.end === right?.end;
}

export function isPersonalAnalyticsActive(snapshot: PersonalAnalyticsSnapshotVm | null): boolean {
  const status = snapshot?.operation?.status;
  return status === 'queued'
    || status === 'scanning'
    || status === 'analyzing'
    || status === 'validating-report'
    || status === 'cancelling';
}
