import type { ELK } from 'elkjs/lib/elk-api';

let engine: Promise<ELK> | null = null;

/**
 * Lazily starts one ELK Web Worker shared by every workflow graph. ELK and its
 * worker bundle are loaded on first use so they stay out of the initial chunk.
 */
export function workflowLayoutEngine(): Promise<ELK> {
  engine ??= Promise.all([
    import('elkjs/lib/elk-api.js'),
    import('elkjs/lib/elk-worker.min.js?worker'),
  ]).then(([{ default: ElkApi }, { default: ElkWorker }]) => new ElkApi({ workerFactory: () => new ElkWorker() }));
  engine.catch(() => {
    engine = null;
  });
  return engine;
}
