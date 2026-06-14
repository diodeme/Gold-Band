export interface AcpLiveEventFlushPolicyInput {
  coalescable: boolean;
  paused: boolean;
  deferRemainingMs?: number;
  flushDelayMs?: number;
  hasScheduledFlush: boolean;
}

export interface AcpLiveEventFlushDecision {
  buffer: boolean;
  applyImmediately: boolean;
  flushPendingBeforeApply: boolean;
  scheduleFlush: boolean;
  scheduleDelayMs: number | null;
}

export function decideAcpLiveEventFlush(
  input: AcpLiveEventFlushPolicyInput,
): AcpLiveEventFlushDecision {
  const deferRemainingMs = Math.max(0, input.deferRemainingMs ?? 0);
  const flushDelayMs = Math.max(0, input.flushDelayMs ?? 0);
  const deferred = deferRemainingMs > 0;

  if (!input.coalescable) {
    return {
      buffer: false,
      applyImmediately: true,
      flushPendingBeforeApply: !input.paused && !deferred,
      scheduleFlush: false,
      scheduleDelayMs: null,
    };
  }

  const scheduleFlush = !input.paused && !input.hasScheduledFlush;
  return {
    buffer: true,
    applyImmediately: false,
    flushPendingBeforeApply: false,
    scheduleFlush,
    scheduleDelayMs: scheduleFlush
      ? Math.max(flushDelayMs, deferRemainingMs)
      : null,
  };
}

export function shouldAutoScrollAfterAcpTimelineUpdate(input: {
  pinned: boolean;
  deferRemainingMs?: number;
}) {
  return input.pinned && Math.max(0, input.deferRemainingMs ?? 0) === 0;
}
