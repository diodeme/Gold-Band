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

export interface AcpMergeableLiveStreamEvent extends AcpLiveEventLike {
  id?: string;
  seq?: number;
  timestamp?: string;
  content?: string | null;
  startedSeq?: number | null;
  endedSeq?: number | null;
  startedAt?: string | null;
  endedAt?: string | null;
  raw?: unknown;
}

const acpTextStreamEventKinds = new Set(["textDelta", "thoughtDelta"]);
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

export function isAcpTextStreamEventKind(kind: string) {
  return acpTextStreamEventKinds.has(kind);
}

export function isCoalescableAcpLiveEvent(event: AcpLiveEventLike) {
  if (coalescableTextEventKinds.has(event.kind)) return true;
  return isAcpLiveToolEvent(event) && !isTerminalAcpToolStatus(event.status);
}

export function mergeAcpLiveStreamEvent<T extends AcpMergeableLiveStreamEvent>(
  previous: T | null | undefined,
  next: T,
  mergeRaw?: (previous: unknown, next: unknown) => unknown,
): T {
  if (!previous || !isSameAcpLiveStream(previous, next)) return next;
  const previousContent = previous.content ?? "";
  const nextContent = next.content ?? "";
  const stale = compareAcpLiveStreamPosition(previous, next) > 0;
  if (stale) {
    return {
      ...previous,
      raw: mergeRaw ? mergeRaw(previous.raw, next.raw) : previous.raw ?? next.raw,
    } as T;
  }
  const keepPreviousContent =
    previousContent.length > 0 &&
    (nextContent.length === 0 || nextContent.length < previousContent.length);
  return {
    ...previous,
    ...next,
    content: keepPreviousContent ? previous.content : next.content ?? previous.content,
    startedSeq: previous.startedSeq ?? next.startedSeq,
    startedAt: previous.startedAt ?? next.startedAt,
    endedSeq: newerNumber(previous.endedSeq, next.endedSeq),
    endedAt: newerTimestamp(previous.endedAt ?? previous.timestamp, next.endedAt ?? next.timestamp) ?? next.endedAt ?? previous.endedAt,
    raw: mergeRaw ? mergeRaw(previous.raw, next.raw) : next.raw ?? previous.raw,
  } as T;
}

function isSameAcpLiveStream(
  previous: AcpMergeableLiveStreamEvent,
  next: AcpMergeableLiveStreamEvent,
) {
  return (
    isAcpTextStreamEventKind(previous.kind) &&
    isAcpTextStreamEventKind(next.kind) &&
    previous.kind === next.kind &&
    Boolean(previous.id) &&
    previous.id === next.id
  );
}

function compareAcpLiveStreamPosition(
  previous: AcpMergeableLiveStreamEvent,
  next: AcpMergeableLiveStreamEvent,
) {
  const previousSeq = previous.endedSeq ?? previous.seq;
  const nextSeq = next.endedSeq ?? next.seq;
  if (previousSeq != null && nextSeq != null && previousSeq !== nextSeq) {
    return previousSeq - nextSeq;
  }
  const previousTime = parseAcpLiveTimestamp(previous.endedAt ?? previous.timestamp);
  const nextTime = parseAcpLiveTimestamp(next.endedAt ?? next.timestamp);
  if (previousTime != null && nextTime != null && previousTime !== nextTime) {
    return previousTime - nextTime;
  }
  return 0;
}

function newerNumber(previous?: number | null, next?: number | null) {
  if (previous == null) return next;
  if (next == null) return previous;
  return Math.max(previous, next);
}

function newerTimestamp(previous?: string | null, next?: string | null) {
  const previousTime = parseAcpLiveTimestamp(previous);
  const nextTime = parseAcpLiveTimestamp(next);
  if (previousTime == null) return next;
  if (nextTime == null) return previous;
  return nextTime >= previousTime ? next : previous;
}

function parseAcpLiveTimestamp(value?: string | null) {
  if (!value) return null;
  const numeric = value.match(/^(\d+(?:\.\d+)?)Z?$/);
  if (numeric) return Number(numeric[1]) * 1000;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
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
  if (!input.coalescable) {
    return {
      buffer: false,
      applyImmediately: true,
      flushPendingBeforeApply: !input.paused,
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
