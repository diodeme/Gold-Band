import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, type ReactNode } from 'react';
import { conversationEventBranchId, conversationEventMatchesAttempt, subscribeConversationEvents } from '@/lib/conversation-event-router';

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

export type RightWorkspaceResource = {
  kind: 'agent-transcript';
  key: string;
  title: string;
  description?: string | null;
  status: string;
  attention: boolean;
  dirtyRevision: number;
  locator: AgentTranscriptLocator;
};

export interface RightWorkspaceState {
  tabs: RightWorkspaceResource[];
  activeTabKey: string | null;
  requestedOpen: boolean;
  width: number;
}

interface RightWorkspaceContextValue extends RightWorkspaceState {
  openResource: (resource: RightWorkspaceResource) => void;
  activateTab: (key: string) => void;
  closeTab: (key: string) => void;
  closeWorkspace: () => void;
  setWidth: (width: number) => void;
}

type Action =
  | { type: 'open'; resource: RightWorkspaceResource }
  | { type: 'activate'; key: string }
  | { type: 'close'; key: string }
  | { type: 'close-workspace' }
  | { type: 'set-width'; width: number }
  | { type: 'live-event'; event: Parameters<typeof conversationEventBranchId>[0] };

const DEFAULT_RIGHT_WORKSPACE_WIDTH = 440;
const RightWorkspaceContext = createContext<RightWorkspaceContextValue | null>(null);

function reducer(state: RightWorkspaceState, action: Action): RightWorkspaceState {
  switch (action.type) {
    case 'open': {
      const existing = state.tabs.findIndex((tab) => tab.key === action.resource.key);
      const tabs = existing < 0
        ? [...state.tabs, action.resource]
        : state.tabs.map((tab, index) => index === existing ? { ...tab, ...action.resource } : tab);
      return { ...state, tabs, activeTabKey: action.resource.key, requestedOpen: true };
    }
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
      return { ...state, tabs, activeTabKey, requestedOpen: tabs.length > 0 && state.requestedOpen };
    }
    case 'close-workspace':
      return { ...state, requestedOpen: false };
    case 'set-width':
      return { ...state, width: action.width };
    case 'live-event': {
      const eventBranchId = conversationEventBranchId(action.event);
      let changed = false;
      const tabs = state.tabs.map((tab) => {
        if (!conversationEventMatchesAttempt(action.event, tab.locator)) return tab;
        if (action.event.event && eventBranchId !== tab.locator.branchId) return tab;
        changed = true;
        const pendingAttention = action.event.event
          && (action.event.event.kind === 'permissionRequest' || action.event.event.kind === 'elicitationRequest')
          && (action.event.event.status ?? 'pending') === 'pending';
        const resolvedAttention = action.event.event
          && (action.event.event.kind === 'permissionRequest' || action.event.event.kind === 'elicitationResponse')
          && action.event.event.status !== 'pending';
        return {
          ...tab,
          status: action.event.session?.status ?? (pendingAttention ? 'waiting_permission' : tab.status),
          attention: pendingAttention ? true : resolvedAttention ? false : tab.attention,
          dirtyRevision: tab.dirtyRevision + 1,
        };
      });
      return changed ? { ...state, tabs } : state;
    }
  }
}

export function RightWorkspaceProvider({ initialWidth, children }: { initialWidth?: number; children: ReactNode }) {
  const [state, dispatch] = useReducer(reducer, {
    tabs: [],
    activeTabKey: null,
    requestedOpen: false,
    width: initialWidth ?? DEFAULT_RIGHT_WORKSPACE_WIDTH,
  });
  useEffect(() => {
    const unsubscribe = subscribeConversationEvents((event) => dispatch({ type: 'live-event', event }));
    return () => { unsubscribe(); };
  }, []);
  const openResource = useCallback((resource: RightWorkspaceResource) => dispatch({ type: 'open', resource }), []);
  const activateTab = useCallback((key: string) => dispatch({ type: 'activate', key }), []);
  const closeTab = useCallback((key: string) => dispatch({ type: 'close', key }), []);
  const closeWorkspace = useCallback(() => dispatch({ type: 'close-workspace' }), []);
  const setWidth = useCallback((width: number) => dispatch({ type: 'set-width', width }), []);
  const value = useMemo(() => ({
    ...state,
    openResource,
    activateTab,
    closeTab,
    closeWorkspace,
    setWidth,
  }), [activateTab, closeTab, closeWorkspace, openResource, setWidth, state]);
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
