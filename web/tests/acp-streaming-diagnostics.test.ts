import { describe, expect, it } from 'vitest';
import {
  acpStreamingLocatorMismatches,
  createAcpStreamingDiagnosticBuffer,
  summarizeLongAnimationFrame,
  type AcpStreamingDiagnosticRecord,
} from '@/lib/acp-streaming-diagnostics';

function diagnostic(sequence: number): AcpStreamingDiagnosticRecord {
  return {
    sequence,
    recordedAt: `2026-08-13T00:00:0${sequence}.000Z`,
    elapsedMs: sequence,
    stage: 'router-received',
    details: { eventSeq: sequence },
  };
}

describe('ACP streaming diagnostics', () => {
  it('keeps a bounded copy-safe diagnostic window', () => {
    const buffer = createAcpStreamingDiagnosticBuffer(2);
    buffer.append(diagnostic(1));
    buffer.append(diagnostic(2));
    buffer.append(diagnostic(3));

    const snapshot = buffer.snapshot();
    expect(snapshot.map((entry) => entry.sequence)).toEqual([2, 3]);
    snapshot[0].details.eventSeq = 99;
    expect(buffer.snapshot()[0].details.eventSeq).toBe(2);
  });

  it('reports the exact locator fields that rejected a live event', () => {
    const event = {
      projectId: 'project-a',
      taskId: 'task-223',
      runId: 'run-001',
      roundId: 'round-001',
      nodeId: 'direct-agent',
      attemptId: 'attempt-001',
      outerNodeId: null,
      outerAttemptId: null,
    };
    expect(acpStreamingLocatorMismatches(event, {
      ...event,
      projectId: 'project-b',
      attemptId: 'attempt-002',
    })).toEqual([
      { field: 'projectId', eventValue: 'project-a', locatorValue: 'project-b' },
      { field: 'attemptId', eventValue: 'attempt-001', locatorValue: 'attempt-002' },
    ]);
  });

  it('accepts bounded playback summaries without message text', () => {
    const buffer = createAcpStreamingDiagnosticBuffer(2);
    const playback: AcpStreamingDiagnosticRecord = {
      sequence: 1,
      recordedAt: '2026-08-13T00:00:00.000Z',
      elapsedMs: 100,
      stage: 'markdown-playback-sample',
      details: {
        canonicalLength: 6400,
        unitCount: 6300,
        revealedUnitCount: 400,
        backlog: 5900,
        frameCount: 25,
        longestFrameMs: 62.5,
      },
    };

    buffer.append(playback);

    expect(buffer.snapshot()).toEqual([playback]);
    expect(buffer.snapshot()[0].details).not.toHaveProperty('canonical');
    expect(buffer.snapshot()[0].details).not.toHaveProperty('content');
  });

  it('summarizes long animation frames without script source URLs or message data', () => {
    expect(summarizeLongAnimationFrame({
      startTime: 100,
      duration: 90,
      blockingDuration: 32.26,
      renderStart: 150,
      styleAndLayoutStart: 170,
      firstUIEventTimestamp: 120,
      scripts: [
        {
          duration: 45.55,
          blockingDuration: 12.24,
          forcedStyleAndLayoutDuration: 8.84,
          invokerType: 'event-listener',
          windowAttribution: 'self',
        },
        { duration: 20, invokerType: 'requestAnimationFrame' },
        { duration: 10, invokerType: 'resolve-promise' },
        { duration: 5, invokerType: 'event-listener' },
      ],
    })).toEqual({
      startTimeMs: 100,
      durationMs: 90,
      blockingDurationMs: 32.3,
      renderDurationMs: 40,
      styleAndLayoutDurationMs: 20,
      firstUIEventDelayMs: 70,
      scripts: [
        {
          durationMs: 45.6,
          blockingDurationMs: 12.2,
          forcedStyleAndLayoutDurationMs: 8.8,
          invokerType: 'event-listener',
          windowAttribution: 'self',
        },
        {
          durationMs: 20,
          blockingDurationMs: null,
          forcedStyleAndLayoutDurationMs: null,
          invokerType: 'requestAnimationFrame',
          windowAttribution: null,
        },
        {
          durationMs: 10,
          blockingDurationMs: null,
          forcedStyleAndLayoutDurationMs: null,
          invokerType: 'resolve-promise',
          windowAttribution: null,
        },
      ],
    });
  });
});
