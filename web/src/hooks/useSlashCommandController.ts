import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import type { AcpCommandItemVm } from '@/types';
import {
  type SlashCatalogGroup,
  type SlashCatalogItem,
  type SlashItemIdentity,
  clearSlashCommandDismissal,
  commandSlashItems,
  filterSlashCatalog,
  flattenSlashCatalog,
  matchSlashCommandQuery,
  rememberSlashCommandDismissal,
  restoreSlashCommandDismissal,
  slashCommandText,
  unwrapSelectedSlashItem,
} from '@/lib/slash-command';

interface UseSlashCommandControllerOptions {
  input: string;
  groups?: readonly SlashCatalogGroup[];
  commands?: readonly AcpCommandItemVm[];
  contextKey?: string | null;
  onInputChange: (value: string) => void;
  onInputFocusRequested?: () => void;
}

function catalogGroups(
  groups: readonly SlashCatalogGroup[] | undefined,
  commands: readonly AcpCommandItemVm[] | undefined,
): SlashCatalogGroup[] {
  if (groups) return [...groups];
  const items = commandSlashItems(commands ?? []);
  return items.length > 0 ? [{ id: 'agent', heading: 'Agent', items }] : [];
}

export function useSlashCommandController({
  input,
  groups,
  commands,
  contextKey,
  onInputChange,
  onInputFocusRequested,
}: UseSlashCommandControllerOptions) {
  const catalog = useMemo(() => catalogGroups(groups, commands), [commands, groups]);
  const catalogItems = useMemo(() => flattenSlashCatalog(catalog), [catalog]);
  const query = useMemo(() => matchSlashCommandQuery(input), [input]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [selectedIdentity, setSelectedIdentity] = useState<SlashItemIdentity | null>(null);
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
      setSelectedIdentity(null);
      return;
    }
    setDismissed(restoreSlashCommandDismissal(contextKey, input, query !== null));
  }, [contextKey, input, query]);

  const filteredGroups = useMemo(
    () => (query === null ? [] : filterSlashCatalog(catalog, query)),
    [catalog, query],
  );
  const filteredItems = useMemo(() => flattenSlashCatalog(filteredGroups), [filteredGroups]);
  const isOpen = query !== null && !dismissed && filteredItems.length > 0;

  const selectByIndex = useCallback((index: number) => {
    const item = filteredItems[index];
    if (!item) return false;
    setSelectedIdentity({ kind: item.kind, id: item.id });
    onInputChange(slashCommandText(item.name));
    setDismissed(true);
    onInputFocusRequested?.();
    return true;
  }, [filteredItems, onInputChange, onInputFocusRequested]);

  const dismiss = useCallback(() => {
    rememberSlashCommandDismissal(contextKey, input);
    setDismissed(true);
  }, [contextKey, input]);

  const onKeyDown = useCallback((event: ReactKeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Backspace' && !event.nativeEvent.isComposing) {
      const unwrappedInput = unwrapSelectedSlashItem(
        input,
        catalogItems,
        event.currentTarget.selectionStart,
        event.currentTarget.selectionEnd,
        selectedIdentity,
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
      setActiveIndex((index) => (index + 1) % filteredItems.length);
      return true;
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault();
      setActiveIndex((index) => (index - 1 + filteredItems.length) % filteredItems.length);
      return true;
    }
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault();
      return selectByIndex(activeIndex);
    }
    return false;
  }, [
    activeIndex,
    catalogItems,
    dismiss,
    filteredItems.length,
    input,
    isOpen,
    onInputChange,
    onInputFocusRequested,
    selectByIndex,
    selectedIdentity,
  ]);

  return {
    activeIndex,
    filteredGroups,
    filteredItems,
    filteredCommands: filteredItems,
    selectedIdentity,
    catalogItems,
    isOpen,
    onKeyDown,
    selectByIndex,
    dismiss,
    setActiveIndex,
  };
}
