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
  unwrapSelectedSlashCommand,
} from '@/lib/slash-command';

interface UseSlashCommandControllerOptions {
  input: string;
  commands: readonly AcpCommandItemVm[];
  contextKey?: string | null;
  onInputChange: (value: string) => void;
  onInputFocusRequested?: () => void;
}

export function useSlashCommandController({
  input,
  commands,
  contextKey,
  onInputChange,
  onInputFocusRequested,
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
    onInputFocusRequested?.();
    return true;
  }, [filteredCommands, onInputChange, onInputFocusRequested]);

  const dismiss = useCallback(() => {
    rememberSlashCommandDismissal(contextKey, input);
    setDismissed(true);
  }, [contextKey, input]);

  const onKeyDown = useCallback((event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Backspace' && !event.nativeEvent.isComposing) {
      const unwrappedInput = unwrapSelectedSlashCommand(
        input,
        commands,
        event.currentTarget.selectionStart,
        event.currentTarget.selectionEnd,
      );
      if (unwrappedInput !== null) {
        event.preventDefault();
        onInputChange(unwrappedInput);
        setDismissed(false);
        onInputFocusRequested?.();
        return true;
      }
    }
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
  }, [activeIndex, commands, dismiss, filteredCommands.length, input, isOpen, onInputChange, onInputFocusRequested, selectByIndex]);

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
