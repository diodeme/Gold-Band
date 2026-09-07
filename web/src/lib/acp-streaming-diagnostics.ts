import type { AcpSessionUpdatedEventVm } from '@/api/client';

const ACP_STREAMING_DEBUG_KEY = 'goldBand.debug.acpStreaming';
const ACP_STREAMING_DIAGNOSTIC_LIMIT = 2_000;
const ACP_STREAMING_LONG_ANIMATION_FRAME_MS = 50;
const ACP_STREAMING_LONG_ANIMATION_FRAME_SCRIPT_LIMIT = 3;

export type AcpStreamingDiagnosticStage =
  | 'router-received'
  | 'locator-match'
  | 'animation-readiness'
  | 'replay-catch-up'
  | 'target-decision'
  | 'markdown-render'
  | 'markdown-playback-init'
  | 'markdown-playback-reconcile'
  | 'markdown-playback-sample'
  | 'markdown-playback-long-frame'
  | 'markdown-playback-settle'
  | 'browser-long-animation-frame'
  | 'chat-layout-sample'
  | 'chat-follow-sample'
  | 'chat-scroll-trace'
  | 'return-to-latest-trace'
  | 'stream-settle';

export type AcpStreamingDiagnosticRecord = {
  sequence: number;
  recordedAt: string;
  elapsedMs: number | null;
  stage: AcpStreamingDiagnosticStage;
  details: Record<string, unknown>;
};

export type AcpStreamingAttemptLocator = Pick<
  AcpSessionUpdatedEventVm,
  | 'projectId'
  | 'taskId'
  | 'runId'
  | 'roundId'
  | 'nodeId'
  | 'attemptId'
  | 'outerNodeId'
  | 'outerAttemptId'
>;

type AcpStreamingDiagnosticBridge = {
  clear: () => void;
  exportJson: () => string;
  snapshot: () => AcpStreamingDiagnosticRecord[];
};

declare global {
  interface Window {
    __goldBandAcpStreamingDiagnostics?: AcpStreamingDiagnosticBridge;
  }
}

type LongAnimationFrameScriptLike = {
  blockingDuration?: number;
  duration?: number;
  forcedStyleAndLayoutDuration?: number;
  invokerType?: string;
  windowAttribution?: string;
};

type LongAnimationFrameLike = {
  blockingDuration?: number;
  duration: number;
  firstUIEventTimestamp?: number;
  renderStart?: number;
  scripts?: LongAnimationFrameScriptLike[];
  startTime: number;
  styleAndLayoutStart?: number;
};

export function createAcpStreamingDiagnosticBuffer(limit: number) {
  const capacity = Math.max(1, Math.floor(limit));
  const records: AcpStreamingDiagnosticRecord[] = [];
  return {
    append(record: AcpStreamingDiagnosticRecord) {
      records.push(record);
      if (records.length > capacity) records.splice(0, records.length - capacity);
    },
    clear() {
      records.splice(0, records.length);
    },
    snapshot() {
      return records.map((record) => ({
        ...record,
        details: { ...record.details },
      }));
    },
  };
}

const diagnosticBuffer = createAcpStreamingDiagnosticBuffer(
  ACP_STREAMING_DIAGNOSTIC_LIMIT,
);
let diagnosticSequence = 0;
const acpStreamingDiagnosticsEnabled = readAcpStreamingDiagnosticsEnabled();

export function isAcpStreamingDiagnosticsEnabled() {
  return acpStreamingDiagnosticsEnabled;
}

function readAcpStreamingDiagnosticsEnabled() {
  if (typeof window === 'undefined') return false;
  try {
    return window.localStorage.getItem(ACP_STREAMING_DEBUG_KEY) === '1';
  } catch {
    return false;
  }
}

export function recordAcpStreamingDiagnostic(
  stage: AcpStreamingDiagnosticStage,
  createDetails: () => Record<string, unknown>,
) {
  if (!isAcpStreamingDiagnosticsEnabled()) return;
  installAcpStreamingDiagnosticBridge();
  const record: AcpStreamingDiagnosticRecord = {
    sequence: ++diagnosticSequence,
    recordedAt: new Date().toISOString(),
    elapsedMs:
      typeof performance === 'undefined' ? null : Math.round(performance.now()),
    stage,
    details: createDetails(),
  };
  diagnosticBuffer.append(record);
}

export function summarizeLongAnimationFrame(entry: LongAnimationFrameLike) {
  const frameEnd = entry.startTime + entry.duration;
  const renderStart = finiteOrNull(entry.renderStart);
  const styleAndLayoutStart = finiteOrNull(entry.styleAndLayoutStart);
  const scripts = [...(entry.scripts ?? [])]
    .sort((left, right) => (right.duration ?? 0) - (left.duration ?? 0))
    .slice(0, ACP_STREAMING_LONG_ANIMATION_FRAME_SCRIPT_LIMIT)
    .map((script) => ({
      durationMs: roundDiagnosticDuration(script.duration),
      blockingDurationMs: roundDiagnosticDuration(script.blockingDuration),
      forcedStyleAndLayoutDurationMs: roundDiagnosticDuration(
        script.forcedStyleAndLayoutDuration,
      ),
      invokerType: script.invokerType ?? null,
      windowAttribution: script.windowAttribution ?? null,
    }));
  return {
    startTimeMs: roundDiagnosticDuration(entry.startTime),
    durationMs: roundDiagnosticDuration(entry.duration),
    blockingDurationMs: roundDiagnosticDuration(entry.blockingDuration),
    renderDurationMs: renderStart === null
      ? null
      : roundDiagnosticDuration(frameEnd - renderStart),
    styleAndLayoutDurationMs: styleAndLayoutStart === null
      ? null
      : roundDiagnosticDuration(frameEnd - styleAndLayoutStart),
    firstUIEventDelayMs:
      Number.isFinite(entry.firstUIEventTimestamp)
      && Number(entry.firstUIEventTimestamp) > 0
        ? roundDiagnosticDuration(
          frameEnd - Number(entry.firstUIEventTimestamp),
        )
        : null,
    scripts,
  };
}

export function summarizeAcpStreamingEvent(event: AcpSessionUpdatedEventVm) {
  return {
    projectId: event.projectId ?? null,
    taskId: event.taskId,
    runId: event.runId,
    roundId: event.roundId,
    nodeId: event.nodeId,
    attemptId: event.attemptId,
    outerNodeId: event.outerNodeId ?? null,
    outerAttemptId: event.outerAttemptId ?? null,
    branchId: event.branchId ?? 'root',
    payload: event.event ? 'event' : event.session ? 'session' : 'lifecycle',
    eventKind: event.event?.kind ?? null,
    eventId: event.event?.id ?? null,
    eventSeq: event.event?.seq ?? null,
    eventEndedSeq: event.event?.endedSeq ?? null,
    contentLength: event.event?.content?.length ?? null,
    sessionStatus: event.session?.status ?? null,
  };
}

export function acpStreamingLocatorMismatches(
  event: AcpStreamingAttemptLocator,
  locator: AcpStreamingAttemptLocator,
) {
  const fields = [
    'projectId',
    'taskId',
    'runId',
    'roundId',
    'nodeId',
    'attemptId',
    'outerNodeId',
    'outerAttemptId',
  ] as const;
  return fields.flatMap((field) => {
    const eventValue = event[field] ?? null;
    const locatorValue = locator[field] ?? null;
    return eventValue === locatorValue
      ? []
      : [{ field, eventValue, locatorValue }];
  });
}

function installAcpStreamingDiagnosticBridge() {
  if (typeof window === 'undefined' || window.__goldBandAcpStreamingDiagnostics) {
    return;
  }
  window.__goldBandAcpStreamingDiagnostics = {
    clear: () => diagnosticBuffer.clear(),
    snapshot: () => diagnosticBuffer.snapshot(),
    exportJson: () => JSON.stringify({
      version: 1,
      exportedAt: new Date().toISOString(),
      records: diagnosticBuffer.snapshot(),
    }, null, 2),
  };
}

function installAcpStreamingLongAnimationFrameObserver() {
  if (
    !isAcpStreamingDiagnosticsEnabled()
    || typeof PerformanceObserver === 'undefined'
    || !PerformanceObserver.supportedEntryTypes?.includes('long-animation-frame')
  ) return null;
  const observer = new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      if (entry.duration < ACP_STREAMING_LONG_ANIMATION_FRAME_MS) continue;
      recordAcpStreamingDiagnostic('browser-long-animation-frame', () => (
        summarizeLongAnimationFrame(entry as unknown as LongAnimationFrameLike)
      ));
    }
  });
  try {
    observer.observe({ type: 'long-animation-frame', buffered: true });
    return observer;
  } catch {
    observer.disconnect();
    return null;
  }
}

function finiteOrNull(value: number | undefined) {
  return Number.isFinite(value) ? Number(value) : null;
}

function roundDiagnosticDuration(value: number | undefined) {
  return Number.isFinite(value) ? Math.round(Number(value) * 10) / 10 : null;
}

if (isAcpStreamingDiagnosticsEnabled()) {
  installAcpStreamingDiagnosticBridge();
  const longAnimationFrameObserver = installAcpStreamingLongAnimationFrameObserver();
  window.addEventListener('pagehide', () => longAnimationFrameObserver?.disconnect(), {
    once: true,
  });
}
