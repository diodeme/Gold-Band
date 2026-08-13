import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, useRef, useState, type ReactNode } from 'react';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
import type { AttachmentItem } from '@/lib/attachment-service';
import { RIGHT_WORKSPACE_DEFAULT_WIDTH } from './workspace-layout';

export interface AgentTranscriptLocator {
  projectId: string;
  taskId: string;
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
  | GitFileComparisonWorkspaceResource
  | SourceControlWorkspaceResource
  | ConversationAssetWorkspaceResource
  | DraftAttachmentWorkspaceResource
  | WorkflowViewWorkspaceResource
  | WorkflowEditWorkspaceResource
  | SystemPromptWorkspaceResource
  | RawFramesWorkspaceResource
  | ScheduledTaskConfigWorkspaceResource;

export interface RightWorkspaceSessionState {
  tabs: RightWorkspaceResource[];
  activeTabKey: string | null;
  requestedOpen: boolean;
  openRevision: number;
}

export interface RightWorkspaceState extends RightWorkspaceSessionState {
  scopeKey: string | null;
  width: number;
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
  | { type: 'open-workspace' }
  | { type: 'activate'; key: string }
  | { type: 'close'; key: string }
  | { type: 'close-workspace' };

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
    key: `conversation:${input.projectId}:${input.taskId}:${input.runId}`,
    ...input,
  };
}

export function createInitialRightWorkspaceState(): RightWorkspaceSessionState {
  return { tabs: [], activeTabKey: null, requestedOpen: false, openRevision: 0 };
}

function cloneRightWorkspaceState(state: RightWorkspaceSessionState): RightWorkspaceSessionState {
  return { ...state, tabs: [...state.tabs] };
}

interface StoredConversationWorkspace {
  scope: ConversationWorkspaceScope;
  state: RightWorkspaceSessionState;
}

export class ConversationWorkspaceStore {
  private readonly entries = new BoundedLruCache<string, StoredConversationWorkspace>(CONVERSATION_WORKSPACE_LRU_LIMIT);

  restore(scope: ConversationWorkspaceScope) {
    const stored = this.entries.get(scope.key);
    return stored ? cloneRightWorkspaceState(stored.state) : createInitialRightWorkspaceState();
  }

  peek(scope: ConversationWorkspaceScope) {
    const stored = this.entries.peek(scope.key);
    return stored ? cloneRightWorkspaceState(stored.state) : createInitialRightWorkspaceState();
  }

  save(scope: ConversationWorkspaceScope, state: RightWorkspaceSessionState) {
    this.entries.set(scope.key, { scope, state: cloneRightWorkspaceState(state) });
  }

  touch(scope: ConversationWorkspaceScope) {
    this.entries.get(scope.key);
  }

  promoteDraft(draft: ConversationWorkspaceScope, conversation: ConversationWorkspaceScope) {
    if (draft.kind !== 'draft' || conversation.kind !== 'conversation') return;
    const previous = this.entries.get(draft.key);
    this.entries.delete(draft.key);
    if (!previous) return;
    this.entries.set(conversation.key, {
      scope: conversation,
      state: {
        ...createInitialRightWorkspaceState(),
        requestedOpen: previous.state.requestedOpen,
        openRevision: previous.state.openRevision,
      },
    });
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
        requestedOpen: true,
        openRevision: state.openRevision + 1,
      };
    }
    case 'open-workspace':
      return {
        ...state,
        requestedOpen: true,
        openRevision: state.openRevision + 1,
      };
    case 'activate':
      return state.tabs.some((tab) => tab.key === action.key)
        ? { ...state, activeTabKey: action.key, requestedOpen: true }
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
        requestedOpen: tabs.length > 0 && state.requestedOpen,
      };
    }
    case 'close-workspace':
      return { ...state, requestedOpen: false };
  }
}

const DEFAULT_SCOPE = createDraftConversationWorkspaceScope('default');

export function RightWorkspaceProvider({
  initialWidth,
  scope = DEFAULT_SCOPE,
  store,
  children,
}: {
  initialWidth?: number;
  scope?: ConversationWorkspaceScope | null;
  store?: ConversationWorkspaceStore;
  children: ReactNode;
}) {
  const internalStoreRef = useRef<ConversationWorkspaceStore | null>(null);
  if (!internalStoreRef.current) internalStoreRef.current = new ConversationWorkspaceStore();
  const effectiveStore = store ?? internalStoreRef.current;
  const scopeRef = useRef(scope);
  scopeRef.current = scope;
  const widthTouchedRef = useRef(false);
  const [width, setWidthState] = useState(initialWidth ?? DEFAULT_RIGHT_WORKSPACE_WIDTH);
  const [conversationDirectoryEntry, setConversationDirectoryEntryState] = useState<ConversationDirectoryWorkspaceEntry | null>(null);
  const [revision, render] = useReducer((currentRevision) => currentRevision + 1, 0);
  const rendererRegistryRef = useRef(new Map<RightWorkspaceResourceKind, RightWorkspaceResourceRenderer>());
  const closeResolverRegistryRef = useRef(new Map<RightWorkspaceResourceKind, RightWorkspaceResourceCloseResolver>());
  const previousScopeRef = useRef<ConversationWorkspaceScope | null>(scope);
  const [rendererRevision, renderRenderer] = useReducer((currentRevision) => currentRevision + 1, 0);
  const sessionState = useMemo(
    () => scope ? effectiveStore.peek(scope) : createInitialRightWorkspaceState(),
    [effectiveStore, revision, scope],
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
    if (widthTouchedRef.current || initialWidth == null) return;
    setWidthState(initialWidth);
  }, [initialWidth]);

  const commit = useCallback((action: RightWorkspaceAction) => {
    const currentScope = scopeRef.current;
    if (!currentScope) return;
    const current = effectiveStore.peek(currentScope);
    if (action.type === 'open' && action.resource.scopeKey !== currentScope.key) return;
    effectiveStore.save(currentScope, rightWorkspaceReducer(current, action));
    render();
  }, [effectiveStore]);
  const openResource = useCallback(async (resource: RightWorkspaceResource) => {
    const currentScope = scopeRef.current;
    if (!currentScope) return;
    const current = effectiveStore.peek(currentScope);
    if (current.activeTabKey && current.activeTabKey !== resource.key) {
      const active = current.tabs.find((tab) => tab.key === current.activeTabKey);
      if (active && await closeResolverRegistryRef.current.get(active.kind)?.(active, 'deactivate') === false) return;
    }
    commit({ type: 'open', resource });
  }, [commit, effectiveStore]);
  const getResource = useCallback((key: string) => {
    const currentScope = scopeRef.current;
    if (!currentScope) return null;
    return effectiveStore.peek(currentScope).tabs.find((tab) => tab.key === key) ?? null;
  }, [effectiveStore]);
  const openWorkspace = useCallback(() => commit({ type: 'open-workspace' }), [commit]);
  const activateTab = useCallback(async (key: string) => {
    if (!scope) return;
    const current = effectiveStore.peek(scope);
    if (current.activeTabKey && current.activeTabKey !== key) {
      const active = current.tabs.find((tab) => tab.key === current.activeTabKey);
      if (active && await closeResolverRegistryRef.current.get(active.kind)?.(active, 'deactivate') === false) return;
    }
    commit({ type: 'activate', key });
  }, [commit, effectiveStore, scope]);
  const closeTab = useCallback(async (key: string) => {
    if (!scope) return;
    const resource = effectiveStore.peek(scope).tabs.find((tab) => tab.key === key);
    if (resource && await closeResolverRegistryRef.current.get(resource.kind)?.(resource, 'close') === false) return;
    commit({ type: 'close', key });
  }, [commit, effectiveStore, scope]);
  const closeWorkspace = useCallback(async () => {
    if (!scope) return;
    const current = effectiveStore.peek(scope);
    const active = current.tabs.find((tab) => tab.key === current.activeTabKey);
    if (active && await closeResolverRegistryRef.current.get(active.kind)?.(active, 'workspace-close') === false) return;
    commit({ type: 'close-workspace' });
  }, [commit, effectiveStore, scope]);
  const setWidth = useCallback((nextWidth: number) => {
    widthTouchedRef.current = true;
    setWidthState(nextWidth);
  }, []);
  const setConversationDirectoryEntry = useCallback((entry: ConversationDirectoryWorkspaceEntry | null) => {
    setConversationDirectoryEntryState(entry);
  }, []);
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
    scopeKey: scope?.key ?? null,
    projectId: scope?.projectId ?? null,
    conversationDirectoryEntry: conversationDirectoryEntry?.scopeKey === scope?.key ? conversationDirectoryEntry : null,
    setConversationDirectoryEntry,
    width,
    openResource,
    openWorkspace,
    activateTab,
    closeTab,
    closeWorkspace,
    setWidth,
    renderResource,
    registerResourceRenderer,
    registerResourceCloseResolver,
  }), [activateTab, closeTab, closeWorkspace, conversationDirectoryEntry, openResource, openWorkspace, registerResourceCloseResolver, registerResourceRenderer, renderResource, rendererRevision, scope?.key, scope?.projectId, sessionState, setConversationDirectoryEntry, setWidth, width]);
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
    locator.taskId,
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
  return `${kind}:${locator.projectId}:${locator.taskId}:${locator.runId}`;
}

export function fileBrowserWorkspaceResourceKey(projectId: string) {
  return `file-browser:${projectId}`;
}

export function sourceControlWorkspaceResourceKey(projectId: string, workspacePath?: string | null) {
  const normalizedPath = workspacePath?.replaceAll('\\', '/');
  return `source-control:${projectId}:${normalizedPath ?? 'main'}`;
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
  return ['conversation-directory', locator.projectId, locator.taskId, locator.runId, locator.roundId, locator.outerNodeId ?? '', locator.outerAttemptId ?? '', locator.nodeId, locator.attemptId].join(':');
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
    locator.taskId,
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
    locator.taskId,
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

export function draftAttachmentWorkspaceResourceKey(scopeKey: string, attachmentId: string) {
  return ['draft-attachment', scopeKey, attachmentId].join(':');
}
