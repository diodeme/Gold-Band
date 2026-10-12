import { useEffect, useMemo, useRef, useState } from 'react';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import { layoutWorkflowGraph, workflowLayoutKey, type WorkflowLayout, type WorkflowLayoutSpec } from '@/components/workflowGraph';
import { workflowLayoutEngine } from '@/components/workflowLayoutEngine';

const WORKFLOW_LAYOUT_CACHE_LIMIT = 32;
// Reopening a graph remounts its canvas; reuse the finished layout instead of a blank frame.
const layoutCache = new BoundedLruCache<string, WorkflowLayout>(WORKFLOW_LAYOUT_CACHE_LIMIT);

export type WorkflowLayoutState = {
  key: string;
  /** Latest finished layout; may belong to an older key while `pending`. */
  layout: WorkflowLayout | null;
  pending: boolean;
  failed: boolean;
};

/**
 * Lays out a workflow graph off the main thread. Results are latest-wins: a
 * response for a superseded spec never replaces the layout of a newer one.
 */
export function useWorkflowLayout(spec: WorkflowLayoutSpec): WorkflowLayoutState {
  const key = useMemo(() => workflowLayoutKey(spec), [spec]);
  const specRef = useRef(spec);
  specRef.current = spec;
  const [resolved, setResolved] = useState<{ key: string; layout: WorkflowLayout } | null>(() => {
    const cached = layoutCache.get(key);
    return cached ? { key, layout: cached } : null;
  });
  const [failedKey, setFailedKey] = useState<string | null>(null);

  useEffect(() => {
    const cached = layoutCache.get(key);
    if (cached) {
      setResolved((current) => (current?.key === key ? current : { key, layout: cached }));
      return undefined;
    }
    let cancelled = false;
    const requested = specRef.current;
    workflowLayoutEngine()
      .then((elk) => layoutWorkflowGraph(requested, elk))
      .then((layout) => {
        layoutCache.set(key, layout);
        if (!cancelled) setResolved({ key, layout });
      }, (error: unknown) => {
        if (cancelled) return;
        console.error('workflow-graph.layout-failed', error);
        setFailedKey(key);
      });
    return () => {
      cancelled = true;
    };
  }, [key]);

  const failed = failedKey === key;
  return { key, layout: resolved?.layout ?? null, pending: resolved?.key !== key && !failed, failed };
}
