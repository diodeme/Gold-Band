import type { AcpSessionVm, AcpUiEventVm } from '@/types';

const scopeSeparator = ':';

type RawObject = Record<string, unknown>;

export function scopeAcpId(attemptId: string, id: string) {
  return id.startsWith(`${attemptId}${scopeSeparator}`) ? id : `${attemptId}${scopeSeparator}${id}`;
}

export function normalizeAcpSessionForAttempt(session: AcpSessionVm | null, attemptId: string): AcpSessionVm | null {
  if (!session) return null;
  return { ...session, events: session.events.map((event) => normalizeAcpEventForAttempt(event, attemptId)) };
}

export function normalizeAcpEventForAttempt(event: AcpUiEventVm, attemptId: string, displaySeq = event.seq): AcpUiEventVm {
  const raw = rawObject(event.raw);
  const scope = rawObject(raw?.goldBandScope);
  const originalId = stringValue(scope?.originalId) ?? unscopedAcpId(event.id, attemptId);
  const originalToolCallId = stringValue(scope?.originalToolCallId) ?? (event.toolCallId ? unscopedAcpId(event.toolCallId, attemptId) : null);
  const parentId = parentToolUseIdFromRaw(raw);
  const originalParentToolUseId = stringValue(scope?.originalParentToolUseId) ?? (parentId ? unscopedAcpId(parentId, attemptId) : null);
  const originalSeq = numberValue(scope?.originalSeq) ?? event.seq;

  return {
    ...event,
    id: scopeAcpId(attemptId, originalId),
    seq: displaySeq,
    toolCallId: originalToolCallId ? scopeAcpId(attemptId, originalToolCallId) : event.toolCallId,
    raw: normalizeAcpRaw(event.raw, attemptId, {
      ...scope,
      attemptId,
      originalId,
      originalToolCallId,
      originalParentToolUseId,
      originalSeq,
    }, originalParentToolUseId),
  };
}

export function attemptIdFromAcpEvent(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const scope = rawObject(raw?.goldBandScope);
  const scopedAttemptId = stringValue(scope?.attemptId);
  if (scopedAttemptId) return scopedAttemptId;
  const rawAttemptId = stringValue(raw?.attemptId);
  if (rawAttemptId) return rawAttemptId;
  const separatorIndex = event.id.indexOf(scopeSeparator);
  return separatorIndex > 0 ? event.id.slice(0, separatorIndex) : null;
}

export function originalSeqFromAcpEvent(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const scope = rawObject(raw?.goldBandScope);
  return numberValue(scope?.originalSeq) ?? event.seq;
}

export function isAcpAttemptSeparator(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const scope = rawObject(raw?.goldBandScope);
  return scope?.separator === true;
}

function normalizeAcpRaw(value: unknown, attemptId: string, scope: RawObject, originalParentToolUseId?: string | null) {
  const rawValue = rawObject(value);
  const raw: RawObject = rawValue ? { ...rawValue } : { value };
  raw.attemptId = attemptId;
  raw.goldBandScope = scope;
  if (originalParentToolUseId) raw._meta = normalizeMeta(raw._meta, attemptId, originalParentToolUseId);
  return raw;
}

function normalizeMeta(value: unknown, attemptId: string, originalParentToolUseId: string) {
  const metaValue = rawObject(value);
  const meta: RawObject = metaValue ? { ...metaValue } : {};
  const claudeCodeValue = rawObject(meta.claudeCode);
  const claudeCode: RawObject = claudeCodeValue ? { ...claudeCodeValue } : {};
  claudeCode.parentToolUseId = scopeAcpId(attemptId, originalParentToolUseId);
  meta.claudeCode = claudeCode;
  return meta;
}

function parentToolUseIdFromRaw(raw: RawObject | null) {
  const meta = rawObject(raw?._meta);
  const claudeCode = rawObject(meta?.claudeCode);
  return stringValue(claudeCode?.parentToolUseId);
}

function unscopedAcpId(id: string, attemptId: string) {
  const prefix = `${attemptId}${scopeSeparator}`;
  return id.startsWith(prefix) ? id.slice(prefix.length) : id;
}

function rawObject(value: unknown): RawObject | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as RawObject : null;
}

function stringValue(value: unknown) {
  return typeof value === 'string' && value.length > 0 ? value : null;
}

function numberValue(value: unknown) {
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}
