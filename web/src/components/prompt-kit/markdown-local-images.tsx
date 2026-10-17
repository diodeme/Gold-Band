import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { WorkspaceFilesVm, WorkspaceRootRef } from '@/types';
import { FALLBACK_WORKSPACE_FILES } from '@/components/workspace/workspace-layout';
import { MarkdownImageLeases, type LocalImageLease, type LocalImageState } from '@/lib/markdown-image-leases';
import { normalizeWorkspacePath } from '@/lib/workspace-root';

const ConfigContext = createContext(FALLBACK_WORKSPACE_FILES);
const ImageScopeContext = createContext<MarkdownImageLeases | null>(null);

export function MarkdownLocalImageConfigProvider({ config, children }: { config: WorkspaceFilesVm; children: ReactNode }) {
  return <ConfigContext.Provider value={config}>{children}</ConfigContext.Provider>;
}

export function MarkdownLocalImageProvider({ scopeKey, root, children }: {
  scopeKey: string;
  root: WorkspaceRootRef | null;
  children: ReactNode;
}) {
  const config = useContext(ConfigContext);
  const projectId = root?.projectId;
  const workspacePath = normalizeWorkspacePath(root?.workspacePath);
  const owner = useMemo(() => projectId
    ? new MarkdownImageLeases({ projectId, workspacePath: root?.workspacePath ?? null }, config)
    : null,
  // Only semantic scope/root and low-frequency limits own this lease lifetime.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  [scopeKey, projectId, workspacePath, config.markdownEmbeddedImageLimit, config.markdownEmbeddedImageMaxConcurrent, config.imagePreviewMaxPixels]);
  useEffect(() => () => owner?.clear(), [owner]);
  return <ImageScopeContext.Provider value={owner}>{children}</ImageScopeContext.Provider>;
}

export function useLocalMarkdownImage(source: string | null) {
  const owner = useContext(ImageScopeContext);
  const hostRef = useRef<HTMLSpanElement>(null);
  const leaseRef = useRef<LocalImageLease | null>(null);
  const [visible, setVisible] = useState(false);
  const [retry, setRetry] = useState(0);
  const [pageVisible, setPageVisible] = useState(() => typeof document !== 'undefined' && !document.hidden);
  const [result, setResult] = useState<{ owner: MarkdownImageLeases; source: string; state: LocalImageState } | null>(null);
  useEffect(() => {
    const host = hostRef.current;
    if (!host || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting), { rootMargin: '128px' });
    observer.observe(host);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const update = () => setPageVisible(!document.hidden);
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);
  useEffect(() => {
    if (!owner || !source || !visible || !pageVisible) return;
    let cancelled = false;
    const lease = owner.acquire(source, (state) => {
      if (!cancelled) setResult({ owner, source, state });
    });
    leaseRef.current = lease;
    return () => {
      cancelled = true;
      if (leaseRef.current === lease) leaseRef.current = null;
      lease.release();
    };
  }, [owner, source, visible, pageVisible, retry]);
  const current = result && result.owner === owner && result.source === source ? result.state : null;
  return {
    hostRef,
    state: !owner || !source ? { kind: 'error' as const, code: 'workspace-file.workspace-unavailable' }
      : visible && pageVisible && leaseRef.current ? current : null,
    // Keep geometry, not a live img/token, while it is outside the loading window.
    dimensions: current?.kind === 'ready' ? { width: current.width, height: current.height } : null,
    canRetry: Boolean(owner && source),
    refresh: () => current?.kind === 'error' ? setRetry(value => value + 1) : leaseRef.current?.refresh(),
  };
}
