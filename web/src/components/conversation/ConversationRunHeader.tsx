import { Eye, FolderOpen, RotateCcw, Workflow, ChevronDown, Pencil } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useCallback, useRef, useState } from 'react';
import type { ConversationRunVm, ConversationSessionLeafVm } from '../../types';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface ConversationRunHeaderProps {
  run: ConversationRunVm;
  onRerun: () => void;
  onEditWorkflow: () => void;
  onViewWorkflow: () => void;
  onOpenInFileManager?: () => void;
  onToggleSessionSwitcher: () => void;
  sessionSwitcherOpen: boolean;
  selectedSessionLeaf?: ConversationSessionLeafVm | null;
  canViewWorkflow?: boolean;
  canEditWorkflow?: boolean;
  onTitleChange?: (title: string) => void;
}

export function ConversationRunHeader({
  run,
  onRerun,
  onEditWorkflow,
  onViewWorkflow,
  onOpenInFileManager,
  onToggleSessionSwitcher,
  sessionSwitcherOpen,
  selectedSessionLeaf,
  canViewWorkflow,
  canEditWorkflow,
  onTitleChange,
}: ConversationRunHeaderProps) {
  const { t } = useTranslation();
  const isRunning = run.runStatus === 'running';
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleValue, setTitleValue] = useState(run.title);
  const inputRef = useRef<HTMLInputElement>(null);
  const selectedSessionDisplay = selectedSessionLeaf?.runtimeDisplay;
  const selectedSessionRunning = selectedSessionDisplay?.tone === 'running';
  const selectedSessionDotClass = runtimeDotClass(selectedSessionDisplay?.tone);

  const startEditing = useCallback(() => {
    setTitleValue(run.title);
    setEditingTitle(true);
    requestAnimationFrame(() => inputRef.current?.select());
  }, [run.title]);

  const commitTitle = useCallback(() => {
    setEditingTitle(false);
    const trimmed = titleValue.trim();
    if (trimmed && trimmed !== run.title) {
      onTitleChange?.(trimmed);
    }
  }, [titleValue, run.title, onTitleChange]);

  const handleTitleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter') { e.preventDefault(); commitTitle(); }
    if (e.key === 'Escape') { setTitleValue(run.title); setEditingTitle(false); }
  }, [commitTitle, run.title]);

  return (
    <div className="shrink-0 bg-gold-surface-high/60 px-5 pb-0.5 pt-0.5">
      <div className="flex min-w-0 items-center gap-2">
        {/* Title */}
        {editingTitle ? (
          <input
            ref={inputRef}
            className="min-w-0 flex-1 rounded-md border border-primary/40 bg-background px-2 py-0.5 text-sm font-semibold text-foreground outline-none ring-2 ring-primary/10"
            value={titleValue}
            onChange={(e) => setTitleValue(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={handleTitleKeyDown}
          />
        ) : (
          <button
            type="button"
            className="group -ml-1 flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-1 py-0.5 transition-colors hover:bg-muted/50"
            onClick={startEditing}
            title={t('conversation.runtime.titleEdit')}
          >
            <h1 className="min-w-0 truncate text-sm font-semibold leading-6 text-foreground">{run.title}</h1>
            <span className="shrink-0 text-[10px] text-muted-foreground/60">{run.runId}</span>
            <Pencil className="size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
          </button>
        )}

        {/* Session switcher toggle */}
        <Button
          variant="ghost"
          size="sm"
          className="h-5.5 gap-1 px-1.5 text-[11px]"
          onClick={onToggleSessionSwitcher}
        >
          {selectedSessionLeaf ? (
            <span
              aria-hidden="true"
              className="relative inline-flex size-3 shrink-0 items-center justify-center rounded-full border border-background/80"
            >
              {selectedSessionRunning ? (
                <span className="absolute inset-0 rounded-full bg-primary/18 animate-ping" />
              ) : null}
              <span className={cn('relative inline-block size-2 rounded-full', selectedSessionDotClass)} />
            </span>
          ) : null}
          <span className="truncate text-muted-foreground">
            {run.sessionTree.selectedSessionKey ?? t('conversation.runtime.sessionSwitcher')}
          </span>
          <ChevronDown className={cn('size-3 transition-transform', sessionSwitcherOpen && 'rotate-180')} />
        </Button>

        {/* Actions */}
        <div className="flex shrink-0 items-center gap-0.5">
          {canViewWorkflow ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" className="size-5.5" onClick={onViewWorkflow}>
                  <Eye className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t('conversation.runtime.viewWorkflow')}</TooltipContent>
            </Tooltip>
          ) : null}

          {canEditWorkflow ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" className="size-5.5" onClick={onEditWorkflow}>
                  <Workflow className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t('conversation.runtime.editWorkflow')}</TooltipContent>
            </Tooltip>
          ) : null}

          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="size-5.5" onClick={onRerun}>
                <RotateCcw className="size-3.5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {isRunning ? t('conversation.runtime.rerunConfirmAction') : t('conversation.runtime.rerun')}
            </TooltipContent>
          </Tooltip>

          {onOpenInFileManager ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" className="size-5.5" onClick={onOpenInFileManager}>
                  <FolderOpen className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t('conversation.runtime.openInFileManager')}</TooltipContent>
            </Tooltip>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function runtimeDotClass(tone?: string | null) {
  if (tone === 'success') return 'bg-emerald-500';
  if (tone === 'danger') return 'bg-red-500';
  if (tone === 'running') return 'bg-primary';
  if (tone === 'warning') return 'bg-yellow-500';
  if (tone === 'neutral') return 'bg-muted-foreground';
  return '';
}
