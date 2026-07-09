import type { AcpSessionVm, AcpUiEventVm } from "@/types";
import { attemptIdFromAcpEvent } from "@/lib/acp-event-normalization";
import { isAcpTextStreamEventKind, mergeAcpLiveStreamEvent } from "@/lib/acp-live-flush";

type RawObject = Record<string, unknown>;

export function mergeRawObject(previous: unknown, next: unknown) {
  const previousObject = rawObject(previous);
  const nextObject = rawObject(next);
  if (!previousObject || !nextObject) return next ?? previous;
  const previousMeta = rawObject(previousObject._meta);
  const nextMeta = rawObject(nextObject._meta);
  const previousClaudeCode = rawObject(previousMeta?.claudeCode);
  const nextClaudeCode = rawObject(nextMeta?.claudeCode);
  const merged: RawObject = { ...previousObject, ...nextObject };
  if (previousMeta || nextMeta) {
    merged._meta = { ...previousMeta, ...nextMeta };
    if (previousClaudeCode || nextClaudeCode) {
      (merged._meta as RawObject).claudeCode = {
        ...previousClaudeCode,
        ...nextClaudeCode,
      };
    }
  }
  return merged;
}

export function mergeAcpEventSnapshots(
  existing: AcpUiEventVm,
  incoming: AcpUiEventVm,
): AcpUiEventVm {
  if (
    isAcpTextStreamEventKind(existing.kind) &&
    isAcpTextStreamEventKind(incoming.kind) &&
    existing.kind === incoming.kind &&
    existing.id === incoming.id
  ) {
    const merged = mergeAcpLiveStreamEvent(existing, incoming, mergeRawObject);
    return { ...merged, seq: existing.seq };
  }
  return { ...incoming, seq: existing.seq };
}

export function mergeAcpEventWindows(
  previous: AcpUiEventVm[],
  next: AcpUiEventVm[],
  alignDisplaySeq: (event: AcpUiEventVm, previous: AcpUiEventVm[]) => number = (
    event,
  ) => event.seq,
) {
  if (next.length === 0) return previous;
  const replacementByKey = new Map<string, AcpUiEventVm>();
  for (const event of next) {
    const key = acpEventKey(event);
    const existing = replacementByKey.get(key);
    replacementByKey.set(
      key,
      existing ? mergeAcpEventSnapshots(existing, event) : event,
    );
  }
  let allUpdatesReplaceExistingEvents = replacementByKey.size > 0;
  for (const key of replacementByKey.keys()) {
    if (!previous.some((event) => acpEventKey(event) === key)) {
      allUpdatesReplaceExistingEvents = false;
      break;
    }
  }
  if (allUpdatesReplaceExistingEvents) {
    let changed = false;
    const merged = previous.map((event) => {
      const replacement = replacementByKey.get(acpEventKey(event));
      if (!replacement) return event;
      changed = true;
      return mergeAcpEventSnapshots(event, replacement);
    });
    return changed ? merged : previous;
  }

  const previousByKey = new Map<string, AcpUiEventVm>();
  const byKey = new Map<string, AcpUiEventVm>();
  for (const event of previous) {
    const key = acpEventKey(event);
    previousByKey.set(key, event);
    byKey.set(key, event);
  }
  for (const event of replacementByKey.values()) {
    const key = acpEventKey(event);
    const existing = previousByKey.get(key);
    byKey.set(
      key,
      existing
        ? mergeAcpEventSnapshots(existing, event)
        : { ...event, seq: alignDisplaySeq(event, previous) },
    );
  }
  return [...byKey.values()].sort((left, right) => left.seq - right.seq);
}

export function acpEventKey(event: AcpUiEventVm) {
  if (event.kind === "permissionRequest")
    return `permission:${permissionRequestIdFromEvent(event)}`;
  const attemptId = attemptIdFromAcpEvent(event) ?? event.sessionId ?? "";
  return `${attemptId}:${event.kind}:${event.id}`;
}

export function acpSessionEventsSignature(
  session: Pick<AcpSessionVm, "events" | "eventPage"> | null | undefined,
) {
  if (!session) return "null";
  return JSON.stringify({
    length: session.events.length,
    hasOlder: session.eventPage.hasOlder,
    hasNewer: session.eventPage.hasNewer,
    events: session.events.map((event) => ({
      key: acpEventKey(event),
      status: event.status ?? null,
      seq: event.seq,
      startedSeq: event.startedSeq ?? null,
      endedSeq: event.endedSeq ?? null,
      timestamp: event.timestamp,
      endedAt: event.endedAt ?? null,
      contentLength: event.content?.length ?? 0,
    })),
  });
}

export function permissionRequestIdFromEvent(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const requestId = stringValue(raw?.requestId);
  if (requestId) return canonicalPermissionRequestId(requestId);
  const id = event.id;
  const prefixes = ["permission-permission-", "permission-", "request-"];
  for (const prefix of prefixes) {
    if (id.startsWith(prefix)) return canonicalPermissionRequestId(id.slice(prefix.length));
  }
  return canonicalPermissionRequestId(id);
}

function canonicalPermissionRequestId(value: string) {
  return value.replace(/^(permission-)+/, "");
}

function rawObject(value: unknown): RawObject | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as RawObject)
    : null;
}

function stringValue(value: unknown) {
  return typeof value === "string" && value.length > 0 ? value : null;
}
