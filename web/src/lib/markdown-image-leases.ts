import { releaseWorkspaceFilePreview, resolveMarkdownImage } from '@/api';
import type { MarkdownImagePreviewVm, WorkspaceFilesVm, WorkspaceRootRef } from '@/types';

export type LocalImageState =
  | { kind: 'loading' }
  | Extract<MarkdownImagePreviewVm, { kind: 'ready' }>
  | { kind: 'error'; code: string };

type Limits = Pick<WorkspaceFilesVm, 'markdownEmbeddedImageLimit' | 'markdownEmbeddedImageMaxConcurrent' | 'imagePreviewMaxPixels'>;
type Listener = (state: LocalImageState) => void;
type Entry = {
  source: string;
  state: LocalImageState;
  listeners: Set<Listener>;
  pending: boolean;
  requested: boolean;
  timer: ReturnType<typeof setTimeout> | null;
};

const REFRESH_LEAD_MS = 60_000;
const MIN_REFRESH_DELAY_MS = 1_000;
const LOADING: LocalImageState = { kind: 'loading' };
const LIMIT_ERROR: LocalImageState = { kind: 'error', code: 'workspace-file.too-large' };

export interface LocalImageLease {
  release(): void;
  refresh(): void;
}

/** Transient, visible references in one conversation root; never a document/content cache. */
export class MarkdownImageLeases {
  private readonly entries = new Map<string, Entry>();
  private activeReads = 0;

  constructor(
    private readonly root: WorkspaceRootRef,
    private readonly limits: Limits,
    private readonly io: { resolve: typeof resolveMarkdownImage; release: typeof releaseWorkspaceFilePreview } = {
      resolve: input => resolveMarkdownImage(input),
      release: token => releaseWorkspaceFilePreview(token),
    },
  ) {}

  acquire(source: string, listener: Listener): LocalImageLease {
    let entry = this.entries.get(source);
    if (!entry) {
      if (this.entries.size >= this.limits.markdownEmbeddedImageLimit) {
        listener(LIMIT_ERROR);
        return { release() {}, refresh() {} };
      }
      entry = { source, state: LOADING, listeners: new Set(), pending: false, requested: true, timer: null };
      this.entries.set(source, entry);
    }
    entry.listeners.add(listener);
    listener(entry.state);
    this.pump();
    let released = false;
    const owned = entry;
    return {
      release: () => {
        if (released) return;
        released = true;
        owned.listeners.delete(listener);
        if (owned.listeners.size || this.entries.get(source) !== owned) return;
        this.entries.delete(source);
        this.releaseEntry(owned);
      },
      refresh: () => {
        if (released || this.entries.get(source) !== owned || owned.pending) return;
        if (owned.timer) clearTimeout(owned.timer);
        owned.timer = null;
        owned.requested = true;
        this.pump();
      },
    };
  }

  clear() {
    for (const entry of this.entries.values()) this.releaseEntry(entry);
    this.entries.clear();
  }

  private releaseToken(token: string) {
    void this.io.release(token).catch(() => undefined);
  }

  private releaseEntry(entry: Entry) {
    if (entry.timer) clearTimeout(entry.timer);
    entry.timer = null;
    if (entry.state.kind === 'ready') this.releaseToken(entry.state.previewGrant.token);
    entry.listeners.clear();
  }

  private publish(entry: Entry, state: LocalImageState) {
    entry.state = state;
    for (const listener of entry.listeners) listener(state);
  }

  private pump() {
    for (const entry of this.entries.values()) {
      if (this.activeReads >= Math.max(1, this.limits.markdownEmbeddedImageMaxConcurrent)) break;
      if (!entry.requested || entry.pending) continue;
      entry.requested = false;
      entry.pending = true;
      this.activeReads += 1;
      void this.read(entry);
    }
  }

  private async read(entry: Entry) {
    let next: LocalImageState;
    try {
      const result = await this.io.resolve({ ...this.root, source: { kind: 'workspace' }, rawSrc: entry.source });
      next = result.kind === 'ready' ? result : {
        kind: 'error',
        code: result.kind === 'unsupported' ? result.limitationCode : 'workspace-file.path-outside-workspace',
      };
    } catch (reason) {
      const error = reason as { code?: string } | null;
      next = { kind: 'error', code: error?.code ?? 'workspace-file.read-failed' };
    }
    this.activeReads -= 1;
    entry.pending = false;
    if (this.entries.get(entry.source) !== entry) {
      if (next.kind === 'ready') this.releaseToken(next.previewGrant.token);
      this.pump();
      return;
    }

    // Grants contain no image bytes. Admit decoded pixels before assigning an img src,
    // so many individually valid images cannot exhaust the WebView's decoded memory.
    if (next.kind === 'ready') {
      let pixels = next.width * next.height;
      for (const other of this.entries.values()) {
        if (other !== entry && other.state.kind === 'ready') pixels += other.state.width * other.state.height;
      }
      if (pixels > this.limits.imagePreviewMaxPixels) {
        this.releaseToken(next.previewGrant.token);
        next = LIMIT_ERROR;
      }
    }
    const previous = entry.state;
    this.publish(entry, next);
    if (previous.kind === 'ready' && (next.kind !== 'ready' || next.previewGrant.token !== previous.previewGrant.token)) {
      this.releaseToken(previous.previewGrant.token);
    }
    if (next.kind === 'ready') {
      const delay = Math.max(MIN_REFRESH_DELAY_MS, Number(next.previewGrant.expiresAtMs) - Date.now() - REFRESH_LEAD_MS);
      entry.timer = setTimeout(() => {
        entry.timer = null;
        if (this.entries.get(entry.source) !== entry) return;
        entry.requested = true;
        this.pump();
      }, delay);
    }
    this.pump();
  }
}
