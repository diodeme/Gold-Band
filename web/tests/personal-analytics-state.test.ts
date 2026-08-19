import { describe, expect, it } from 'vitest';
import type { PersonalAnalyticsOperationStatus, PersonalAnalyticsSnapshotVm } from '../src/types';
import { isPersonalAnalyticsActive, mergePersonalAnalyticsSnapshot } from '../src/lib/personal-analytics-state';

function snapshot(
  revision: number,
  status: PersonalAnalyticsOperationStatus,
  insightStatus: PersonalAnalyticsOperationStatus | null = null,
): PersonalAnalyticsSnapshotVm {
  const operation = {
    operationId: 'operation-1',
    agentType: 'agent-a',
    status,
    revision,
    progress: { stage: status, processedUnits: 1, totalUnits: 2 },
    sourceWatermark: '2026-08-17T00:00:00Z',
    reportId: null,
    error: null,
    createdAt: '2026-08-17T00:00:00Z',
    updatedAt: '2026-08-17T00:00:00Z',
    completedAt: null,
  };
  return {
    operation,
    insightOperation: insightStatus ? { ...operation, status: insightStatus } : null,
    latestReport: null,
  };
}

describe('personal analytics snapshot state', () => {
  it('rejects a late lower revision', () => {
    expect(mergePersonalAnalyticsSnapshot(snapshot(4, 'analyzing'), snapshot(3, 'scanning')).operation?.revision).toBe(4);
  });

  it('keeps deterministic sync and insight lifecycles independent', () => {
    const current = snapshot(4, 'scanning', 'analyzing');
    const syncOnly = mergePersonalAnalyticsSnapshot(current, snapshot(5, 'completed'));
    expect(syncOnly.operation?.status).toBe('completed');
    expect(syncOnly.insightOperation?.status).toBe('analyzing');

    const insightOnly = mergePersonalAnalyticsSnapshot(syncOnly, {
      operation: null,
      insightOperation: snapshot(5, 'completed').operation,
      latestReport: null,
    });
    expect(insightOnly.operation?.status).toBe('completed');
    expect(insightOnly.insightOperation?.status).toBe('completed');
  });

  it('rejects a stale same-range report with a lower index revision', () => {
    const currentReport = { range: { start: '2026-08-01', end: '2026-08-18' }, indexRevision: 3 } as NonNullable<PersonalAnalyticsSnapshotVm['latestReport']>;
    const current: PersonalAnalyticsSnapshotVm = {
      ...snapshot(4, 'completed'),
      latestReport: currentReport,
    };
    const incoming: PersonalAnalyticsSnapshotVm = {
      operation: null,
      insightOperation: null,
      latestReport: { ...currentReport, indexRevision: 2 } as NonNullable<PersonalAnalyticsSnapshotVm['latestReport']>,
    };
    expect(mergePersonalAnalyticsSnapshot(current, incoming).latestReport).toBe(currentReport);
  });

  it('keeps the selected-range report during a background sync update', () => {
    const selectedReport = { range: { start: '2026-08-01', end: '2026-08-18' } } as NonNullable<PersonalAnalyticsSnapshotVm['latestReport']>;
    const current: PersonalAnalyticsSnapshotVm = {
      ...snapshot(4, 'scanning'),
      latestReport: selectedReport,
    };
    const incoming: PersonalAnalyticsSnapshotVm = {
      ...snapshot(5, 'completed'),
      latestReport: { range: { start: null, end: null } } as NonNullable<PersonalAnalyticsSnapshotVm['latestReport']>,
    };
    expect(mergePersonalAnalyticsSnapshot(current, incoming).latestReport).toBe(selectedReport);
  });

    it('recognizes only non-terminal operations as active', () => {
    expect(isPersonalAnalyticsActive(snapshot(1, 'validating-report'))).toBe(true);
    expect(isPersonalAnalyticsActive(snapshot(2, 'completed'))).toBe(false);
    expect(isPersonalAnalyticsActive(snapshot(2, 'failed'))).toBe(false);
  });
});
