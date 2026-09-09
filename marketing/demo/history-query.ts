import type { AcpSessionQueryInput, AcpSessionVm, AcpUiEventVm } from '@/types';

export const HISTORY_PAGE_SIZE = 50;
export const HISTORY_PAGE_MAX = 100;
export function missing(params: Record<string, unknown> = {}): never {
  throw { code: 'demo.resource-not-found', params };
}
export function pageLimit(value = HISTORY_PAGE_SIZE) {
  if (!Number.isSafeInteger(value) || value < 1) throw { code: 'demo.invalid-query', params: {} };
  return Math.min(value, HISTORY_PAGE_MAX);
}
export function sessionIdentity(session: AcpSessionVm, query: { branchId?: string; sessionId?: string }) {
  if ((query.branchId && query.branchId !== session.branchId) || (query.sessionId && query.sessionId !== session.sessionId)) missing(query);
}
export function cursorSequence(value?: string | null) {
  if (value == null) return undefined;
  if (!/^rev:\d+$/.test(value) || !Number.isSafeInteger(Number(value.slice(4)))) throw { code: 'demo.invalid-cursor', params: {} };
  return Number(value.slice(4));
}
export function pageSession(session: AcpSessionVm, query: AcpSessionQueryInput = {}) {
  sessionIdentity(session, query);
  const events = session.events;
  if (query.beforeCursor && query.afterCursor) throw { code: 'demo.invalid-query', params: {} };
  const before = cursorSequence(query.beforeCursor) ?? query.beforeSeq;
  const after = cursorSequence(query.afterCursor) ?? query.afterSeq;
  const forward = query.afterCursor != null || query.afterSeq != null || query.afterRevision != null;
  const matches = events.filter((event) =>
    (before == null || (event.startedSeq ?? event.seq) < before) &&
    (after == null || (event.endedSeq ?? event.seq) > after) &&
    (query.afterRevision == null || (event.timing?.revision ?? event.seq) > query.afterRevision));
  const limit = pageLimit(query.pageSize ?? query.eventLimit);
  let selected = forward ? matches.slice(0, limit) : matches.slice(-limit);
  // A revision is atomic even when it exceeds the requested logical-item count.
  if (selected.length) {
    const boundary = forward ? selected.at(-1)! : selected[0];
    const revision = boundary.timing?.revision ?? boundary.seq;
    const identities = new Set(selected.map((event) => event.id));
    selected = matches.filter((event) => identities.has(event.id) || (event.timing?.revision ?? event.seq) === revision);
  }
  const first = selected[0];
  const last = selected.at(-1);
  return { ...session, events: selected, eventPage: { ...session.eventPage,
    loadedCount: selected.length, total: events.length,
    oldestSeq: first?.startedSeq ?? first?.seq ?? null, newestSeq: last?.endedSeq ?? last?.seq ?? null,
    oldestCursor: first ? `rev:${first.startedSeq ?? first.seq}` : null, newestCursor: last ? `rev:${last.endedSeq ?? last.seq}` : null,
    hasOlder: first ? events.indexOf(first) > 0 : false,
    hasNewer: last ? events.indexOf(last) < events.length - 1 : false,
  } };
}
