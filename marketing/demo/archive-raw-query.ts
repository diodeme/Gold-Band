import type { AcpRawFramePageVm, AcpRawFrameQueryInput, AcpRawFrameVm } from '@/types';
import { boundedBytes } from './archive-http';

export interface ArchiveRawIndex {
  resource: string;
  bytes: number;
  count: number;
  pages: { offset: number; length: number; count: number; firstLine: number }[];
}
export const ARCHIVE_RAW_MAX_PAGE_SIZE = 200;
export function normalizeRawQuery(query: AcpRawFrameQueryInput = {}) {
  const { page = 0, pageSize = 100, order = 'desc' } = query;
  if (!Number.isSafeInteger(page) || page < 0 || !Number.isInteger(pageSize) || pageSize < 1 || pageSize > ARCHIVE_RAW_MAX_PAGE_SIZE
    || !['asc', 'desc'].includes(order)) throw { code: 'demo.archive-cursor-invalid', params: {} };
  const filter = (value?: string) => value?.trim().toLowerCase() || null;
  return { page, pageSize, order, search: filter(query.search), kind: filter(query.kind), direction: filter(query.direction) };
}
export function rawFrame(line: string, lineNumber: number): AcpRawFrameVm {
  const value = JSON.parse(line);
  const frame = value.frame ?? {};
  return { id: `raw-${lineNumber}`, lineNumber, timestamp: value.timestamp ?? null, direction: value.direction ?? null,
    kind: frame.params?.update?.sessionUpdate ?? frame.method ?? ('error' in frame ? 'error' : 'result' in frame ? 'result' : 'frame'),
    content: line, contentTruncated: false };
}
export async function queryArchiveRawFrames(url: string, index: ArchiveRawIndex, input: AcpRawFrameQueryInput,
  fetcher: typeof fetch = fetch, signal?: AbortSignal): Promise<AcpRawFramePageVm> {
  const query = normalizeRawQuery(input);
  const { page, pageSize, order, search, kind, direction } = query;
  const items: AcpRawFrameVm[] = [];
  let total = 0;
  const start = page * pageSize;
  // Scan in display order so only the requested page survives each bounded chunk.
  const pages = order === 'desc' ? [...index.pages].reverse() : index.pages;
  for (const part of pages) {
    signal?.throwIfAborted();
    if (part.offset + part.length > index.bytes) throw { code: 'demo.archive-range-invalid', params: {} };
    const range = `${part.offset}-${part.offset + part.length - 1}`;
    const response = await fetcher(url, { headers: { Range: `bytes=${range}` }, signal });
    if (response.status !== 206 || response.headers.get('Content-Range') !== `bytes ${range}/${index.bytes}`) {
      await response.body?.cancel();
      throw { code: 'demo.archive-range-unsupported', params: {} };
    }
    const bytes = await boundedBytes(response, part.length);
    signal?.throwIfAborted();
    const lines = new TextDecoder('utf-8', { fatal: true }).decode(bytes).trimEnd().split('\n');
    if (lines.length !== part.count) throw { code: 'demo.archive-range-invalid', params: {} };
    for (let i = 0; i < lines.length; i++) {
      const position = order === 'desc' ? lines.length - i - 1 : i;
      const line = lines[position];
      if (search && !line.toLowerCase().includes(search)) continue;
      const item = rawFrame(line, part.firstLine + position);
      if (kind && !item.kind.toLowerCase().includes(kind)) continue;
      if (direction && item.direction?.toLowerCase() !== direction) continue;
      if (total >= start && items.length < pageSize) items.push(item);
      total++;
    }
  }
  return { ...query, items, total, hasPrevious: page > 0 && total > 0, hasNext: start + pageSize < total };
}
