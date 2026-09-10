import type { AcpRawFramePageVm, AcpRawFrameQueryInput } from '@/types';
import type { ArchiveRawIndex } from './archive-raw-query';

export function runArchiveRawQuery(url: string, index: ArchiveRawIndex, query: AcpRawFrameQueryInput,
  signal?: AbortSignal): Promise<AcpRawFramePageVm> {
  signal?.throwIfAborted();
  const worker = new Worker(new URL('./archive-raw.worker.ts', import.meta.url), { type: 'module' });
  return new Promise((resolve, reject) => {
    const cleanup = () => { signal?.removeEventListener('abort', abort); worker.terminate(); };
    const abort = () => { cleanup(); reject(signal?.reason ?? new DOMException('Aborted', 'AbortError')); };
    signal?.addEventListener('abort', abort, { once: true });
    worker.onmessage = ({ data }) => { cleanup(); if (data.error) reject(data.error); else resolve(data.result); };
    worker.onerror = () => { cleanup(); reject({ code: 'demo.resource-not-found', params: {} }); };
    worker.postMessage({ url, index, query });
  });
}
