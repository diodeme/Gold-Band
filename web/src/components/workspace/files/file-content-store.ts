import { useSyncExternalStore } from 'react';
import {
  readFileResource,
  resolveMarkdownImage,
  releaseExternalFileAccess,
  releaseWorkspaceFilePreview,
  renewExternalFileAccess,
  startWorkspaceFileWatch,
  stopWorkspaceFileWatch,
  subscribeWorkspaceFileChanges,
  writeFileResource,
} from '@/api';
import { FALLBACK_WORKSPACE_FILES } from '../workspace-layout';
import type {
  ExternalFileAccessGrantVm,
  FileRevisionVm,
  WorkspaceFileChangedEventVm,
  WorkspaceFileSnapshotVm,
  WorkspaceFilesVm,
  MarkdownImagePreviewVm,
} from '@/types';
import type { FileWorkspaceResource } from '../right-workspace-context';

export type FileSaveState =
  | { kind: 'clean' }
  | { kind: 'scheduled'; localRevision: number }
  | { kind: 'saving'; localRevision: number; operationId: string }
  | { kind: 'error'; localRevision: number; errorCode: string }
  | { kind: 'conflict'; localRevision: number; diskRevision: FileRevisionVm | null };

export type MarkdownEditorMode = 'live-preview' | 'source';

export type MarkdownImageState =
  | { kind: 'loading'; rawSrc: string }
  | ({ rawSrc: string } & MarkdownImagePreviewVm)
  | { kind: 'error'; rawSrc: string; errorCode: string };

export interface FileContentEntry {
  key: string;
  resource: FileWorkspaceResource;
  status: 'idle' | 'loading' | 'ready' | 'error';
  snapshot: WorkspaceFileSnapshotVm | null;
  errorCode: string | null;
  requestRevision: number;
  contentRevision: number;
  localRevision: number;
  savedLocalRevision: number;
  saveState: FileSaveState;
}

const EMPTY_ENTRY: FileContentEntry = {
  key: '',
  resource: null as unknown as FileWorkspaceResource,
  status: 'idle',
  snapshot: null,
  errorCode: null,
  requestRevision: 0,
  contentRevision: 0,
  localRevision: 0,
  savedLocalRevision: 0,
  saveState: { kind: 'clean' },
};

interface SaveRuntime {
  timer: ReturnType<typeof setTimeout> | null;
  promise: Promise<boolean> | null;
  latestContent: string;
  encoding: string;
  lineEnding: string;
  diskRevision: FileRevisionVm;
  externalAccessGrant: ExternalFileAccessGrantVm | null;
  renewalTimer: ReturnType<typeof setTimeout> | null;
  previewRefreshTimer: ReturnType<typeof setTimeout> | null;
  editorStateJson: unknown | null;
  imageViewState: FileImageViewState;
  markdownMode: MarkdownEditorMode;
  markdownImages: Map<string, MarkdownImageState>;
  markdownImageSources: Set<string>;
  markdownImageRequestRevision: number;
  approvedMarkdownImageTargets: Set<string>;
}

export interface FileImageViewState {
  zoom: number;
  scrollLeft: number;
  scrollTop: number;
}

interface PrimedExternalGrant {
  projectId: string;
  canonicalPath: string;
  grant: ExternalFileAccessGrantVm;
}

function errorCode(reason: unknown) {
  if (typeof reason === 'object' && reason && 'code' in reason && typeof reason.code === 'string') return reason.code;
  return 'workspace-file.read-failed';
}

function operationId() {
  return globalThis.crypto?.randomUUID?.() ?? `file-write-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export class FileContentStore {
  private config: WorkspaceFilesVm = FALLBACK_WORKSPACE_FILES;
  private readonly entries = new Map<string, FileContentEntry>();
  private readonly runtimes = new Map<string, SaveRuntime>();
  private readonly primedGrants = new Map<string, PrimedExternalGrant>();
  private readonly listeners = new Set<() => void>();
  private readonly changeListeners = new Set<(event: WorkspaceFileChangedEventVm) => void>();
  private readonly projectWatchRefs = new Map<string, number>();
  private readonly projectWatchOperations = new Map<string, Promise<void>>();
  private eventUnsubscribe: (() => void) | null = null;
  private eventSubscriptionPromise: Promise<void> | null = null;

  configure(config: WorkspaceFilesVm) {
    this.config = config;
  }

  shouldHighlight(characters: number) {
    return characters <= this.config.textHighlightMaxChars;
  }

  canUseMarkdownLivePreview(characters: number) {
    return characters <= this.config.markdownLivePreviewMaxChars;
  }

  markdownMode(resourceKey: string): MarkdownEditorMode {
    return this.runtimes.get(resourceKey)?.markdownMode ?? 'live-preview';
  }

  setMarkdownMode(resourceKey: string, mode: MarkdownEditorMode) {
    const runtime = this.runtimes.get(resourceKey);
    const entry = this.entries.get(resourceKey);
    if (!runtime || !entry || runtime.markdownMode === mode) return;
    if (mode === 'live-preview' && entry.snapshot?.kind === 'text'
      && !this.canUseMarkdownLivePreview(entry.snapshot.content.length)) return;
    runtime.markdownMode = mode;
    this.setEntry(resourceKey, { ...entry });
  }

  markdownImages(resourceKey: string) {
    return new Map(this.runtimes.get(resourceKey)?.markdownImages ?? []);
  }

  async syncMarkdownImages(resourceKey: string, rawSources: string[]) {
    const runtime = this.runtimes.get(resourceKey);
    const entry = this.entries.get(resourceKey);
    if (!runtime || !entry || entry.snapshot?.kind !== 'text') return;
    const sources = new Set(rawSources.slice(0, this.config.markdownEmbeddedImageLimit));
    runtime.markdownImageSources = sources;
    const staleTokens: string[] = [];
    for (const [rawSrc, image] of runtime.markdownImages) {
      if (sources.has(rawSrc)) continue;
      if (image.kind === 'ready') staleTokens.push(image.previewToken);
      runtime.markdownImages.delete(rawSrc);
    }
    for (const source of sources) {
      if (!runtime.markdownImages.has(source)) {
        runtime.markdownImages.set(source, { kind: 'loading', rawSrc: source });
      }
    }
    const requestRevision = ++runtime.markdownImageRequestRevision;
    this.setEntry(resourceKey, { ...entry });
    await Promise.all(staleTokens.map((token) => releaseWorkspaceFilePreview(token).catch(() => undefined)));
    const unresolved = [...sources].filter((source) => runtime.markdownImages.get(source)?.kind === 'loading');
    const concurrency = this.config.markdownEmbeddedImageMaxConcurrent;
    for (let index = 0; index < unresolved.length; index += concurrency) {
      await Promise.all(unresolved.slice(index, index + concurrency).map(async (rawSrc) => {
        let next: MarkdownImageState;
        try {
          const result = await resolveMarkdownImage({
            projectId: entry.resource.projectId,
            markdownCanonicalPath: entry.resource.locator.canonicalPath,
            markdownExternalAccessToken: runtime.externalAccessGrant?.token ?? null,
            rawSrc,
            approvedExternalTargets: [...runtime.approvedMarkdownImageTargets],
          });
          next = { ...result, rawSrc };
        } catch (reason) {
          next = { kind: 'error', rawSrc, errorCode: errorCode(reason) };
        }
        const currentRuntime = this.runtimes.get(resourceKey);
        const currentEntry = this.entries.get(resourceKey);
        if (!currentRuntime || !currentEntry
          || currentRuntime.markdownImageRequestRevision !== requestRevision
          || !currentRuntime.markdownImageSources.has(rawSrc)) {
          if (next.kind === 'ready') {
            await releaseWorkspaceFilePreview(next.previewToken).catch(() => undefined);
          }
          return;
        }
        const previous = currentRuntime.markdownImages.get(rawSrc);
        if (previous?.kind === 'ready' && previous.previewToken !== (next.kind === 'ready' ? next.previewToken : null)) {
          await releaseWorkspaceFilePreview(previous.previewToken).catch(() => undefined);
        }
        currentRuntime.markdownImages.set(rawSrc, next);
        this.setEntry(resourceKey, { ...currentEntry });
      }));
    }
  }

  async approveMarkdownImages(resourceKey: string) {
    const runtime = this.runtimes.get(resourceKey);
    if (!runtime) return;
    for (const image of runtime.markdownImages.values()) {
      if (image.kind === 'approvalRequired') {
        runtime.approvedMarkdownImageTargets.add(image.canonicalPath);
        runtime.markdownImages.set(image.rawSrc, { kind: 'loading', rawSrc: image.rawSrc });
      }
    }
    await this.syncMarkdownImages(resourceKey, [...runtime.markdownImageSources]);
  }

  editorState(resourceKey: string) {
    return this.runtimes.get(resourceKey)?.editorStateJson ?? null;
  }

  persistEditorState(resourceKey: string, state: unknown, contentRevision?: number) {
    if (contentRevision != null && this.entries.get(resourceKey)?.contentRevision !== contentRevision) return;
    const runtime = this.runtimes.get(resourceKey);
    if (runtime) runtime.editorStateJson = state;
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  subscribeChanges(listener: (event: WorkspaceFileChangedEventVm) => void) {
    this.changeListeners.add(listener);
    return () => this.changeListeners.delete(listener);
  }

  snapshot = (key: string) => this.entries.get(key) ?? EMPTY_ENTRY;

  primeExternalGrant(
    resourceKey: string,
    projectId: string,
    canonicalPath: string,
    grant: ExternalFileAccessGrantVm | null,
  ) {
    if (!grant) return;
    const previous = this.primedGrants.get(resourceKey)?.grant;
    if (previous && previous.token !== grant.token) {
      void releaseExternalFileAccess(previous.token).catch(() => undefined);
    }
    this.primedGrants.set(resourceKey, { projectId, canonicalPath, grant });
  }

  async load(resource: FileWorkspaceResource, preferSource = false, force = false) {
    const existing = this.entries.get(resource.key);
    if (!force && existing?.status === 'ready') return existing;
    const requestRevision = (existing?.requestRevision ?? 0) + 1;
    this.setEntry(resource.key, {
      ...(existing ?? { ...EMPTY_ENTRY, key: resource.key, resource }),
      resource,
      status: 'loading',
      errorCode: null,
      requestRevision,
    });
    const grant = existing?.snapshot?.externalAccessGrant
      ?? this.primedGrants.get(resource.key)?.grant
      ?? null;
    try {
      const snapshot = await readFileResource(
        resource.projectId,
        resource.locator.canonicalPath,
        grant?.token,
        preferSource,
      );
      if (this.entries.get(resource.key)?.requestRevision !== requestRevision) return this.entries.get(resource.key) ?? null;
      if (snapshot.externalAccessGrant) {
        this.primedGrants.set(resource.key, {
          projectId: resource.projectId,
          canonicalPath: resource.locator.canonicalPath,
          grant: snapshot.externalAccessGrant,
        });
      }
      if (
        existing?.snapshot?.kind === 'image'
        && (snapshot.kind !== 'image' || snapshot.previewToken !== existing.snapshot.previewToken)
      ) {
        await releaseWorkspaceFilePreview(existing.snapshot.previewToken).catch(() => undefined);
      }
      const next: FileContentEntry = {
        key: resource.key,
        resource,
        status: 'ready',
        snapshot,
        errorCode: null,
        requestRevision,
        contentRevision: (existing?.contentRevision ?? 0) + 1,
        localRevision: 0,
        savedLocalRevision: 0,
        saveState: { kind: 'clean' },
      };
      this.setEntry(resource.key, next);
      this.installRuntime(resource.key, snapshot);
      this.touch(resource.key);
      this.prune();
      return next;
    } catch (reason) {
      if (this.entries.get(resource.key)?.requestRevision === requestRevision) {
        this.setEntry(resource.key, {
          ...(this.entries.get(resource.key) ?? { ...EMPTY_ENTRY, key: resource.key, resource }),
          resource,
          status: 'error',
          errorCode: errorCode(reason),
          requestRevision,
        });
      }
      return this.entries.get(resource.key) ?? null;
    }
  }

  updateText(resourceKey: string, content: string) {
    const entry = this.entries.get(resourceKey);
    const runtime = this.runtimes.get(resourceKey);
    if (!entry || !runtime || entry.snapshot?.kind !== 'text' || !entry.snapshot.editable) return;
    runtime.latestContent = content;
    if (runtime.markdownMode === 'live-preview' && !this.canUseMarkdownLivePreview(content.length)) {
      runtime.markdownMode = 'source';
    }
    const localRevision = entry.localRevision + 1;
    if (runtime.timer) clearTimeout(runtime.timer);
    const blocked = entry.saveState.kind === 'error' || entry.saveState.kind === 'conflict';
    runtime.timer = blocked
      ? null
      : setTimeout(() => void this.flush(resourceKey), this.config.autoSaveDelayMs);
    const saveState: FileSaveState = entry.saveState.kind === 'error'
      ? { ...entry.saveState, localRevision }
      : entry.saveState.kind === 'conflict'
        ? { ...entry.saveState, localRevision }
        : { kind: 'scheduled', localRevision };
    this.setEntry(resourceKey, {
      ...entry,
      snapshot: { ...entry.snapshot, content },
      localRevision,
      saveState,
    });
  }

  async flush(resourceKey: string): Promise<boolean> {
    const entry = this.entries.get(resourceKey);
    const runtime = this.runtimes.get(resourceKey);
    if (!entry || !runtime || entry.snapshot?.kind !== 'text') return true;
    if (runtime.timer) {
      clearTimeout(runtime.timer);
      runtime.timer = null;
    }
    if (runtime.promise) return runtime.promise;
    if (entry.localRevision <= entry.savedLocalRevision) return entry.saveState.kind !== 'conflict' && entry.saveState.kind !== 'error';
    runtime.promise = this.runSaveLoop(resourceKey).finally(() => {
      const current = this.runtimes.get(resourceKey);
      if (current) current.promise = null;
    });
    return runtime.promise;
  }

  async retry(resourceKey: string) {
    const entry = this.entries.get(resourceKey);
    if (!entry) return false;
    if (entry.saveState.kind === 'conflict') return false;
    this.setEntry(resourceKey, {
      ...entry,
      saveState: entry.localRevision > entry.savedLocalRevision
        ? { kind: 'scheduled', localRevision: entry.localRevision }
        : { kind: 'clean' },
    });
    return this.flush(resourceKey);
  }

  async forceOverwrite(resourceKey: string) {
    const runtime = this.runtimes.get(resourceKey);
    if (!runtime) return true;
    if (runtime.promise) await runtime.promise;
    runtime.promise = this.runSaveLoop(resourceKey, true).finally(() => {
      const current = this.runtimes.get(resourceKey);
      if (current) current.promise = null;
    });
    return runtime.promise;
  }

  async reload(resourceKey: string, preferSource?: boolean) {
    const entry = this.entries.get(resourceKey);
    if (!entry) return;
    const showSvgSource = entry.snapshot?.kind === 'text'
      && entry.resource.locator.canonicalPath.toLowerCase().endsWith('.svg');
    await this.load(entry.resource, preferSource ?? showSvgSource, true);
  }

  async startProjectWatch(projectId: string) {
    const refs = this.projectWatchRefs.get(projectId) ?? 0;
    this.projectWatchRefs.set(projectId, refs + 1);
    if (refs > 0) return;
    await this.ensureEventSubscription();
    await this.queueWatchOperation(projectId, () => startWorkspaceFileWatch(projectId));
  }

  async stopProjectWatch(projectId: string) {
    const refs = this.projectWatchRefs.get(projectId) ?? 0;
    if (refs > 1) {
      this.projectWatchRefs.set(projectId, refs - 1);
      return;
    }
    this.projectWatchRefs.delete(projectId);
    await this.queueWatchOperation(projectId, () => stopWorkspaceFileWatch(projectId));
    if (this.projectWatchRefs.size === 0 && this.eventUnsubscribe) {
      this.eventUnsubscribe();
      this.eventUnsubscribe = null;
    }
  }

  async close(resourceKey: string) {
    const entry = this.entries.get(resourceKey);
    if (!entry) {
      await this.releasePrimedGrant(resourceKey);
      return true;
    }
    const saved = await this.flush(resourceKey);
    if (!saved) return false;
    await this.release(resourceKey);
    return true;
  }

  async release(resourceKey: string) {
    const entry = this.entries.get(resourceKey);
    const runtime = this.runtimes.get(resourceKey);
    if (runtime?.timer) clearTimeout(runtime.timer);
    if (runtime?.renewalTimer) clearTimeout(runtime.renewalTimer);
    if (runtime?.previewRefreshTimer) clearTimeout(runtime.previewRefreshTimer);
    const previewToken = entry?.snapshot?.kind === 'image' ? entry.snapshot.previewToken : null;
    const markdownPreviewTokens = runtime
      ? [...runtime.markdownImages.values()].flatMap((image) => image.kind === 'ready' ? [image.previewToken] : [])
      : [];
    const grant = entry?.snapshot?.externalAccessGrant
      ?? runtime?.externalAccessGrant
      ?? this.primedGrants.get(resourceKey)?.grant;
    this.primedGrants.delete(resourceKey);
    this.runtimes.delete(resourceKey);
    this.entries.delete(resourceKey);
    this.emit();
    await Promise.all([
      previewToken ? releaseWorkspaceFilePreview(previewToken).catch(() => undefined) : Promise.resolve(),
      ...markdownPreviewTokens.map((token) => releaseWorkspaceFilePreview(token).catch(() => undefined)),
      grant ? releaseExternalFileAccess(grant.token).catch(() => undefined) : Promise.resolve(),
    ]);
  }

  async flushAll(projectId?: string) {
    const keys = [...this.entries.entries()]
      .filter(([, entry]) => !projectId || entry.resource.projectId === projectId)
      .map(([key]) => key);
    const results = await Promise.all(keys.map((key) => this.flush(key)));
    return results.every(Boolean);
  }

  async releaseProject(projectId: string) {
    const saved = await this.flushAll(projectId);
    if (!saved) return false;
    const keys = new Set(
      [...this.entries.entries()]
        .filter(([, entry]) => entry.resource.projectId === projectId)
        .map(([key]) => key),
    );
    for (const [key, primed] of this.primedGrants) {
      if (primed.projectId === projectId) keys.add(key);
    }
    await Promise.all([...keys].map((key) => this.release(key)));
    return true;
  }

  imageViewState(resourceKey: string): FileImageViewState {
    return this.runtimes.get(resourceKey)?.imageViewState ?? { zoom: 1, scrollLeft: 0, scrollTop: 0 };
  }

  persistImageViewState(resourceKey: string, state: FileImageViewState) {
    const runtime = this.runtimes.get(resourceKey);
    if (runtime) runtime.imageViewState = state;
  }

  async reauthorize(resourceKey: string, grant: ExternalFileAccessGrantVm) {
    const entry = this.entries.get(resourceKey);
    const runtime = this.runtimes.get(resourceKey);
    if (!entry || !runtime) return false;
    const previous = runtime.externalAccessGrant;
    if (previous && previous.token !== grant.token) {
      await releaseExternalFileAccess(previous.token).catch(() => undefined);
    }
    runtime.externalAccessGrant = grant;
    this.primedGrants.set(resourceKey, {
      projectId: entry.resource.projectId,
      canonicalPath: entry.resource.locator.canonicalPath,
      grant,
    });
    if (runtime.renewalTimer) clearTimeout(runtime.renewalTimer);
    const saveState = entry.localRevision > entry.savedLocalRevision
      ? { kind: 'scheduled' as const, localRevision: entry.localRevision }
      : { kind: 'clean' as const };
    this.setEntry(resourceKey, {
      ...entry,
      status: entry.snapshot ? 'ready' : entry.status,
      errorCode: entry.snapshot ? null : entry.errorCode,
      snapshot: entry.snapshot ? { ...entry.snapshot, externalAccessGrant: grant } : entry.snapshot,
      saveState,
    });
    this.scheduleGrantRenewal(resourceKey, runtime);
    return this.flush(resourceKey);
  }

  private async runSaveLoop(resourceKey: string, force = false): Promise<boolean> {
    let firstWrite = true;
    while (true) {
      const entry = this.entries.get(resourceKey);
      const runtime = this.runtimes.get(resourceKey);
      if (!entry || !runtime || entry.snapshot?.kind !== 'text') return true;
      if (!force && (entry.saveState.kind === 'conflict' || entry.saveState.kind === 'error')) return false;
      if (!firstWrite && entry.localRevision <= entry.savedLocalRevision) return true;
      firstWrite = false;
      const targetLocalRevision = entry.localRevision;
      const operation = operationId();
      this.setEntry(resourceKey, {
        ...entry,
        saveState: { kind: 'saving', localRevision: targetLocalRevision, operationId: operation },
      });
      try {
        const revision = await writeFileResource({
          projectId: entry.resource.projectId,
          canonicalPath: entry.resource.locator.canonicalPath,
          externalAccessToken: runtime.externalAccessGrant?.token ?? null,
          content: runtime.latestContent,
          encoding: runtime.encoding,
          lineEnding: runtime.lineEnding,
          expectedRevision: runtime.diskRevision,
          operationId: operation,
          force,
        });
        runtime.diskRevision = revision;
        const current = this.entries.get(resourceKey);
        if (!current || current.snapshot?.kind !== 'text') return true;
        const savedLocalRevision = Math.max(current.savedLocalRevision, targetLocalRevision);
        this.setEntry(resourceKey, {
          ...current,
          snapshot: { ...current.snapshot, content: runtime.latestContent, revision },
          savedLocalRevision,
          saveState: current.localRevision <= savedLocalRevision
            ? { kind: 'clean' }
            : { kind: 'scheduled', localRevision: current.localRevision },
        });
        force = false;
        if (current.localRevision <= savedLocalRevision) return true;
      } catch (reason) {
        const current = this.entries.get(resourceKey);
        if (!current) return false;
        const code = errorCode(reason);
        this.setEntry(resourceKey, {
          ...current,
          saveState: code === 'workspace-file.changed-on-disk'
            ? { kind: 'conflict', localRevision: current.localRevision, diskRevision: null }
            : { kind: 'error', localRevision: current.localRevision, errorCode: code },
        });
        return false;
      }
    }
  }

  private installRuntime(key: string, snapshot: WorkspaceFileSnapshotVm) {
    const existing = this.runtimes.get(key);
    if (existing?.timer) clearTimeout(existing.timer);
    if (existing?.renewalTimer) clearTimeout(existing.renewalTimer);
    if (existing?.previewRefreshTimer) clearTimeout(existing.previewRefreshTimer);
    if (existing) {
      for (const image of existing.markdownImages.values()) {
        if (image.kind === 'ready') {
          void releaseWorkspaceFilePreview(image.previewToken).catch(() => undefined);
        }
      }
    }
    const runtime: SaveRuntime = {
      timer: null,
      promise: null,
      latestContent: snapshot.kind === 'text' ? snapshot.content : '',
      encoding: snapshot.kind === 'text' ? snapshot.encoding : 'utf-8',
      lineEnding: snapshot.kind === 'text' ? snapshot.lineEnding : 'lf',
      diskRevision: snapshot.revision,
      externalAccessGrant: snapshot.externalAccessGrant,
      renewalTimer: null,
      previewRefreshTimer: null,
      // A disk reload creates a new undo boundary. Tab deactivation does not call load,
      // so normal Tab switches still preserve the serialized CodeMirror history.
      editorStateJson: null,
      imageViewState: existing?.imageViewState ?? { zoom: 1, scrollLeft: 0, scrollTop: 0 },
      markdownMode: existing?.markdownMode ?? 'live-preview',
      markdownImages: new Map(),
      markdownImageSources: new Set(),
      markdownImageRequestRevision: (existing?.markdownImageRequestRevision ?? 0) + 1,
      approvedMarkdownImageTargets: existing?.approvedMarkdownImageTargets ?? new Set(),
    };
    this.runtimes.set(key, runtime);
    this.scheduleGrantRenewal(key, runtime);
    this.schedulePreviewRefresh(key, runtime, snapshot);
  }

  private scheduleGrantRenewal(key: string, runtime: SaveRuntime) {
    const grant = runtime.externalAccessGrant;
    if (!grant) return;
    const remaining = Number(grant.expiresAtMs) - Date.now();
    const delay = Math.max(1_000, remaining - 60_000);
    runtime.renewalTimer = setTimeout(async () => {
      try {
        const next = await renewExternalFileAccess(grant.token);
        runtime.externalAccessGrant = next;
        const entry = this.entries.get(key);
        if (entry?.snapshot) this.setEntry(key, { ...entry, snapshot: { ...entry.snapshot, externalAccessGrant: next } });
        this.scheduleGrantRenewal(key, runtime);
      } catch (reason) {
        const entry = this.entries.get(key);
        if (entry) this.setEntry(key, { ...entry, saveState: { kind: 'error', localRevision: entry.localRevision, errorCode: errorCode(reason) } });
      }
    }, delay);
  }

  private schedulePreviewRefresh(key: string, runtime: SaveRuntime, snapshot: WorkspaceFileSnapshotVm) {
    if (snapshot.kind !== 'image') return;
    const delay = Math.max(1_000, this.config.previewTokenTtlSeconds * 1_000 - 60_000);
    runtime.previewRefreshTimer = setTimeout(() => {
      const entry = this.entries.get(key);
      if (entry?.snapshot?.kind === 'image' && entry.saveState.kind === 'clean') {
        void this.load(entry.resource, false, true);
      }
    }, delay);
  }

  private async releasePrimedGrant(resourceKey: string) {
    const primed = this.primedGrants.get(resourceKey);
    this.primedGrants.delete(resourceKey);
    if (primed) await releaseExternalFileAccess(primed.grant.token).catch(() => undefined);
  }

  private async ensureEventSubscription() {
    if (this.eventUnsubscribe) return;
    if (!this.eventSubscriptionPromise) {
      this.eventSubscriptionPromise = subscribeWorkspaceFileChanges((event) => void this.handleFileChange(event))
        .then((unsubscribe) => {
          this.eventUnsubscribe = unsubscribe;
        })
        .finally(() => {
          this.eventSubscriptionPromise = null;
        });
    }
    await this.eventSubscriptionPromise;
  }

  private async queueWatchOperation(projectId: string, operation: () => Promise<void>) {
    const previous = this.projectWatchOperations.get(projectId) ?? Promise.resolve();
    const next = previous.catch(() => undefined).then(operation).catch(() => undefined);
    this.projectWatchOperations.set(projectId, next);
    await next;
    if (this.projectWatchOperations.get(projectId) === next) {
      this.projectWatchOperations.delete(projectId);
    }
  }

  private async handleFileChange(event: WorkspaceFileChangedEventVm) {
    for (const listener of this.changeListeners) listener(event);
    for (const [key, entry] of this.entries) {
      if (
        entry.resource.projectId !== event.projectId
        || normalizePathKey(entry.resource.locator.canonicalPath) !== normalizePathKey(event.canonicalPath)
      ) continue;
      const runtime = this.runtimes.get(key);
      if (event.operationId && entry.saveState.kind === 'saving' && event.operationId === entry.saveState.operationId) continue;
      if (event.revision && runtime?.diskRevision.contentHash === event.revision.contentHash) continue;
      if (event.kind === 'removed') {
        this.setEntry(key, {
          ...entry,
          status: entry.snapshot ? 'ready' : 'error',
          errorCode: entry.snapshot ? null : 'workspace-file.not-found',
          saveState: entry.snapshot
            ? { kind: 'error', localRevision: entry.localRevision, errorCode: 'workspace-file.not-found' }
            : entry.saveState,
        });
        continue;
      }
      if (entry.saveState.kind !== 'clean') {
        this.setEntry(key, { ...entry, saveState: { kind: 'conflict', localRevision: entry.localRevision, diskRevision: event.revision } });
        continue;
      }
      await this.load(entry.resource, entry.snapshot?.kind === 'text' && entry.resource.locator.canonicalPath.toLowerCase().endsWith('.svg'), true);
    }
  }

  private setEntry(key: string, entry: FileContentEntry) {
    this.entries.set(key, entry);
    this.emit();
  }

  private touch(key: string) {
    const entry = this.entries.get(key);
    if (!entry) return;
    this.entries.delete(key);
    this.entries.set(key, entry);
  }

  private prune() {
    let totalBytes = [...this.entries.values()].reduce((total, entry) => total + (entry.snapshot?.revision.byteLength ?? 0), 0);
    for (const [key, entry] of this.entries) {
      if (this.entries.size <= this.config.contentCacheEntries && totalBytes <= this.config.contentCacheMaxBytes) break;
      if (entry.saveState.kind !== 'clean') continue;
      totalBytes -= entry.snapshot?.revision.byteLength ?? 0;
      void this.release(key);
    }
  }

  private emit() {
    for (const listener of this.listeners) listener();
  }
}

function normalizePathKey(path: string) {
  return /^[a-z]:[\\/]/iu.test(path) ? path.replaceAll('/', '\\').toLowerCase() : path;
}

export const fileContentStore = new FileContentStore();

export function useFileContentEntry(resourceKey: string) {
  return useSyncExternalStore(
    fileContentStore.subscribe,
    () => fileContentStore.snapshot(resourceKey),
    () => fileContentStore.snapshot(resourceKey),
  );
}
