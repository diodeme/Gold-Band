export interface SourceControlLoadReport {
  loadId: string;
  initial: boolean;
  outcome: 'ready' | 'unavailable' | 'error' | 'superseded';
  elapsedMs: number;
  capabilityMs?: number;
  subscriptionsMs?: number;
  monitorMs?: number;
  snapshotMs?: number;
}

type Stage = 'capabilityMs' | 'subscriptionsMs' | 'monitorMs' | 'snapshotMs';

/** Measures data publication, not browser paint. Reporting never blocks loading. */
export function createSourceControlLoadDiagnostic(
  initial: boolean,
  report?: (report: SourceControlLoadReport) => Promise<void>,
  now = () => performance.now(),
) {
  const loadId = crypto.randomUUID();
  const started = now();
  const stages: Partial<Record<Stage, number>> = {};
  let finished = false;
  return {
    loadId,
    async measure<T>(stage: Stage, action: () => Promise<T>): Promise<T> {
      const started = now();
      try { return await action(); }
      finally { stages[stage] = now() - started; }
    },
    finish(outcome: SourceControlLoadReport['outcome']) {
      if (finished) return;
      finished = true;
      try {
        void report?.({ loadId, initial, outcome, elapsedMs: now() - started, ...stages })
          .catch(() => undefined);
      } catch { /* Diagnostics must not change the load result. */ }
    },
  };
}
