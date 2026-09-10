import type { AcpSessionVm, AcpUiEventVm } from '@/types';
import type { ArchiveReader, ArchiveSession } from './archive';

const text = (value: unknown) => typeof value === 'string' ? value : null;
const object = (value: unknown): Record<string, unknown> | null => value !== null && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;

export function archiveTimestamp(value: unknown): string | null {
  if (typeof value !== 'string') return null;
  const epoch = /^(\d+)Z$/.exec(value);
  const milliseconds = epoch ? Number(epoch[1]) * 1000 : Date.parse(value);
  return Number.isFinite(milliseconds) ? new Date(milliseconds).toISOString() : null;
}
export function archiveEvent(event: AcpUiEventVm): AcpUiEventVm {
  return { ...event, timestamp: archiveTimestamp(event.timestamp) ?? event.timestamp,
    startedAt: archiveTimestamp(event.startedAt), endedAt: archiveTimestamp(event.endedAt) };
}

export function archiveSessionVm(session: ArchiveSession, branchId: string, page: Awaited<ReturnType<ArchiveReader['history']>>): AcpSessionVm {
  const diagnostics = page.diagnostics;
  if (!diagnostics) throw Object.assign(new Error('demo.archive-diagnostics-unavailable'), { code: 'demo.archive-diagnostics-unavailable', params: { branchId } });
  const root = branchId === 'root';
  const snapshot = root ? page.snapshot ?? {} : {};
  const models = object(snapshot.models);
  const modes = object(snapshot.modes);
  return {
    branchId, readOnly: true, sessionId: text(page.snapshot?.sessionId), title: root ? session.node.title : branchId,
    roundId: session.roundId, nodeId: session.nodeId, attemptId: session.attemptId,
    outerNodeId: session.outerNodeId, outerAttemptId: session.outerAttemptId,
    provider: text(page.workerRef.provider) ?? text(session.node.provider) ?? 'unknown', adapterId: text(snapshot.adapterId), adapterDisplayName: text(snapshot.adapterDisplayName),
    cwd: text(snapshot.cwd), status: root ? text(snapshot.latestTurnStatus) ?? 'unknown' : 'unknown',
    sessionStartedAt: root ? archiveTimestamp(snapshot.createdAt ?? session.node.startedAt) : null,
    sessionUpdatedAt: root ? archiveTimestamp(snapshot.updatedAt ?? session.node.finishedAt) : null,
    restored: snapshot.restored === true, stopReason: root ? text(snapshot.stopReason) : null,
    systemPromptAppend: root ? text(snapshot.systemPromptAppend) : null,
    config: root ? { models: snapshot.models, modes: snapshot.modes, configOptions: snapshot.configOptions,
      currentModelId: text(models?.currentModelId), currentModeId: text(modes?.currentModeId) } : null,
    events: page.events.map(archiveEvent), eventPage: page.eventPage,
    timelineProjection: null,
    pendingInteractions: [],
    diagnostics: { ...diagnostics, eventCount: page.eventPage.total },
  };
}
