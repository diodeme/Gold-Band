import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, useRef, useState, type ReactNode } from 'react';
import { BoundedLruCache } from '@/lib/bounded-lru-cache';
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

export type AgentTranscriptResource = RightWorkspaceResourceBase & {
  kind: 'agent-transcript';
  status: string;
  locator: AgentTranscriptLocator;
};

export type FileWorkspaceResource = RightWorkspaceResourceBase & {
  kind: 'file';
  path: string;
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

export type RightWorkspaceResource =
  | AgentTranscriptResource
  | FileWorkspaceResource
  | WorkflowViewWorkspaceResource
  | WorkflowEditWorkspaceResource
  | SystemPromptWorkspaceResource
  | RawFramesWorkspaceResource;

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
  openResource: (resource: RightWorkspaceResource) => void;
  openWorkspace: () => void;
  activateTab: (key: string) => void;
  closeTab: (key: string) => void;
  closeWorkspace: () => void;
  setWidth: (width: number) => void;
  renderResource: (resource: RightWorkspaceResource) => ReactNode;
  registerResourceRenderer: (renderer: RightWorkspaceResourceRenderer) => () => void;
  registerResourceCloseResolver: (resolver: RightWorkspaceResourceCloseResolver) => () => void;
}

export type RightWorkspaceResourceRenderer = (resource: RightWorkspaceResource) => ReactNode;
export type RightWorkspaceResourceCloseResolver = (resource: RightWorkspaceResource) => boolean;

export type RightWorkspaceAction =
  | { type: 'open'; resource: RightWorkspaceResource }
  | { type: 'open-workspace' }
  | { type: 'activate'; key: string }
  | { type: 'close'; key: string }
  | { type: 'close-workspace' };

export const DEFAULT_RIGHT_WORKSPACE_WIDTH = RIGHT_WORKSPACE_DEFAULT_WIDTH;
export const CONVERSATION_WORKSPACE_LRU_LIMIT = 24;
const RightWorkspaceContext = createContext<RightWorkspaceContextValue | null>(null);

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
      const existing = state.tabs.findIndex((tab) => tab.key === action.resource.key);
      const tabs = existing < 0
        ? [...state.tabs, action.resource]
        : state.tabs.map((tab, index) => index === existing ? action.resource : tab);
      return {
        ...state,
        tabs,
        activeTabKey: action.resource.key,
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
      return { ...state, tabs, activeTabKey };
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
  const widthTouchedRef = useRef(false);
  const [width, setWidthState] = useState(initialWidth ?? DEFAULT_RIGHT_WORKSPACE_WIDTH);
  const [revision, render] = useReducer((currentRevision) => currentRevision + 1, 0);
  const rendererRef = useRef<RightWorkspaceResourceRenderer | null>(null);
  const closeResolverRef = useRef<RightWorkspaceResourceCloseResolver | null>(null);
  const [rendererRevision, renderRenderer] = useReducer((currentRevision) => currentRevision + 1, 0);
  const sessionState = useMemo(
    () => scope ? effectiveStore.peek(scope) : createInitialRightWorkspaceState(),
    [effectiveStore, revision, scope],
  );

  useEffect(() => {
    if (scope) effectiveStore.touch(scope);
  }, [effectiveStore, scope]);

  useEffect(() => {
    if (widthTouchedRef.current || initialWidth == null) return;
    setWidthState(initialWidth);
  }, [initialWidth]);

  const commit = useCallback((action: RightWorkspaceAction) => {
    if (!scope) return;
    const current = effectiveStore.peek(scope);
    if (action.type === 'open' && action.resource.scopeKey !== scope.key) return;
    effectiveStore.save(scope, rightWorkspaceReducer(current, action));
    render();
  }, [effectiveStore, scope]);
  const openResource = useCallback((resource: RightWorkspaceResource) => commit({ type: 'open', resource }), [commit]);
  const openWorkspace = useCallback(() => commit({ type: 'open-workspace' }), [commit]);
  const activateTab = useCallback((key: string) => commit({ type: 'activate', key }), [commit]);
  const closeTab = useCallback((key: string) => {
    if (!scope) return;
    const resource = effectiveStore.peek(scope).tabs.find((tab) => tab.key === key);
    if (resource && closeResolverRef.current?.(resource) === false) return;
    commit({ type: 'close', key });
  }, [commit, effectiveStore, scope]);
  const closeWorkspace = useCallback(() => commit({ type: 'close-workspace' }), [commit]);
  const setWidth = useCallback((nextWidth: number) => {
    widthTouchedRef.current = true;
    setWidthState(nextWidth);
  }, []);
  const renderResource = useCallback((resource: RightWorkspaceResource) => rendererRef.current?.(resource) ?? null, []);
  const registerResourceRenderer = useCallback((renderer: RightWorkspaceResourceRenderer) => {
    rendererRef.current = renderer;
    renderRenderer();
    return () => {
      if (rendererRef.current !== renderer) return;
      rendererRef.current = null;
      renderRenderer();
    };
  }, []);
  const registerResourceCloseResolver = useCallback((resolver: RightWorkspaceResourceCloseResolver) => {
    closeResolverRef.current = resolver;
    return () => {
      if (closeResolverRef.current === resolver) closeResolverRef.current = null;
    };
  }, []);
  const value = useMemo(() => ({
    ...sessionState,
    scopeKey: scope?.key ?? null,
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
  }), [activateTab, closeTab, closeWorkspace, openResource, openWorkspace, registerResourceCloseResolver, registerResourceRenderer, renderResource, rendererRevision, scope?.key, sessionState, setWidth, width]);
  return <RightWorkspaceContext.Provider value={value}>{children}</RightWorkspaceContext.Provider>;
}

export function useRightWorkspace() {
  const value = useContext(RightWorkspaceContext);
  if (!value) throw new Error('useRightWorkspace must be used inside RightWorkspaceProvider');
  return value;
}

export function useOptionalRightWorkspace() {
  return useContext(RightWorkspaceContext);
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
