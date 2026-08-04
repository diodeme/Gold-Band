import type { AcpSessionVm, AcpUiEventVm } from "@/types";
import {
  attemptIdFromAcpEvent,
  originalSeqFromAcpEvent,
} from "@/lib/acp-event-normalization";
import { isAcpTextStreamEventKind, mergeAcpLiveStreamEvent } from "@/lib/acp-live-flush";

type RawObject = Record<string, unknown>;

export function mergeRawObject(previous: unknown, next: unknown) {
  const previousObject = rawObject(previous);
  const nextObject = rawObject(next);
  if (!previousObject || !nextObject) return next ?? previous;
  const previousMeta = rawObject(previousObject._meta);
  const nextMeta = rawObject(nextObject._meta);
  const merged: RawObject = { ...previousObject, ...nextObject };
  if (previousMeta || nextMeta) {
    merged._meta = { ...previousMeta, ...nextMeta };
    for (const key of new Set([
      ...Object.keys(previousMeta ?? {}),
      ...Object.keys(nextMeta ?? {}),
    ])) {
      const previousNested = rawObject(previousMeta?.[key]);
      const nextNested = rawObject(nextMeta?.[key]);
      if (previousNested || nextNested) {
        (merged._meta as RawObject)[key] = {
          ...previousNested,
          ...nextNested,
        };
      }
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
    return changed ? orderProviderHistoryByPromptAnchors(merged) : previous;
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
  return orderProviderHistoryByPromptAnchors(
    [...byKey.values()].sort((left, right) => left.seq - right.seq),
  );
}

type ProviderHistoryPlacement = {
  afterPromptId: string | null;
  beforePromptId: string | null;
  gapTurnIndex: number;
  provider: string;
};

type ProviderHistoryGroup = {
  slot: number;
  afterAnchorIndex: number | null;
  gapTurnIndex: number;
  auditSeq: number;
  stableKey: string;
  items: AcpUiEventVm[];
};

export function orderProviderHistoryByPromptAnchors(events: AcpUiEventVm[]) {
  const base: AcpUiEventVm[] = [];
  const grouped = new Map<
    string,
    { placement: ProviderHistoryPlacement; sample: AcpUiEventVm; items: AcpUiEventVm[] }
  >();
  for (const event of events) {
    const placement = providerHistoryPlacement(event);
    if (!placement) {
      base.push(event);
      continue;
    }
    const stableKey = JSON.stringify([
      attemptIdFromAcpEvent(event) ?? "",
      event.sessionId ?? "",
      placement.provider,
      placement.afterPromptId,
      placement.beforePromptId,
      placement.gapTurnIndex,
    ]);
    const existing = grouped.get(stableKey);
    if (existing) existing.items.push(event);
    else grouped.set(stableKey, { placement, sample: event, items: [event] });
  }
  if (grouped.size === 0) return events;

  const promptIndexes = new Map<string, number>();
  base.forEach((event, index) => {
    if (!isGoldBandPrompt(event)) return;
    promptIndexes.set(promptAnchorLookupKey(event, promptAnchorId(event)), index);
  });

  const groups: ProviderHistoryGroup[] = [];
  for (const [stableKey, group] of grouped) {
    group.items.sort(
      (left, right) =>
        providerHistoryItemIndex(left) - providerHistoryItemIndex(right) ||
        originalSeqFromAcpEvent(left) - originalSeqFromAcpEvent(right) ||
        left.seq - right.seq,
    );
    const auditSeq = Math.min(...group.items.map(originalSeqFromAcpEvent));
    const beforeAnchorIndex = group.placement.beforePromptId
      ? promptIndexes.get(
          promptAnchorLookupKey(group.sample, group.placement.beforePromptId),
        ) ?? null
      : null;
    const afterAnchorIndex = group.placement.afterPromptId
      ? promptIndexes.get(
          promptAnchorLookupKey(group.sample, group.placement.afterPromptId),
        ) ?? null
      : null;
    let slot = beforeAnchorIndex;
    if (slot === null && afterAnchorIndex !== null) {
      slot = base.findIndex(
        (event, index) => index > afterAnchorIndex && isGoldBandPrompt(event),
      );
      if (slot < 0) slot = base.length;
    }
    if (slot === null) {
      slot = base.filter((event) => originalSeqFromAcpEvent(event) <= auditSeq).length;
    }
    groups.push({
      slot,
      afterAnchorIndex,
      gapTurnIndex: group.placement.gapTurnIndex,
      auditSeq,
      stableKey,
      items: group.items,
    });
  }
  groups.sort(
    (left, right) =>
      left.slot - right.slot ||
      (left.afterAnchorIndex ?? -1) - (right.afterAnchorIndex ?? -1) ||
      left.gapTurnIndex - right.gapTurnIndex ||
      left.auditSeq - right.auditSeq ||
      left.stableKey.localeCompare(right.stableKey),
  );

  const slots = Array.from({ length: base.length + 1 }, () => [] as AcpUiEventVm[]);
  for (const group of groups) slots[group.slot]!.push(...group.items);
  const ordered: AcpUiEventVm[] = [];
  base.forEach((event, index) => {
    ordered.push(...slots[index]!, event);
  });
  ordered.push(...slots[base.length]!);
  return ordered;
}

function providerHistoryPlacement(event: AcpUiEventVm): ProviderHistoryPlacement | null {
  const raw = rawObject(event.raw);
  if (raw?.source !== "providerHistory") return null;
  const placement = rawObject(raw.historyPlacement);
  if (placement?.version !== 1) return null;
  return {
    afterPromptId: stringValue(placement.afterPromptId),
    beforePromptId: stringValue(placement.beforePromptId),
    gapTurnIndex: numberValue(placement.gapTurnIndex) ?? 0,
    provider: stringValue(raw.historyProvider) ?? "",
  };
}

function isGoldBandPrompt(event: AcpUiEventVm) {
  return event.kind === "userTextDelta" && rawObject(event.raw)?.source === "goldBandPrompt";
}

function promptAnchorId(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const scope = rawObject(raw?.goldBandScope);
  return stringValue(raw?.promptId) ?? stringValue(scope?.originalId) ?? event.id;
}

function promptAnchorLookupKey(event: AcpUiEventVm, promptId: string) {
  return JSON.stringify([
    attemptIdFromAcpEvent(event) ?? "",
    event.sessionId ?? "",
    promptId,
  ]);
}

function providerHistoryItemIndex(event: AcpUiEventVm) {
  return numberValue(rawObject(event.raw)?.historyItemIndex) ?? 0;
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

function numberValue(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}
