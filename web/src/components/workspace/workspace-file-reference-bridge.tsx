import {
  createContext,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

import type { ComposerWorkspaceFileRef } from '@/lib/composer-context';

export type AddWorkspaceFileRefCommand = (
  reference: ComposerWorkspaceFileRef,
  options: { isDocked: boolean },
) => AddWorkspaceFileRefResult;

export type AddWorkspaceFileRefResult =
  | { kind: 'added' }
  | { kind: 'duplicate' }
  | { kind: 'limit-exceeded'; max: number }
  | { kind: 'unavailable' };

export interface WorkspaceFileReferencePresentation {
  isDocked: boolean;
}

const WorkspaceFileReferencePresentationContext =
  createContext<WorkspaceFileReferencePresentation>({ isDocked: false });

export function useWorkspaceFileReferencePresentation() {
  return useContext(WorkspaceFileReferencePresentationContext);
}

export function WorkspaceFileReferencePresentationProvider({
  value,
  children,
}: {
  value: WorkspaceFileReferencePresentation;
  children: ReactNode;
}) {
  return (
    <WorkspaceFileReferencePresentationContext.Provider value={value}>
      {children}
    </WorkspaceFileReferencePresentationContext.Provider>
  );
}

interface WorkspaceFileReferenceBridgeValue {
  register: (command: AddWorkspaceFileRefCommand | null) => () => void;
}

interface WorkspaceFileReferenceCommandsValue {
  available: boolean;
  addWorkspaceFileRef: AddWorkspaceFileRefCommand;
}

const BridgeContext = createContext<WorkspaceFileReferenceBridgeValue | null>(null);
const CommandsContext = createContext<WorkspaceFileReferenceCommandsValue | null>(null);

export function useWorkspaceFileReferenceBridge() {
  const value = useContext(BridgeContext);
  if (!value) {
    throw new Error('useWorkspaceFileReferenceBridge must be used inside RightWorkspaceProvider');
  }
  return value;
}

export function useWorkspaceFileReferenceCommands() {
  return useContext(CommandsContext);
}

export function useWorkspaceFileReferenceBridgeState() {
  const [available, setAvailable] = useState(false);
  const commandRef = useMemo(() => ({ current: null as AddWorkspaceFileRefCommand | null }), []);
  const register = useMemo(() => (command: AddWorkspaceFileRefCommand | null) => {
    commandRef.current = command;
    setAvailable(command !== null);
    return () => {
      if (commandRef.current !== command) return;
      commandRef.current = null;
      setAvailable(false);
    };
  }, [commandRef]);
  const commands = useMemo<WorkspaceFileReferenceCommandsValue>(() => ({
    available,
    addWorkspaceFileRef: (reference, options) =>
      commandRef.current?.(reference, options) ?? { kind: 'unavailable' },
  }), [available, commandRef]);
  const bridge = useMemo<WorkspaceFileReferenceBridgeValue>(() => ({ register }), [register]);
  return { bridge, commands };
}

export function WorkspaceFileReferenceBridgeProvider({
  bridge,
  commands,
  children,
}: {
  bridge: WorkspaceFileReferenceBridgeValue;
  commands: WorkspaceFileReferenceCommandsValue;
  children: ReactNode;
}) {
  return (
    <BridgeContext.Provider value={bridge}>
      <CommandsContext.Provider value={commands}>{children}</CommandsContext.Provider>
    </BridgeContext.Provider>
  );
}
