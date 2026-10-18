import { describe, expect, it, vi } from 'vitest';
import { createSourceControlLoadDiagnostic } from '../src/lib/source-control-load-diagnostics';

describe('source control load diagnostics', () => {
  it('records failed stages and reports only once without waiting for log delivery', async () => {
    let time = 0;
    const report = vi.fn(() => new Promise<void>(() => {}));
    const diagnostic = createSourceControlLoadDiagnostic(true, report, () => time);
    await expect(diagnostic.measure('capabilityMs', async () => {
      time = 125;
      throw new Error('failure');
    })).rejects.toThrow('failure');
    time = 150;
    diagnostic.finish('error');
    diagnostic.finish('ready');
    expect(report).toHaveBeenCalledExactlyOnceWith({
      loadId: diagnostic.loadId, initial: true, outcome: 'error', elapsedMs: 150, capabilityMs: 125,
    });
  });

  it('isolates concurrent loads and ignores synchronous and asynchronous reporting failures', async () => {
    const first = createSourceControlLoadDiagnostic(true, () => { throw new Error('offline'); });
    const second = createSourceControlLoadDiagnostic(false, async () => { throw new Error('offline'); });
    expect(first.loadId).not.toBe(second.loadId);
    expect(() => first.finish('ready')).not.toThrow();
    expect(() => second.finish('superseded')).not.toThrow();
    await Promise.resolve();
  });
});
