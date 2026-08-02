import { createContext, useCallback, useContext, useMemo, useReducer, type ReactNode } from 'react';

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

interface RightWorkspaceResourceBase {
  key: string;
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

export type RightWorkspaceResource = AgentTranscriptResource | FileWorkspaceResource;

export interface RightWorkspaceState {
  tabs: RightWorkspaceResource[];
  activeTabKey: string | null;
  requestedOpen: boolean;
  width: number;
  openRevision: number;
}

interface RightWorkspaceContextValue extends RightWorkspaceState {
  openResource: (resource: RightWorkspaceResource) => void;
  activateTab: (key: string) => void;
  closeTab: (key: string) => void;
  closeWorkspace: () => void;
  setWidth: (width: number) => void;
}

export type RightWorkspaceAction =
  | { type: 'open'; resource: RightWorkspaceResource }
  | { type: 'activate'; key: string }
  | { type: 'close'; key: string }
  | { type: 'close-workspace' }
  | { type: 'set-width'; width: number };

export const DEFAULT_RIGHT_WORKSPACE_WIDTH = 440;
const RightWorkspaceContext = createContext<RightWorkspaceContextValue | null>(null);

export function createInitialRightWorkspaceState(width = DEFAULT_RIGHT_WORKSPACE_WIDTH): RightWorkspaceState {
  return { tabs: [], activeTabKey: null, requestedOpen: false, width, openRevision: 0 };
}

export function rightWorkspaceReducer(state: RightWorkspaceState, action: RightWorkspaceAction): RightWorkspaceState {
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
  }
}

export function RightWorkspaceProvider({ initialWidth, children }: { initialWidth?: number; children: ReactNode }) {
  const [state, dispatch] = useReducer(
    rightWorkspaceReducer,
    initialWidth ?? DEFAULT_RIGHT_WORKSPACE_WIDTH,
    createInitialRightWorkspaceState,
  );
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
