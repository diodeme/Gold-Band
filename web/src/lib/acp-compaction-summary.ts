import type { AcpUiEventVm } from '@/types';

export function compactionSummaryEventId(event: Pick<AcpUiEventVm, 'id' | 'raw'>): string {
  const original = (event.raw as { goldBandScope?: { originalId?: unknown } } | null)?.goldBandScope?.originalId;
  return typeof original === 'string' && original.length > 0 ? original : event.id;
}

export function hasCompactionSummary(raw: unknown): boolean {
  if (!raw || typeof raw !== 'object') return false;
  const value = raw as Record<string, unknown>;
  if (typeof value.compactionSummaryAvailable === 'boolean') return value.compactionSummaryAvailable;
  return Array.isArray(value.summary) && value.summary.some((block) => (
    block?.type === 'text' && typeof block.text === 'string' && block.text.trim().length > 0
  ));
}
