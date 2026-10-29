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
import { useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import {
  deleteConversationComposerDraft,
  listConversationComposerDrafts,
  saveConversationComposerDraft,
  takeConversationComposerDraft,
} from '@/api';
import { displayAppError } from '@/i18n';
import { formatCompactRelativeTime } from '@/lib/datetime';
import { cn } from '@/lib/utils';
import type {
  ConversationComposerDraftBoxVm,
  ConversationPromptInput,
  ConversationQueuedPromptDraftVm,
} from '@/types';

/** The composer's current payload, already detached from the input; exactly one of commit/rollback runs. */
export interface CapturedComposerDraft {
  input: ConversationPromptInput;
  attachmentPaths: string[];
  commit: () => void;
  rollback: () => void;
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
}

type DraftBoxScope = { key: string; box: ConversationComposerDraftBoxVm | null };

function draftInput(draft: ConversationQueuedPromptDraftVm): ConversationPromptInput {
  return {
    displayText: draft.content,
    quotes: draft.quotes,
    ...(draft.role ? { role: draft.role } : {}),
    ...(draft.workspaceFiles?.length ? { workspaceFiles: draft.workspaceFiles } : {}),
  };
}

export function ComposerDraftBox({
  projectId,
  taskId,
  disabled,
  canSave,
  composerOccupied,
  captureDraft,
  restoreDraft,
}: ComposerDraftBoxProps) {
  const { t } = useTranslation();
  const scopeKey = `${projectId ?? ''}\u0000${taskId}`;
  const [scope, setScope] = useState<DraftBoxScope>({ key: scopeKey, box: null });
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState<'save' | string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const box = scope.key === scopeKey ? scope.box : null;

  // Only boxes for the current conversation, at the same or a newer revision, are accepted.
  const converge = (key: string, next: ConversationComposerDraftBoxVm) => {
    setScope((current) => {
      if (current.key !== key) return current;
      if (current.box && current.box.revision > next.revision) return current;
      return { key, box: next };
    });
  };

  useEffect(() => {
    let active = true;
    setScope((current) => (current.key === scopeKey ? current : { key: scopeKey, box: null }));
    setError(null);
    void listConversationComposerDrafts(projectId, taskId).then((next) => {
      if (active) converge(scopeKey, next);
    }).catch(() => {
      // The entry stays usable; the list shows its empty state until the next successful write.
    });
    return () => {
      active = false;
    };
  }, [projectId, scopeKey, taskId]);

  const count = box?.items.length ?? 0;
  const maxItems = box?.maxItems ?? 0;
  const full = box != null && count >= maxItems;
  const busy = pending !== null;

  const save = async () => {
    if (busy || !canSave || full) return;
    setPending('save');
    setError(null);
    const captured = await captureDraft().catch(() => null);
    if (!captured) {
      setPending(null);
      return;
    }
    try {
      const next = await saveConversationComposerDraft(
        projectId,
        taskId,
        captured.input,
        captured.attachmentPaths.length > 0 ? captured.attachmentPaths : undefined,
      );
      captured.commit();
      converge(scopeKey, next);
      // Saving hands focus back to the emptied composer, so the box steps aside.
      setOpen(false);
    } catch (saveError) {
      captured.rollback();
      setError(displayAppError(t, saveError));
    } finally {
      setPending(null);
    }
  };

  const take = async (draftId: string) => {
    if (busy || composerOccupied) return;
    setPending(draftId);
    setError(null);
    try {
      const result = await takeConversationComposerDraft(projectId, taskId, draftId);
      converge(scopeKey, result.drafts);
      if (!restoreDraft(result.draft)) {
        // The composer filled up while the draft was being taken; put it back rather than lose it.
        const restored = await saveConversationComposerDraft(
          projectId,
          taskId,
          draftInput(result.draft),
          result.draft.attachmentPaths.length > 0 ? result.draft.attachmentPaths : undefined,
        );
        converge(scopeKey, restored);
        setError(t('acp.composerDrafts.takeConflict'));
        return;
      }
      setOpen(false);
    } catch (takeError) {
      setError(displayAppError(t, takeError));
    } finally {
      setPending(null);
    }
  };

  const remove = async (draftId: string) => {
    if (busy) return;
    setPending(draftId);
    setError(null);
    try {
      converge(scopeKey, await deleteConversationComposerDraft(projectId, taskId, draftId));
    } catch (deleteError) {
      setError(displayAppError(t, deleteError));
    } finally {
      setPending(null);
    }
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
        className="w-80 p-0"
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
          <p className="px-3 pb-2 text-ui-caption text-destructive" role="alert">{error}</p>
        ) : null}
        {count === 0 ? (
          <p className="px-3 pb-3 text-xs text-muted-foreground" data-composer-draft-empty="true">
            {t('acp.composerDrafts.empty')}
          </p>
        ) : (
          <ul className="max-h-72 overflow-y-auto border-t border-border/60 py-1" data-composer-draft-list="true">
            {box?.items.map((draft) => (
              <li
                key={draft.id}
                className="flex min-w-0 items-start gap-1 px-3 py-1.5 text-xs"
                data-composer-draft-id={draft.id}
              >
                <div className="min-w-0 flex-1">
                  {draft.content.trim() ? (
                    <p className="line-clamp-2 whitespace-pre-wrap break-words leading-5 text-foreground/90 [overflow-wrap:anywhere]">
                      {draft.content}
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
                  onClick={() => { void remove(draft.id); }}
                >
                  <Trash2 className="size-3.5" />
                </DraftAction>
              </li>
            ))}
          </ul>
        )}
      </PopoverContent>
    </Popover>
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
