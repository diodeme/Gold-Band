import { RotateCcw, Workflow, ChevronDown, Pencil } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useCallback, useRef, useState } from 'react';
import type { ConversationRunVm } from '../../types';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface ConversationRunHeaderProps {
  run: ConversationRunVm;
  onRerun: () => void;
  onEditWorkflow: () => void;
  onToggleSessionSwitcher: () => void;
  sessionSwitcherOpen: boolean;
  onTitleChange?: (title: string) => void;
}

export function ConversationRunHeader({
  run,
  onRerun,
  onEditWorkflow,
  onToggleSessionSwitcher,
  sessionSwitcherOpen,
  onTitleChange,
}: ConversationRunHeaderProps) {
  const { t } = useTranslation();
  const isRunning = run.runStatus === 'running';
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleValue, setTitleValue] = useState(run.title);
  const inputRef = useRef<HTMLInputElement>(null);

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
    <div className="shrink-0 border-b bg-muted/10 px-5 py-3">
      <div className="flex min-w-0 items-center gap-3">
        {/* Title */}
        {editingTitle ? (
          <input
            ref={inputRef}
            className="min-w-0 flex-1 rounded-md border border-primary/40 bg-background px-2 py-0.5 text-base font-semibold text-foreground outline-none ring-2 ring-primary/10"
            value={titleValue}
            onChange={(e) => setTitleValue(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={handleTitleKeyDown}
          />
        ) : (
          <button
            type="button"
            className="group flex min-w-0 flex-1 items-center gap-2 rounded-md px-1 py-0.5 -ml-1 hover:bg-muted/50 transition-colors"
            onClick={startEditing}
            title={t('conversation.runtime.titleEdit')}
          >
            <h1 className="min-w-0 truncate text-base font-semibold text-foreground">{run.title}</h1>
            <Pencil className="size-3 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" />
          </button>
        )}

        {/* Session switcher toggle */}
        <Button
          variant="ghost"
          size="sm"
          className="h-7 gap-1 px-2 text-xs"
          onClick={onToggleSessionSwitcher}
        >
          <span className="truncate text-muted-foreground">
            {run.sessionTree.selectedSessionKey ?? t('conversation.runtime.sessionSwitcher')}
          </span>
          <ChevronDown className={cn('size-3 transition-transform', sessionSwitcherOpen && 'rotate-180')} />
        </Button>

        {/* Actions */}
        <div className="flex shrink-0 items-center gap-1">
          {run.runMode === 'workflow' ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="ghost" size="icon" className="size-7" onClick={onEditWorkflow}>
                  <Workflow className="size-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{t('conversation.runtime.editWorkflow')}</TooltipContent>
            </Tooltip>
          ) : null}

          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="size-7" onClick={onRerun}>
                <RotateCcw className="size-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {isRunning ? t('conversation.runtime.rerunConfirmAction') : t('conversation.runtime.rerun')}
            </TooltipContent>
          </Tooltip>
        </div>
      </div>
    </div>
  );
}
