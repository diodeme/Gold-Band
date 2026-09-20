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
) => boolean;

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
    addWorkspaceFileRef: (reference) => commandRef.current?.(reference) ?? false,
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
