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

export interface AcpLiveEventLike {
  kind: string;
  toolCallId?: string | null;
  status?: string | null;
}

export interface AcpMergeableLiveToolEvent extends AcpLiveEventLike {
  id?: string;
  seq?: number;
  timestamp?: string;
  title?: string | null;
  content?: string | null;
  startedSeq?: number | null;
  endedSeq?: number | null;
  startedAt?: string | null;
  endedAt?: string | null;
  raw?: unknown;
}

const coalescableTextEventKinds = new Set(["textDelta", "thoughtDelta", "plan"]);
const toolEventKinds = new Set(["toolCall", "toolCallUpdate"]);
const terminalToolStatuses = new Set([
  "completed",
  "success",
  "succeeded",
  "failed",
  "error",
  "cancelled",
  "canceled",
]);

export function isTerminalAcpToolStatus(status?: string | null) {
  return terminalToolStatuses.has(status?.toLowerCase() ?? "");
}

export function isAcpLiveToolEvent(event: AcpLiveEventLike) {
  return toolEventKinds.has(event.kind) && Boolean(event.toolCallId);
}

export function isCoalescableAcpLiveEvent(event: AcpLiveEventLike) {
  if (coalescableTextEventKinds.has(event.kind)) return true;
  return isAcpLiveToolEvent(event) && !isTerminalAcpToolStatus(event.status);
}

export function mergeAcpLiveToolEvent<T extends AcpMergeableLiveToolEvent>(
  previous: T | null | undefined,
  next: T,
  mergeRaw?: (previous: unknown, next: unknown) => unknown,
): T {
  if (!previous || !isSameAcpLiveToolEvent(previous, next)) return next;
  return {
    ...previous,
    ...next,
    title: next.title ?? previous.title,
    content: next.content ?? previous.content,
    startedSeq: previous.startedSeq ?? next.startedSeq,
    startedAt: previous.startedAt ?? next.startedAt,
    endedSeq: next.endedSeq ?? previous.endedSeq,
    endedAt: next.endedAt ?? next.timestamp ?? previous.endedAt,
    raw: mergeRaw ? mergeRaw(previous.raw, next.raw) : next.raw ?? previous.raw,
  } as T;
}

function isSameAcpLiveToolEvent(
  previous: AcpLiveEventLike,
  next: AcpLiveEventLike,
) {
  return (
    isAcpLiveToolEvent(previous) &&
    isAcpLiveToolEvent(next) &&
    previous.toolCallId === next.toolCallId
  );
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
