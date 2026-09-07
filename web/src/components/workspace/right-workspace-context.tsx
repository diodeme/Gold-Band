import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, useRef, useState, type ReactNode } from 'react';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import type { AttachmentItem } from '@/lib/attachment-service';
import { RIGHT_WORKSPACE_DEFAULT_WIDTH } from './workspace-layout';
import {
  normalizeSourceControlWorkspacePath,
  sameSourceControlWorkspacePath,
} from './source-control/source-control-identity';

export interface AgentTranscriptLocator {
  projectId: string;
  taskId: string;
  taskUuid?: string | null;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  branchId: string;
}

export interface ConversationRunLocator {
  projectId: string;
  taskId: string;
  taskUuid?: string | null;
  runId: string;
}

export interface AcpAttemptWorkspaceLocator extends ConversationRunLocator {
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  branchId: string;
}

export interface HiddenPromptSectionWorkspaceLocator extends AcpAttemptWorkspaceLocator {
  eventId: string;
  eventSeq: number;
  partIndex: number;
}

export type ConversationWorkspaceScope =
  | { kind: 'draft'; key: string; projectId: string }
  | { kind: 'conversation'; key: string; projectId: string; taskId: string; taskUuid?: string | null; runId: string };

interface RightWorkspaceResourceBase {
  key: string;
  scopeKey: string;
  title: string;
  description?: string | null;
  attention: boolean;
}

export type FileBrowserWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'file-browser';
  projectId: string;
  selectedFile?: FileWorkspaceResource | null;
};

export type ConversationDirectoryWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'conversation-directory';
  locator: ConversationRunLocator & { roundId: string; nodeId: string; attemptId: string; outerNodeId?: string | null; outerAttemptId?: string | null };
};

export type ConversationDirectoryWorkspaceEntry = Omit<ConversationDirectoryWorkspaceResource, 'key'>;

export type AgentTranscriptResource = RightWorkspaceResourceBase & {
  kind: 'agent-transcript';
  status: string;
  locator: AgentTranscriptLocator;
};

export type FileWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'file';
  projectId: string;
  locator: import('@/types').WorkspaceFileLocatorVm;
  target: import('@/types').FileTargetLocationVm | null;
  targetRevision: number;
};

export type TurnFileWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'file-diff' | 'file-version';
  locator: import('@/types').TurnFileLocatorVm;
  changeSetId: string;
  changeId: string;
};

export type GitFileComparisonWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'file-diff';
  projectId: string;
  gitSource: import('@/types').GitComparisonSourceVm;
  reviewSessionId?: string | null;
  reviewItemId?: string | null;
  reviewLanding?: 'top' | 'first-change' | 'last-change' | null;
};

export type TurnAttachmentWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'turn-attachment';
  locator: import('@/types').TurnFileLocatorVm;
  changeSetId: string;
  attachmentId: string;
};

export type SourceControlWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'source-control';
  projectId: string;
  workspacePath?: string | null;
};

export type ConversationAssetWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'conversation-asset';
  locator: AcpAttemptWorkspaceLocator;
  assetKind: 'artifact' | 'message-attachment' | 'input-attachment';
  name: string;
  path?: string | null;
};

export type DraftAttachmentWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'draft-attachment';
  projectId: string;
  attachment: AttachmentItem;
};

export type WorkflowViewWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'workflow-view';
  locator: ConversationRunLocator;
};

export type WorkflowEditWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'workflow-edit';
  mode: 'edit' | 'repair';
  locator: ConversationRunLocator;
};

export type SystemPromptWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'system-prompt';
  locator: AcpAttemptWorkspaceLocator;
};

export type HiddenPromptSectionWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'hidden-prompt-section';
  locator: HiddenPromptSectionWorkspaceLocator;
};

export type RawFramesWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'raw-frames';
  locator: AcpAttemptWorkspaceLocator;
};

export type ScheduledTaskConfigWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'scheduled-task-config';
};

export type RightWorkspaceResource =
  | AgentTranscriptResource
  | FileBrowserWorkspaceResource
  | ConversationDirectoryWorkspaceResource
  | FileWorkspaceResource
  | TurnFileWorkspaceResource
  | TurnAttachmentWorkspaceResource
  | GitFileComparisonWorkspaceResource
  | SourceControlWorkspaceResource
  | ConversationAssetWorkspaceResource
  | DraftAttachmentWorkspaceResource
  | WorkflowViewWorkspaceResource
  | WorkflowEditWorkspaceResource
  | SystemPromptWorkspaceResource
  | HiddenPromptSectionWorkspaceResource
  | RawFramesWorkspaceResource
  | ScheduledTaskConfigWorkspaceResource;

export interface RightWorkspaceSessionState {
  tabs: RightWorkspaceResource[];
  activeTabKey: string | null;
}

export interface RightWorkspaceShellState {
  requestedOpen: boolean;
  openRevision: number;
  width: number;
}

type RightWorkspacePresentationState = Pick<RightWorkspaceShellState, 'requestedOpen' | 'openRevision'>;

export interface RightWorkspaceState extends RightWorkspaceSessionState, RightWorkspaceShellState {
  scopeKey: string | null;
}

interface RightWorkspaceContextValue extends RightWorkspaceState {
  openResource: (resource: RightWorkspaceResource) => void | Promise<void>;
  openWorkspace: () => void;
  activateTab: (key: string) => void | Promise<void>;
  closeTab: (key: string) => void | Promise<void>;
  closeWorkspace: () => void | Promise<void>;
  setWidth: (width: number) => void;
  renderResource: (resource: RightWorkspaceResource) => ReactNode;
  projectId: string | null;
  conversationDirectoryEntry: ConversationDirectoryWorkspaceEntry | null;
  setConversationDirectoryEntry: (entry: ConversationDirectoryWorkspaceEntry | null) => void;
  registerResourceRenderer: (kind: RightWorkspaceResourceKind, renderer: RightWorkspaceResourceRenderer) => () => void;
  registerResourceCloseResolver: (kind: RightWorkspaceResourceKind, resolver: RightWorkspaceResourceCloseResolver) => () => void;
}

export interface RightWorkspaceCommands {
  scopeKey: string | null;
  projectId: string | null;
  openResource: (resource: RightWorkspaceResource) => void | Promise<void>;
  closeTab: (key: string) => void | Promise<void>;
  getResource: (key: string) => RightWorkspaceResource | null;
}

export type RightWorkspaceResourceKind = RightWorkspaceResource['kind'];
export type RightWorkspaceResourceRenderer = (resource: RightWorkspaceResource) => ReactNode;
export type RightWorkspaceResourceTransitionReason = 'deactivate' | 'close' | 'workspace-close' | 'scope-change';
export type RightWorkspaceResourceCloseResolver = (
  resource: RightWorkspaceResource,
  reason: RightWorkspaceResourceTransitionReason,
) => boolean | Promise<boolean>;

export type RightWorkspaceAction =
  | { type: 'open'; resource: RightWorkspaceResource }
  | { type: 'synchronize'; resource: RightWorkspaceResource }
  | { type: 'activate'; key: string }
  | { type: 'close'; key: string };

export const DEFAULT_RIGHT_WORKSPACE_WIDTH = RIGHT_WORKSPACE_DEFAULT_WIDTH;
export const CONVERSATION_WORKSPACE_LRU_LIMIT = 24;
const RightWorkspaceContext = createContext<RightWorkspaceContextValue | null>(null);
const RightWorkspaceCommandsContext = createContext<RightWorkspaceCommands | null>(null);

export function createDraftConversationWorkspaceScope(projectId: string): ConversationWorkspaceScope {
  return { kind: 'draft', key: `draft:${projectId}`, projectId };
}

export function createConversationWorkspaceScope(input: {
  projectId: string;
  taskId: string;
  taskUuid?: string | null;
  runId: string;
}): ConversationWorkspaceScope {
  return {
    kind: 'conversation',
    key: `conversation:${input.projectId}:${input.taskUuid ?? 'missing-task-uuid'}:${input.runId}`,
    ...input,
  };
}

export function createInitialRightWorkspaceState(): RightWorkspaceSessionState {
  return { tabs: [], activeTabKey: null };
}

function cloneRightWorkspaceState(state: RightWorkspaceSessionState): RightWorkspaceSessionState {
  return { ...state, tabs: [...state.tabs] };
}

interface StoredConversationWorkspace {
  scope: ConversationWorkspaceScope;
  state: RightWorkspaceSessionState;
  presentation: RightWorkspacePresentationState;
}

function createInitialRightWorkspacePresentation(): RightWorkspacePresentationState {
  return { requestedOpen: false, openRevision: 0 };
}

export class ConversationWorkspaceStore {
  private readonly entries = new BoundedLruCache<string, StoredConversationWorkspace>(CONVERSATION_WORKSPACE_LRU_LIMIT);
  private width = RIGHT_WORKSPACE_DEFAULT_WIDTH;
  private widthInitialized = false;
  private widthTouched = false;

  restore(scope: ConversationWorkspaceScope) {
    const stored = this.entries.get(scope.key);
    return stored ? cloneRightWorkspaceState(stored.state) : createInitialRightWorkspaceState();
  }

  peek(scope: ConversationWorkspaceScope) {
    const stored = this.entries.peek(scope.key);
    return stored ? cloneRightWorkspaceState(stored.state) : createInitialRightWorkspaceState();
  }

  save(scope: ConversationWorkspaceScope, state: RightWorkspaceSessionState) {
    const stored = this.entries.peek(scope.key);
    this.entries.set(scope.key, {
      scope,
      state: cloneRightWorkspaceState(state),
      presentation: stored?.presentation ?? createInitialRightWorkspacePresentation(),
    });
  }

  touch(scope: ConversationWorkspaceScope) {
    this.entries.get(scope.key);
  }

  peekShellState(scope: ConversationWorkspaceScope | null, initialWidth?: number): RightWorkspaceShellState {
    const presentation = scope
      ? this.entries.peek(scope.key)?.presentation ?? createInitialRightWorkspacePresentation()
      : createInitialRightWorkspacePresentation();
    return {
      ...presentation,
      width: !this.widthInitialized && initialWidth != null ? initialWidth : this.width,
    };
  }

  hydrateWidth(width: number) {
    if (this.widthTouched) return false;
    const shouldRender = this.widthInitialized && this.width !== width;
    this.widthInitialized = true;
    this.width = width;
    return shouldRender;
  }

  setWidth(width: number) {
    this.widthInitialized = true;
    this.widthTouched = true;
    if (this.width === width) return false;
    this.width = width;
    return true;
  }

  openWorkspace(scope: ConversationWorkspaceScope, { explicit }: { explicit: boolean }) {
    const stored = this.entries.peek(scope.key);
    const presentation = stored?.presentation ?? createInitialRightWorkspacePresentation();
    this.entries.set(scope.key, {
      scope,
      state: cloneRightWorkspaceState(stored?.state ?? createInitialRightWorkspaceState()),
      presentation: {
        ...presentation,
        requestedOpen: true,
        openRevision: explicit ? presentation.openRevision + 1 : presentation.openRevision,
      },
    });
  }

  closeWorkspace(scope: ConversationWorkspaceScope) {
    const stored = this.entries.peek(scope.key);
    if (!stored?.presentation.requestedOpen) return false;
    this.entries.set(scope.key, {
      scope,
      state: cloneRightWorkspaceState(stored.state),
      presentation: {
        ...stored.presentation,
        requestedOpen: false,
      },
    });
    return true;
  }

  promoteDraft(draft: ConversationWorkspaceScope, conversation: ConversationWorkspaceScope) {
    if (
      draft.kind !== 'draft'
      || conversation.kind !== 'conversation'
      || draft.projectId !== conversation.projectId
    ) return;
    const draftWorkspace = this.entries.peek(draft.key);
    if (!draftWorkspace) return;
    const promotedTabs = draftWorkspace.state.tabs.map((resource) => promoteDraftWorkspaceResource(resource, conversation.key));
    const promotedActiveTabKey = draftWorkspace.state.activeTabKey == null
      ? null
      : promotedTabs[draftWorkspace.state.tabs.findIndex((resource) => resource.key === draftWorkspace.state.activeTabKey)]?.key ?? null;
    this.entries.set(conversation.key, {
      scope: conversation,
      state: {
        tabs: promotedTabs,
        activeTabKey: promotedActiveTabKey,
      },
      presentation: { ...draftWorkspace.presentation },
    });
    this.entries.delete(draft.key);
  }

  deleteConversation(projectId: string, taskId: string) {
    this.entries.deleteWhere(({ scope }) => scope.kind === 'conversation' && scope.projectId === projectId && scope.taskId === taskId);
  }

  deleteProject(projectId: string) {
    this.entries.deleteWhere(({ scope }) => scope.projectId === projectId);
  }

  has(scope: ConversationWorkspaceScope) {
    return this.entries.peek(scope.key) !== undefined;
  }

  keys() {
    return this.entries.keys();
  }
}

export function rightWorkspaceReducer(state: RightWorkspaceSessionState, action: RightWorkspaceAction): RightWorkspaceSessionState {
  switch (action.type) {
    case 'open': {
      const fileProjectId = action.resource.kind === 'file' ? action.resource.projectId : null;
      const existingFileBrowser = fileProjectId
        ? state.tabs.find((tab): tab is FileBrowserWorkspaceResource => tab.kind === 'file-browser' && tab.projectId === fileProjectId)
        : null;
      const resource = action.resource.kind === 'file'
        ? {
          kind: 'file-browser' as const,
          key: fileBrowserWorkspaceResourceKey(action.resource.projectId),
          scopeKey: action.resource.scopeKey,
          projectId: action.resource.projectId,
          title: existingFileBrowser?.title ?? action.resource.title,
          description: existingFileBrowser?.description ?? action.resource.description,
          attention: action.resource.attention,
          selectedFile: action.resource,
        }
        : action.resource;
      const existing = state.tabs.findIndex((tab) => tab.key === resource.key);
      const tabs = existing < 0
        ? [...state.tabs, resource]
        : state.tabs.map((tab, index) => index === existing ? resource : tab);
      return {
        ...state,
        tabs,
        activeTabKey: resource.key,
      };
    }
    case 'synchronize': {
      const existing = state.tabs.findIndex((tab) => tab.key === action.resource.key);
      if (existing < 0) return state;
      return {
        ...state,
        tabs: state.tabs.map((tab, index) => index === existing ? action.resource : tab),
      };
    }
    case 'activate':
      return state.tabs.some((tab) => tab.key === action.key)
        ? { ...state, activeTabKey: action.key }
        : state;
    case 'close': {
      const index = state.tabs.findIndex((tab) => tab.key === action.key);
      if (index < 0) return state;
      const tabs = state.tabs.filter((tab) => tab.key !== action.key);
      const activeTabKey = state.activeTabKey === action.key
        ? (tabs[Math.min(index, tabs.length - 1)]?.key ?? null)
        : state.activeTabKey;
      return {
        ...state,
        tabs,
        activeTabKey,
      };
    }
  }
}

function promoteDraftWorkspaceResource(
  resource: RightWorkspaceResource,
  scopeKey: string,
): RightWorkspaceResource {
  if (resource.kind === 'draft-attachment') {
    return {
      ...resource,
      key: draftAttachmentWorkspaceResourceKey(scopeKey, resource.attachment.id),
      scopeKey,
    };
  }
  if (resource.kind === 'scheduled-task-config') {
    return {
      ...resource,
      key: scheduledTaskConfigWorkspaceResourceKey(scopeKey),
      scopeKey,
    };
  }
  if (resource.kind === 'file-browser') {
    return {
      ...resource,
      scopeKey,
      selectedFile: resource.selectedFile
        ? { ...resource.selectedFile, scopeKey }
        : resource.selectedFile,
    };
  }
  return { ...resource, scopeKey };
}

const DEFAULT_SCOPE = createDraftConversationWorkspaceScope('default');

export function RightWorkspaceProvider({
  initialWidth,
  scope = DEFAULT_SCOPE,
  store,
  sourceControlWorkspacePath = null,
  children,
}: {
  initialWidth?: number;
  scope?: ConversationWorkspaceScope | null;
  store?: ConversationWorkspaceStore;
  sourceControlWorkspacePath?: string | null;
  children: ReactNode;
}) {
  const internalStoreRef = useRef<ConversationWorkspaceStore | null>(null);
  if (!internalStoreRef.current) internalStoreRef.current = new ConversationWorkspaceStore();
  const effectiveStore = store ?? internalStoreRef.current;
  const scopeRef = useRef(scope);
  scopeRef.current = scope;
  const sourceControlWorkspacePathRef = useRef(sourceControlWorkspacePath);
  sourceControlWorkspacePathRef.current = sourceControlWorkspacePath;
  const sourceControlWorkspaceIdentity = normalizeSourceControlWorkspacePath(sourceControlWorkspacePath);
  const [conversationDirectoryEntry, setConversationDirectoryEntryState] = useState<ConversationDirectoryWorkspaceEntry | null>(null);
  const [revision, render] = useReducer((currentRevision) => currentRevision + 1, 0);
  const rendererRegistryRef = useRef(new Map<RightWorkspaceResourceKind, RightWorkspaceResourceRenderer>());
  const closeResolverRegistryRef = useRef(new Map<RightWorkspaceResourceKind, RightWorkspaceResourceCloseResolver>());
  const previousScopeRef = useRef<ConversationWorkspaceScope | null>(scope);
  const [rendererRevision, renderRenderer] = useReducer((currentRevision) => currentRevision + 1, 0);
  const peekProjectedState = useCallback((targetScope: ConversationWorkspaceScope) => (
    projectSourceControlWorkspaceState(
      effectiveStore.peek(targetScope),
      sourceControlWorkspacePathRef.current,
    )
  ), [effectiveStore]);
  const sessionState = useMemo(
    () => scope ? peekProjectedState(scope) : createInitialRightWorkspaceState(),
    [peekProjectedState, revision, scope, sourceControlWorkspaceIdentity],
  );
  const shellState = useMemo(
    () => effectiveStore.peekShellState(scope, initialWidth),
    [effectiveStore, initialWidth, revision, scope],
  );

  useEffect(() => {
    if (scope) effectiveStore.touch(scope);
  }, [effectiveStore, scope]);

  useEffect(() => {
    const previous = previousScopeRef.current;
    previousScopeRef.current = scope;
    if (!previous || previous.key === scope?.key) return;
    const previousState = effectiveStore.peek(previous);
    const active = previousState.tabs.find((tab) => tab.key === previousState.activeTabKey);
    if (active) void closeResolverRegistryRef.current.get(active.kind)?.(active, 'scope-change');
  }, [effectiveStore, scope]);

  useEffect(() => {
    if (initialWidth == null || !effectiveStore.hydrateWidth(initialWidth)) return;
    render();
  }, [effectiveStore, initialWidth]);

  const commit = useCallback((action: RightWorkspaceAction) => {
    const currentScope = scopeRef.current;
    if (!currentScope) return null;
    const current = peekProjectedState(currentScope);
    if (
      (action.type === 'open' || action.type === 'synchronize')
      && action.resource.scopeKey !== currentScope.key
    ) return null;
    const next = rightWorkspaceReducer(current, action);
    if (next === current) return null;
    effectiveStore.save(currentScope, next);
    return next;
  }, [effectiveStore, peekProjectedState]);
  const openResource = useCallback(async (resource: RightWorkspaceResource) => {
    const currentScope = scopeRef.current;
    if (!currentScope) return;
    const current = peekProjectedState(currentScope);
    if (current.activeTabKey && current.activeTabKey !== resource.key) {
      const active = current.tabs.find((tab) => tab.key === current.activeTabKey);
      if (active && await closeResolverRegistryRef.current.get(active.kind)?.(active, 'deactivate') === false) return;
    }
    if (!commit({ type: 'open', resource })) return;
    effectiveStore.openWorkspace(currentScope, { explicit: true });
    render();
  }, [commit, effectiveStore, peekProjectedState]);
  const getResource = useCallback((key: string) => {
    const currentScope = scopeRef.current;
    if (!currentScope) return null;
    return peekProjectedState(currentScope).tabs.find((tab) => tab.key === key) ?? null;
  }, [peekProjectedState]);
  const openWorkspace = useCallback(() => {
    const currentScope = scopeRef.current;
    if (!currentScope) return;
    effectiveStore.openWorkspace(currentScope, { explicit: true });
    render();
  }, [effectiveStore]);
  const activateTab = useCallback(async (key: string) => {
    if (!scope) return;
    const current = peekProjectedState(scope);
    if (current.activeTabKey && current.activeTabKey !== key) {
      const active = current.tabs.find((tab) => tab.key === current.activeTabKey);
      if (active && await closeResolverRegistryRef.current.get(active.kind)?.(active, 'deactivate') === false) return;
    }
    if (!commit({ type: 'activate', key })) return;
    effectiveStore.openWorkspace(scope, { explicit: false });
    render();
  }, [commit, effectiveStore, peekProjectedState, scope]);
  const closeTab = useCallback(async (key: string) => {
    if (!scope) return;
    const resource = peekProjectedState(scope).tabs.find((tab) => tab.key === key);
    if (resource && await closeResolverRegistryRef.current.get(resource.kind)?.(resource, 'close') === false) return;
    const next = commit({ type: 'close', key });
    if (!next) return;
    if (next.tabs.length === 0) effectiveStore.closeWorkspace(scope);
    render();
  }, [commit, effectiveStore, peekProjectedState, scope]);
  const closeWorkspace = useCallback(async () => {
    if (!scope) return;
    const current = peekProjectedState(scope);
    const active = current.tabs.find((tab) => tab.key === current.activeTabKey);
    if (active && await closeResolverRegistryRef.current.get(active.kind)?.(active, 'workspace-close') === false) return;
    effectiveStore.closeWorkspace(scope);
    render();
  }, [effectiveStore, peekProjectedState, scope]);
  const setWidth = useCallback((nextWidth: number) => {
    if (effectiveStore.setWidth(nextWidth)) render();
  }, [effectiveStore]);
  const setConversationDirectoryEntry = useCallback((entry: ConversationDirectoryWorkspaceEntry | null) => {
    setConversationDirectoryEntryState(entry);
    if (!entry) return;
    const resource: ConversationDirectoryWorkspaceResource = {
      ...entry,
      key: conversationDirectoryWorkspaceResourceKey(entry.locator),
    };
    if (commit({ type: 'synchronize', resource })) render();
  }, [commit]);
  const renderResource = useCallback((resource: RightWorkspaceResource) => rendererRegistryRef.current.get(resource.kind)?.(resource) ?? null, []);
  const registerResourceRenderer = useCallback((kind: RightWorkspaceResourceKind, renderer: RightWorkspaceResourceRenderer) => {
    rendererRegistryRef.current.set(kind, renderer);
    renderRenderer();
    return () => {
      if (rendererRegistryRef.current.get(kind) !== renderer) return;
      rendererRegistryRef.current.delete(kind);
      renderRenderer();
    };
  }, []);
  const registerResourceCloseResolver = useCallback((kind: RightWorkspaceResourceKind, resolver: RightWorkspaceResourceCloseResolver) => {
    closeResolverRegistryRef.current.set(kind, resolver);
    return () => {
      if (closeResolverRegistryRef.current.get(kind) === resolver) closeResolverRegistryRef.current.delete(kind);
    };
  }, []);
  const value = useMemo(() => ({
    ...sessionState,
    ...shellState,
    scopeKey: scope?.key ?? null,
    projectId: scope?.projectId ?? null,
    conversationDirectoryEntry: conversationDirectoryEntry?.scopeKey === scope?.key ? conversationDirectoryEntry : null,
    setConversationDirectoryEntry,
    openResource,
    openWorkspace,
    activateTab,
    closeTab,
    closeWorkspace,
    setWidth,
    renderResource,
    registerResourceRenderer,
    registerResourceCloseResolver,
  }), [activateTab, closeTab, closeWorkspace, conversationDirectoryEntry, openResource, openWorkspace, registerResourceCloseResolver, registerResourceRenderer, renderResource, rendererRevision, scope?.key, scope?.projectId, sessionState, setConversationDirectoryEntry, setWidth, shellState]);
  const commands = useMemo<RightWorkspaceCommands>(() => ({
    scopeKey: scope?.key ?? null,
    projectId: scope?.projectId ?? null,
    openResource,
    closeTab,
    getResource,
  }), [closeTab, getResource, openResource, scope?.key, scope?.projectId]);
  return (
    <RightWorkspaceCommandsContext.Provider value={commands}>
      <RightWorkspaceContext.Provider value={value}>{children}</RightWorkspaceContext.Provider>
    </RightWorkspaceCommandsContext.Provider>
  );
}

export function useRightWorkspace() {
  const value = useContext(RightWorkspaceContext);
  if (!value) throw new Error('useRightWorkspace must be used inside RightWorkspaceProvider');
  return value;
}

export function useOptionalRightWorkspace() {
  return useContext(RightWorkspaceContext);
}

export function useRightWorkspaceCommands() {
  const value = useContext(RightWorkspaceCommandsContext);
  if (!value) throw new Error('useRightWorkspaceCommands must be used inside RightWorkspaceProvider');
  return value;
}

export function useOptionalRightWorkspaceCommands() {
  return useContext(RightWorkspaceCommandsContext);
}

export function agentTranscriptResourceKey(locator: AgentTranscriptLocator) {
  return [
    locator.projectId,
    locator.taskUuid ?? 'missing-task-uuid',
    locator.runId,
    locator.roundId,
    locator.nodeId,
    locator.attemptId,
    locator.outerNodeId ?? '',
    locator.outerAttemptId ?? '',
    locator.branchId,
  ].join(':');
}

export function conversationRunWorkspaceResourceKey(kind: 'workflow-view' | 'workflow-edit', locator: ConversationRunLocator) {
  return `${kind}:${locator.projectId}:${locator.taskUuid ?? 'missing-task-uuid'}:${locator.runId}`;
}

export function fileBrowserWorkspaceResourceKey(projectId: string) {
  return `file-browser:${projectId}`;
}

export function sourceControlWorkspaceResourceKey(projectId: string) {
  return `source-control:${projectId}`;
}

export function projectSourceControlWorkspaceState(
  state: RightWorkspaceSessionState,
  workspacePath: string | null | undefined,
): RightWorkspaceSessionState {
  let changed = false;
  const tabs = state.tabs.map((tab) => {
    if (tab.kind !== 'source-control' || sameSourceControlWorkspacePath(tab.workspacePath, workspacePath)) {
      return tab;
    }
    changed = true;
    return { ...tab, workspacePath: workspacePath ?? null };
  });
  return changed ? { ...state, tabs } : state;
}

export function scheduledTaskConfigWorkspaceResourceKey(scopeKey: string) {
  return `scheduled-task-config:${scopeKey}`;
}

export function gitFileComparisonWorkspaceResourceKey(projectId: string, source: import('@/types').GitComparisonSourceVm) {
  const workspacePath = source.workspacePath?.replaceAll('\\', '/') ?? 'main';
  if (source.kind === 'workspace') {
    return `git-diff:${projectId}:${workspacePath}:workspace:${source.area}:${source.path}`;
  }
  if (source.kind === 'commit') {
    return `git-diff:${projectId}:${workspacePath}:commit:${source.beforeOid ?? ''}:${source.beforePath ?? ''}:${source.afterOid}:${source.path}`;
  }
  return `git-diff:${projectId}:${workspacePath}:github-pr:${source.host}:${source.repository}:${source.prNumber}:${source.baseOid}:${source.headOid}:${source.beforePath ?? ''}:${source.path}`;
}

export function gitDiffReviewWorkspaceResourceKey(projectId: string, reviewSessionId: string) {
  return `git-diff-review:${projectId}:${reviewSessionId}`;
}

export function conversationDirectoryWorkspaceResourceKey(locator: ConversationDirectoryWorkspaceResource['locator']) {
  return ['conversation-directory', locator.projectId, locator.taskUuid ?? 'missing-task-uuid', locator.runId].join(':');
}

export function conversationDirectoryWorkspaceDataKey(locator: ConversationDirectoryWorkspaceResource['locator']) {
  return [locator.projectId, locator.taskUuid ?? 'missing-task-uuid', locator.runId, locator.roundId, locator.outerNodeId ?? '', locator.outerAttemptId ?? '', locator.nodeId, locator.attemptId].join(':');
}

export function fileWorkspaceResourceKey(projectId: string, canonicalPath: string) {
  const normalizedPath = canonicalPath.replaceAll('\\', '/');
  const platformPath = /^[a-z]:\//iu.test(normalizedPath) ? normalizedPath.toLocaleLowerCase() : normalizedPath;
  return `file:${projectId}:${platformPath}`;
}

export function acpAttemptWorkspaceResourceKey(kind: 'system-prompt' | 'raw-frames', locator: AcpAttemptWorkspaceLocator) {
  return [
    kind,
    locator.projectId,
    locator.taskUuid ?? 'missing-task-uuid',
    locator.runId,
    locator.roundId,
    locator.nodeId,
    locator.attemptId,
    locator.outerNodeId ?? '',
    locator.outerAttemptId ?? '',
    locator.branchId,
  ].join(':');
}

export function conversationAssetWorkspaceResourceKey(
  assetKind: ConversationAssetWorkspaceResource['assetKind'],
  locator: AcpAttemptWorkspaceLocator,
  name: string,
  path?: string | null,
) {
  return [
    'conversation-asset',
    assetKind,
    locator.projectId,
    locator.taskUuid ?? 'missing-task-uuid',
    locator.runId,
    locator.roundId,
    locator.nodeId,
    locator.attemptId,
    locator.outerNodeId ?? '',
    locator.outerAttemptId ?? '',
    locator.branchId,
    path ?? name,
  ].join(':');
}

export function hiddenPromptSectionWorkspaceResourceKey(locator: HiddenPromptSectionWorkspaceLocator) {
  return [
    'hidden-prompt-section',
    locator.projectId,
    locator.taskUuid ?? 'missing-task-uuid',
    locator.runId,
    locator.roundId,
    locator.nodeId,
    locator.attemptId,
    locator.outerNodeId ?? '',
    locator.outerAttemptId ?? '',
    locator.branchId,
    locator.eventId,
    locator.eventSeq,
    locator.partIndex,
  ].join(':');
}

export function createHiddenPromptSectionWorkspaceResource(input: {
  scopeKey: string;
  title: string;
  locator: AcpAttemptWorkspaceLocator;
  eventId: string;
  eventSeq: number;
  partIndex: number;
}): HiddenPromptSectionWorkspaceResource {
  const locator: HiddenPromptSectionWorkspaceLocator = {
    ...input.locator,
    eventId: input.eventId,
    eventSeq: input.eventSeq,
    partIndex: input.partIndex,
  };
  return {
    kind: 'hidden-prompt-section',
    key: hiddenPromptSectionWorkspaceResourceKey(locator),
    scopeKey: input.scopeKey,
    title: input.title,
    description: null,
    attention: false,
    locator,
  };
}

export function draftAttachmentWorkspaceResourceKey(scopeKey: string, attachmentId: string) {
  return ['draft-attachment', scopeKey, attachmentId].join(':');
}

export function createDraftAttachmentWorkspaceResource(input: {
  scopeKey: string;
  projectId: string;
  attachment: AttachmentItem;
}): DraftAttachmentWorkspaceResource {
  return {
    kind: 'draft-attachment',
    key: draftAttachmentWorkspaceResourceKey(input.scopeKey, input.attachment.id),
    scopeKey: input.scopeKey,
    projectId: input.projectId,
    title: input.attachment.name,
    description: input.attachment.path ?? null,
    attention: false,
    attachment: input.attachment,
  };
}
