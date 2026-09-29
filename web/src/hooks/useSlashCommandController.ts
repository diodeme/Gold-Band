import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent, RefObject } from 'react';
import type { AcpCommandItemVm, WorkspaceDirectoryEntryVm, WorkspaceRootRef } from '@/types';
import { useMentionWorkspaceFiles } from '@/hooks/useMentionWorkspaceFiles';
import {
  type MentionMenuLabels,
  type MentionView,
  type SlashCatalogGroup,
  type SlashCatalogItem,
  type SlashItemIdentity,
  MENTION_ROOT_VIEW,
  buildMentionGroups,
  clearSlashCommandDismissal,
  mentionFilesRequest,
  parentMentionView,
  slashCatalogGroupsHaveContent,
  commandSlashItems,
  filterSlashCatalog,
  flattenSlashCatalog,
  groupsForComposerMenuTrigger,
  matchComposerMenuQueryAt,
  replaceComposerMenuQuery,
  rememberSlashCommandDismissal,
  restoreSlashCommandDismissal,
  composerTokenText,
  unwrapSelectedSlashItem,
} from '@/lib/slash-command';

interface UseSlashCommandControllerOptions {
  input: string;
  groups?: readonly SlashCatalogGroup[];
  commands?: readonly AcpCommandItemVm[];
  contextKey?: string | null;
  onInputChange: (value: string) => void;
  /**
   * The composer textarea. The menu query ends at its caret, and choosing an
   * item restores focus and the caret after the inserted text.
   */
  textareaRef?: RefObject<HTMLTextAreaElement | null>;
  /** Turns `@` into a categorized menu of workspace files and the catalog's roles. */
  mention?: {
    root: WorkspaceRootRef | null;
    labels: MentionMenuLabels;
    onSelectWorkspaceFile: (entry: WorkspaceDirectoryEntryVm) => void;
  };
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
  textareaRef,
  mention,
}: UseSlashCommandControllerOptions) {
  const catalog = useMemo(() => catalogGroups(groups, commands), [commands, groups]);
  const catalogItems = useMemo(() => flattenSlashCatalog(catalog), [catalog]);
  const readCaret = useCallback((): number | null => {
    const textarea = textareaRef?.current;
    if (!textarea) return input.length;
    // A committed tag hides its prefix, so the textarea shows only part of the input.
    if (textarea.value !== input) return null;
    return textarea.selectionStart === textarea.selectionEnd ? textarea.selectionStart : null;
  }, [input, textareaRef]);
  const [caret, setCaret] = useState<number | null>(input.length);
  const pendingCaretRef = useRef<number | null>(null);
  useLayoutEffect(() => {
    const textarea = textareaRef?.current;
    const pending = pendingCaretRef.current;
    if (textarea && pending !== null && textarea.value === input) {
      pendingCaretRef.current = null;
      textarea.setSelectionRange(pending, pending);
    }
    setCaret(readCaret());
  }, [input, readCaret, textareaRef]);
  useEffect(() => {
    const textarea = textareaRef?.current;
    if (!textarea) return undefined;
    const sync = () => setCaret(readCaret());
    const onDocumentSelection = () => {
      if (document.activeElement === textarea) sync();
    };
    document.addEventListener('selectionchange', onDocumentSelection);
    textarea.addEventListener('keyup', sync);
    textarea.addEventListener('pointerup', sync);
    return () => {
      document.removeEventListener('selectionchange', onDocumentSelection);
      textarea.removeEventListener('keyup', sync);
      textarea.removeEventListener('pointerup', sync);
    };
  }, [readCaret, textareaRef]);
  const menuQuery = useMemo(() => matchComposerMenuQueryAt(input, caret), [caret, input]);

  /** Commits the next input and puts the caret (and focus) at `nextCaret`. */
  const applyInput = useCallback((next: string, nextCaret: number) => {
    const textarea = textareaRef?.current;
    if (next !== input) {
      pendingCaretRef.current = nextCaret;
      onInputChange(next);
    } else if (textarea) {
      textarea.setSelectionRange(nextCaret, nextCaret);
      setCaret(nextCaret);
    }
    textarea?.focus();
  }, [input, onInputChange, textareaRef]);

  /** Replaces the typed menu query, keeping the text after the caret. */
  const replaceQuery = useCallback((replacement: string) => {
    const result = replaceComposerMenuQuery(input, caret ?? input.length, replacement);
    applyInput(result.input, result.caret);
  }, [applyInput, caret, input]);
  const [activeIndex, setActiveIndex] = useState(0);
  const [selectedIdentity, setSelectedIdentity] = useState<SlashItemIdentity | null>(null);
  const [dismissed, setDismissed] = useState(() => (
    restoreSlashCommandDismissal(contextKey, input, menuQuery !== null)
  ));
  const previousContextKey = useRef(contextKey);
  const [mentionView, setMentionView] = useState<MentionView>(MENTION_ROOT_VIEW);
  const mentionQuery = mention && menuQuery?.trigger === '@' && !dismissed ? menuQuery.query : null;
  const mentionFiles = useMentionWorkspaceFiles(
    mention?.root,
    mentionQuery === null ? null : mentionFilesRequest(mentionView, mentionQuery),
  );

  useEffect(() => {
    setActiveIndex(0);
    if (menuQuery?.trigger !== '@') setMentionView(MENTION_ROOT_VIEW);
    if (previousContextKey.current !== contextKey) {
      previousContextKey.current = contextKey;
      clearSlashCommandDismissal(contextKey);
      setDismissed(false);
      setSelectedIdentity(null);
      return;
    }
    setDismissed(restoreSlashCommandDismissal(contextKey, input, menuQuery !== null));
  }, [contextKey, input, menuQuery]);

  const mentionLabels = mention?.labels;
  const filesAvailable = Boolean(mention?.root);
  const filteredGroups = useMemo(() => {
    if (!menuQuery) return [];
    const triggerGroups = groupsForComposerMenuTrigger(catalog, menuQuery.trigger);
    if (menuQuery.trigger === '@' && mentionLabels) {
      return buildMentionGroups({
        view: mentionView,
        query: menuQuery.query,
        roleItems: flattenSlashCatalog(triggerGroups),
        filesAvailable,
        files: mentionFiles,
        labels: mentionLabels,
      });
    }
    return filterSlashCatalog(triggerGroups, menuQuery.query);
  }, [catalog, filesAvailable, menuQuery, mentionFiles, mentionLabels, mentionView]);
  const filteredItems = useMemo(() => flattenSlashCatalog(filteredGroups), [filteredGroups]);
  const isOpen = menuQuery !== null && !dismissed && slashCatalogGroupsHaveContent(filteredGroups);
  // Asynchronous file results can shrink the list under the highlighted row.
  const activeItemIndex = activeIndex < filteredItems.length ? activeIndex : 0;

  const navigateMention = useCallback((view: MentionView) => {
    setMentionView(view);
    setActiveIndex(0);
    // A typed query searched across categories; opening a level starts browsing it.
    replaceQuery('@');
  }, [replaceQuery]);

  const selectByIndex = useCallback((index: number) => {
    const item = filteredItems[index];
    if (!item) return false;
    if (item.kind === 'mention-category') {
      navigateMention(item.id === 'roles' ? { kind: 'roles' } : { kind: 'files', path: '' });
      return true;
    }
    if (item.kind === 'workspace-directory') {
      navigateMention({ kind: 'files', path: item.id });
      return true;
    }
    if (item.kind === 'workspace-file') {
      if (item.workspaceEntry) mention?.onSelectWorkspaceFile(item.workspaceEntry);
      setMentionView(MENTION_ROOT_VIEW);
      replaceQuery('');
      return true;
    }
    setSelectedIdentity({ kind: item.kind, id: item.id });
    replaceQuery(composerTokenText(item));
    setDismissed(true);
    return true;
  }, [filteredItems, mention, navigateMention, replaceQuery]);

  const dismiss = useCallback(() => {
    rememberSlashCommandDismissal(contextKey, input);
    setDismissed(true);
    setMentionView(MENTION_ROOT_VIEW);
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
        applyInput(unwrappedInput, unwrappedInput.length);
        setDismissed(false);
        return true;
      }
    }
    if (!isOpen) return false;
    if (
      (event.key === 'Backspace' || event.key === 'ArrowLeft')
      && mentionView.kind !== 'root'
      && menuQuery?.trigger === '@'
      && menuQuery.query === ''
    ) {
      event.preventDefault();
      setMentionView(mentionView.kind === 'files' ? parentMentionView(mentionView) : MENTION_ROOT_VIEW);
      setActiveIndex(0);
      return true;
    }
    if (filteredItems.length === 0 && event.key !== 'Escape') {
      // A loading or empty level must not let Enter submit the bare `@`.
      if (event.key !== 'Enter' || event.shiftKey) return false;
      event.preventDefault();
      return true;
    }
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
      return selectByIndex(activeItemIndex);
    }
    return false;
  }, [
    activeItemIndex,
    catalogItems,
    dismiss,
    filteredItems.length,
    input,
    isOpen,
    mentionView,
    menuQuery,
    applyInput,
    selectByIndex,
    selectedIdentity,
  ]);

  return {
    activeIndex: activeItemIndex,
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
