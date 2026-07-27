import { Eye, FolderOpen, RotateCcw, Workflow, ChevronDown } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { ConversationRunVm, ConversationSessionLeafVm } from '../../types';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import { EditableConversationTitle } from '@/components/conversation/EditableConversationTitle';

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
  const isDirect = run.runMode === 'direct';
  const selectedSessionDisplay = selectedSessionLeaf?.runtimeDisplay;
  const selectedSessionRunning = selectedSessionDisplay?.tone === 'running';
  const selectedSessionDotClass = runtimeDotClass(selectedSessionDisplay?.tone);

  return (
    <div className="shrink-0 bg-content-header px-5 pb-0.5 pt-0.5">
      <div className="flex min-w-0 items-center gap-2">
        <EditableConversationTitle
          title={run.title}
          metadata={!isDirect ? run.runId : null}
          className="flex-1"
          onTitleChange={onTitleChange}
        />

        {/* Session switcher toggle */}
        {!isDirect ? <Button
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
        </Button> : null}

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

          {!isDirect ? <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="ghost" size="icon" className="size-5.5" onClick={onRerun}>
                <RotateCcw className="size-3.5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {isRunning ? t('conversation.runtime.rerunConfirmAction') : t('conversation.runtime.rerun')}
            </TooltipContent>
          </Tooltip> : null}

          {onOpenInFileManager ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-5.5"
                  aria-label={t('conversation.runtime.openInFileManager')}
                  onClick={onOpenInFileManager}
                >
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
