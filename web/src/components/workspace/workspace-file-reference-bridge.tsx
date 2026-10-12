import {
  createContext,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

import type { ComposerQuoteDraft, ComposerWorkspaceFileRef } from '@/lib/composer-context';

export type AddWorkspaceFileRefCommand = (
  reference: ComposerWorkspaceFileRef,
  options: { isDocked: boolean },
) => AddWorkspaceFileRefResult;

export type AddWorkspaceFileRefResult =
  | { kind: 'added' }
  | { kind: 'duplicate' }
  | { kind: 'limit-exceeded'; max: number }
  | { kind: 'unavailable' };

/** The composer reports a rejected quote (budget, duplicate) in its own error area. */
export type AddQuoteCommand = (
  quote: ComposerQuoteDraft,
  options: { isDocked: boolean },
) => AddQuoteResult;

export type AddQuoteResult = { kind: 'added' } | { kind: 'rejected' } | { kind: 'unavailable' };

/** The active composer, which receives workspace references and quotes taken outside it. */
export interface ComposerReferenceTarget {
  addWorkspaceFileRef: AddWorkspaceFileRefCommand;
  addQuote: AddQuoteCommand;
}

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
  register: (target: ComposerReferenceTarget | null) => () => void;
}

interface WorkspaceFileReferenceCommandsValue {
  available: boolean;
  addWorkspaceFileRef: AddWorkspaceFileRefCommand;
  addQuote: AddQuoteCommand;
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

/** Sends a quote to the active composer, or `null` while no composer can take one. */
export function useComposerQuoteCommand() {
  const commands = useWorkspaceFileReferenceCommands();
  const { isDocked } = useWorkspaceFileReferencePresentation();
  return useMemo(
    () => commands?.available
      ? (quote: ComposerQuoteDraft) => commands.addQuote(quote, { isDocked })
      : null,
    [commands, isDocked],
  );
}

export function useWorkspaceFileReferenceBridgeState() {
  const [available, setAvailable] = useState(false);
  const targetRef = useMemo(() => ({ current: null as ComposerReferenceTarget | null }), []);
  const register = useMemo(() => (target: ComposerReferenceTarget | null) => {
    targetRef.current = target;
    setAvailable(target !== null);
    return () => {
      if (targetRef.current !== target) return;
      targetRef.current = null;
      setAvailable(false);
    };
  }, [targetRef]);
  const commands = useMemo<WorkspaceFileReferenceCommandsValue>(() => ({
    available,
    addWorkspaceFileRef: (reference, options) =>
      targetRef.current?.addWorkspaceFileRef(reference, options) ?? { kind: 'unavailable' },
    addQuote: (quote, options) =>
      targetRef.current?.addQuote(quote, options) ?? { kind: 'unavailable' },
  }), [available, targetRef]);
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
