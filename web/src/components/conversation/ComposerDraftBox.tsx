import {
  FileText,
  Inbox,
  Loader2,
  MessageSquareQuote,
  Paperclip,
  Pencil,
  Trash2,
  UserRound,
} from 'lucide-react';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import {
  deleteConversationComposerDraft,
  listConversationComposerDrafts,
  saveConversationComposerDraft,
  getConversationComposerDraft,
} from '@/api';
import { displayAppError } from '@/i18n';
import { formatCompactRelativeTime } from '@/lib/datetime';
import { cn } from '@/lib/utils';
import type {
  ConversationComposerDraftBoxVm,
  ConversationPromptInput,
  ConversationQueuedPromptDraftVm,
} from '@/types';

/** A snapshot retained in the composer until storage succeeds. */
export interface CapturedComposerDraft {
  input: ConversationPromptInput;
  attachmentPaths: string[];
  commit: () => void;
}

export interface ComposerDraftBoxProps {
  projectId: string | null | undefined;
  taskId: string;
  disabled: boolean;
  /** The composer holds a payload that can be saved. */
  canSave: boolean;
  /** The composer is not empty, so taking a draft would overwrite the user's input. */
  composerOccupied: boolean;
  captureDraft: () => Promise<CapturedComposerDraft | null>;
  /** Puts a taken draft into the empty composer; returns false when the composer filled up meanwhile. */
  restoreDraft: (draft: ConversationQueuedPromptDraftVm) => boolean;
  /** Acquires the composer transfer lock before any read/consume mutation. */
  acquireMutation?: () => boolean;
  onPendingChange?: (pending: boolean) => void;
}

export function ComposerDraftBox(props: ComposerDraftBoxProps) {
  return <ScopedComposerDraftBox key={JSON.stringify([props.projectId ?? null, props.taskId])} {...props} />;
}

function ScopedComposerDraftBox({
  projectId,
  taskId,
  disabled,
  canSave,
  composerOccupied,
  captureDraft,
  restoreDraft,
  acquireMutation,
  onPendingChange,
}: ComposerDraftBoxProps) {
  const { t } = useTranslation();
  const [box, setBox] = useState<ConversationComposerDraftBoxVm | null>(null);
  const [loadStatus, setLoadStatus] = useState<'idle' | 'loading' | 'ready' | 'error'>('idle');
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState<'save' | string | null>(null);
  const [deleteConfirmItemId, setDeleteConfirmItemId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const active = useRef(true);
  const listInFlight = useRef(false);
  const mutationInFlight = useRef(false);
  const mutationRevision = useRef(0);

  const beginPending = useCallback((next: 'save' | string) => {
    setPending(next);
    onPendingChange?.(true);
  }, [onPendingChange]);
  const endPending = useCallback(() => {
    setPending(null);
    onPendingChange?.(false);
  }, [onPendingChange]);

  const converge = useCallback((next: ConversationComposerDraftBoxVm) => {
    setBox((current) => current && current.revision > next.revision ? current : next);
    setLoadStatus('ready');
  }, []);

  useEffect(() => {
    active.current = true;
    return () => { active.current = false; };
  }, []);

  useEffect(() => () => onPendingChange?.(false), [onPendingChange]);

  const load = useCallback(async () => {
    if (listInFlight.current) return;
    listInFlight.current = true;
    const revision = mutationRevision.current;
    setLoadStatus('loading');
    setError(null);
    try {
      const next = await listConversationComposerDrafts(projectId, taskId);
      if (active.current && mutationRevision.current === revision) converge(next);
    } catch (loadError) {
      if (active.current && mutationRevision.current === revision) {
        setLoadStatus('error');
        setError(displayAppError(t, loadError));
      }
    } finally {
      listInFlight.current = false;
    }
  }, [projectId, taskId, converge, t]);

  useEffect(() => {
    if (open) void load();
  }, [open, load]);

  // The bounded summary (≤20 items × 280-char previews) is prefetched on mount
  // so the saved-draft count is visible before the box is ever opened; the
  // list body still only renders inside the popover.
  useEffect(() => {
    void load();
  }, [load]);

  const count = box?.items.length ?? 0;
  const maxItems = box?.maxItems ?? 0;
  const full = box != null && count >= maxItems;
  const busy = pending !== null;

  const save = async () => {
    if (disabled || mutationInFlight.current || !canSave || full) return;
    if (acquireMutation && !acquireMutation()) return;
    mutationInFlight.current = true;
    beginPending('save');
    setError(null);
    try {
      const captured = await captureDraft();
      if (!captured || !active.current) return;
      const next = await saveConversationComposerDraft(
        projectId,
        taskId,
        captured.input,
        captured.attachmentPaths.length > 0 ? captured.attachmentPaths : undefined,
      );
      if (!active.current) return;
      mutationRevision.current += 1;
      captured.commit();
      converge(next);
      setError(null);
      // Saving hands focus back to the emptied composer, so the box steps aside.
      setOpen(false);
    } catch (saveError) {
      if (active.current) setError(displayAppError(t, saveError));
    } finally {
      mutationInFlight.current = false;
      if (active.current) endPending();
    }
  };

  const take = async (draftId: string) => {
    if (disabled || mutationInFlight.current || composerOccupied) return;
    if (acquireMutation && !acquireMutation()) return;
    mutationInFlight.current = true;
    beginPending(draftId);
    setError(null);
    // Tracks whether the composer already accepted the payload: a failure
    // after that point must explain that the content was restored and the
    // stored copy survived, instead of a bare storage error.
    let restored = false;
    try {
      const result = await getConversationComposerDraft(projectId, taskId, draftId);
      if (!active.current) return;
      converge(result.drafts);
      if (!restoreDraft(result.draft)) {
        setError(t('acp.composerDrafts.takeConflict'));
        return;
      }
      restored = true;
      // Delete only after the composer has accepted the complete payload.
      const next = await deleteConversationComposerDraft(projectId, taskId, draftId);
      if (!active.current) return;
      mutationRevision.current += 1;
      converge(next);
      setError(null);
      setOpen(false);
    } catch (takeError) {
      if (active.current) {
        setError(restored
          ? t('acp.composerDrafts.takeDeleteKept')
          : displayAppError(t, takeError));
      }
    } finally {
      mutationInFlight.current = false;
      if (active.current) endPending();
    }
  };

  const remove = async (draftId: string) => {
    if (disabled || mutationInFlight.current) return;
    if (acquireMutation && !acquireMutation()) return;
    mutationInFlight.current = true;
    beginPending(draftId);
    setError(null);
    try {
      const next = await deleteConversationComposerDraft(projectId, taskId, draftId);
      if (!active.current) return;
      mutationRevision.current += 1;
      converge(next);
      setError(null);
    } catch (deleteError) {
      if (active.current) setError(displayAppError(t, deleteError));
    } finally {
      mutationInFlight.current = false;
      if (active.current) endPending();
    }
  };

  const requestRemove = (draftId: string) => {
    if (disabled || mutationInFlight.current) return;
    setDeleteConfirmItemId(draftId);
  };

  const label = t('acp.composerDrafts.title');
  return (
    <TooltipProvider delayDuration={250}>
    <Popover open={open} onOpenChange={setOpen}>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <Button
              type="button"
              className="relative size-7 rounded-full"
              size="icon"
              variant="ghost"
              disabled={disabled}
              aria-label={count > 0 ? t('acp.composerDrafts.titleWithCount', { count }) : label}
              data-composer-draft-box-trigger="true"
            >
              <Inbox className="size-3.5" />
              {count > 0 ? (
                <span
                  className="absolute -top-0.5 -right-0.5 flex h-3.5 min-w-3.5 items-center justify-center rounded-full bg-foreground px-1 text-[0.625rem] leading-none font-medium tabular-nums text-background"
                  data-composer-draft-count="true"
                >
                  {count}
                </span>
              ) : null}
            </Button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
      <PopoverContent
        align="start"
        side="top"
        className="w-80 max-w-[calc(100vw-1rem)] p-0"
        data-composer-draft-box="true"
      >
        <div className="flex items-center gap-2 px-3 py-2">
          <span className="text-sm font-medium">{label}</span>
          {box ? (
            <span className="text-ui-caption tabular-nums text-muted-foreground">
              {t('acp.composerDrafts.count', { count, max: maxItems })}
            </span>
          ) : null}
          <Button
            type="button"
            size="sm"
            variant="secondary"
            className="ml-auto h-7 gap-1.5 text-xs"
            disabled={busy || !canSave || full}
            onClick={() => { void save(); }}
            data-composer-draft-save="true"
          >
            {pending === 'save' ? <Loader2 className="size-3.5 animate-spin" /> : null}
            {t('acp.composerDrafts.save')}
          </Button>
        </div>
        {error ? (
          <div className="flex items-center gap-2 px-3 pb-2">
            <p className="text-ui-caption text-destructive" role="alert">{error}</p>
            {loadStatus === 'error' ? (
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => { void load(); }} data-composer-draft-retry="true">
                {t('common.retry')}
              </Button>
            ) : null}
          </div>
        ) : null}
        {loadStatus === 'loading' && !box ? (
          <p className="px-3 pb-3 text-xs text-muted-foreground" role="status" data-composer-draft-loading="true">
            {t('common.loading')}
          </p>
        ) : null}
        {box && count === 0 ? (
          <p className="px-3 pb-3 text-xs text-muted-foreground" data-composer-draft-empty="true">
            {t('acp.composerDrafts.empty')}
          </p>
        ) : count > 0 ? (
          <ul className="max-h-72 overflow-y-auto border-t border-border/60 py-1" data-composer-draft-list="true">
            {box?.items.map((draft) => (
              <li
                key={draft.id}
                className="flex min-w-0 items-start gap-1 px-3 py-1.5 text-xs"
                data-composer-draft-id={draft.id}
              >
                <div className="min-w-0 flex-1">
                  {draft.contentPreview.trim() ? (
                    <p className="line-clamp-2 whitespace-pre-wrap break-words leading-5 text-foreground/90 [overflow-wrap:anywhere]">
                      {draft.contentPreview}
                    </p>
                  ) : null}
                  <div className="mt-0.5 flex flex-wrap items-center gap-2 text-ui-caption text-muted-foreground">
                    {draft.roleName ? (
                      <DraftMeta icon={<UserRound className="size-3" />}>{draft.roleName}</DraftMeta>
                    ) : null}
                    {draft.quoteCount > 0 ? (
                      <DraftMeta icon={<MessageSquareQuote className="size-3" />}>
                        {t('acp.userQuoteCount', { count: draft.quoteCount })}
                      </DraftMeta>
                    ) : null}
                    {draft.attachmentCount > 0 ? (
                      <DraftMeta icon={<Paperclip className="size-3" />}>{draft.attachmentCount}</DraftMeta>
                    ) : null}
                    {draft.workspaceFileCount > 0 ? (
                      <DraftMeta icon={<FileText className="size-3" />}>{draft.workspaceFileCount}</DraftMeta>
                    ) : null}
                    <span>{formatCompactRelativeTime(draft.createdAt, t('conversation.runtime.justNow'))}</span>
                  </div>
                </div>
                <DraftAction
                  label={t('acp.composerDrafts.take')}
                  tooltipLabel={composerOccupied ? t('acp.composerDrafts.takeOccupied') : undefined}
                  disabled={busy || composerOccupied}
                  onClick={() => { void take(draft.id); }}
                >
                  {pending === draft.id ? <Loader2 className="size-3.5 animate-spin" /> : <Pencil className="size-3.5" />}
                </DraftAction>
                <DraftAction
                  label={t('acp.composerDrafts.delete')}
                  disabled={busy}
                  destructive
                  onClick={() => requestRemove(draft.id)}
                >
                  <Trash2 className="size-3.5" />
                </DraftAction>
              </li>
            ))}
          </ul>
        ) : null}
      </PopoverContent>
    </Popover>
    <AlertDialog
      open={deleteConfirmItemId !== null}
      onOpenChange={(nextOpen) => {
        if (!nextOpen) setDeleteConfirmItemId(null);
      }}
    >
      <AlertDialogContent data-composer-draft-delete-confirm="true">
        <AlertDialogHeader>
          <AlertDialogTitle>{t('acp.composerDrafts.deleteConfirmTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t('acp.composerDrafts.deleteConfirmDescription')}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel aria-label={t('common.cancel')}>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction
            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            aria-label={t('acp.composerDrafts.delete')}
            onClick={() => {
              const draftId = deleteConfirmItemId;
              setDeleteConfirmItemId(null);
              if (draftId) void remove(draftId);
            }}
          >
            {t('acp.composerDrafts.delete')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
    </TooltipProvider>
  );
}

function DraftMeta({ icon, children }: { icon: ReactNode; children: ReactNode }) {
  return <span className="inline-flex items-center gap-1">{icon}{children}</span>;
}

function DraftAction({
  label,
  tooltipLabel,
  disabled,
  destructive = false,
  onClick,
  children,
}: {
  label: string;
  tooltipLabel?: string;
  disabled: boolean;
  destructive?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  const button = (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className={cn('size-7 shrink-0 rounded-full', destructive && 'text-destructive hover:text-destructive')}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
    >
      {children}
    </Button>
  );
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {disabled ? <span className="inline-flex" tabIndex={0}>{button}</span> : button}
      </TooltipTrigger>
      <TooltipContent>{tooltipLabel ?? label}</TooltipContent>
    </Tooltip>
  );
}
