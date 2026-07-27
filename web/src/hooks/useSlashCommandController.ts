import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import type { AcpCommandItemVm } from '@/types';
import {
  filterSlashCommands,
  clearSlashCommandDismissal,
  matchSlashCommandQuery,
  rememberSlashCommandDismissal,
  restoreSlashCommandDismissal,
  slashCommandText,
} from '@/lib/slash-command';

interface UseSlashCommandControllerOptions {
  input: string;
  commands: readonly AcpCommandItemVm[];
  contextKey?: string | null;
  onInputChange: (value: string) => void;
}

export function useSlashCommandController({
  input,
  commands,
  contextKey,
  onInputChange,
}: UseSlashCommandControllerOptions) {
  const query = useMemo(() => matchSlashCommandQuery(input), [input]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [dismissed, setDismissed] = useState(() => (
    restoreSlashCommandDismissal(contextKey, input, query !== null)
  ));
  const previousContextKey = useRef(contextKey);

  useEffect(() => {
    setActiveIndex(0);
    if (previousContextKey.current !== contextKey) {
      previousContextKey.current = contextKey;
      clearSlashCommandDismissal(contextKey);
      setDismissed(false);
      return;
    }
    setDismissed(restoreSlashCommandDismissal(contextKey, input, query !== null));
  }, [contextKey, input, query]);

  const filteredCommands = useMemo(
    () => (query === null ? [] : filterSlashCommands(commands, query)),
    [commands, query],
  );
  const isOpen = query !== null && !dismissed && filteredCommands.length > 0;

  const selectByIndex = useCallback((index: number) => {
    const command = filteredCommands[index];
    if (!command) return false;
    onInputChange(slashCommandText(command.name));
    setDismissed(true);
    return true;
  }, [filteredCommands, onInputChange]);

  const dismiss = useCallback(() => {
    rememberSlashCommandDismissal(contextKey, input);
    setDismissed(true);
  }, [contextKey, input]);

  const onKeyDown = useCallback((event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (!isOpen) return false;
    if (event.key === 'Escape') {
      event.preventDefault();
      dismiss();
      return true;
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      setActiveIndex((index) => (index + 1) % filteredCommands.length);
      return true;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      setActiveIndex((index) => (index - 1 + filteredCommands.length) % filteredCommands.length);
      return true;
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      return selectByIndex(activeIndex);
    }
    return false;
  }, [activeIndex, dismiss, filteredCommands.length, isOpen, selectByIndex]);

  return {
    activeIndex,
    filteredCommands,
    isOpen,
    onKeyDown,
    selectByIndex,
    dismiss,
    setActiveIndex,
  };
}
