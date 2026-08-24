import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Search } from 'lucide-react';
import type { ConversationSearchResultVm } from '../../types';
import { searchConversationTasks } from '../../api';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import { conversationSearchHighlightSegments } from '@/lib/conversation-search';

interface ConversationSearchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSelectResult: (result: ConversationSearchResultVm) => void;
}

function SearchMatchPreview({ text, query }: { text: string; query: string }) {
  return conversationSearchHighlightSegments(text, query).map((segment, index) =>
    segment.highlighted ? (
      <mark
        key={`${index}-${segment.text}`}
        className="bg-transparent font-semibold text-foreground underline decoration-foreground/45 decoration-2 underline-offset-2"
      >
        {segment.text}
      </mark>
    ) : (
      <span key={`${index}-${segment.text}`}>{segment.text}</span>
    ),
  );
}

export function ConversationSearchDialog({ open, onOpenChange, onSelectResult }: ConversationSearchDialogProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<ConversationSearchResultVm[]>([]);
  const [loading, setLoading] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!open) {
      setQuery('');
      setResults([]);
      setLoading(false);
      setFailed(false);
      return;
    }
    const trimmed = query.trim();
    if (trimmed.length < 2) {
      setResults([]);
      setLoading(false);
      setFailed(false);
      return;
    }
    let active = true;
    const timer = setTimeout(async () => {
      setLoading(true);
      setFailed(false);
      try {
        const data = await searchConversationTasks(trimmed, 20);
        if (active) setResults(data);
      } catch {
        if (active) {
          setResults([]);
          setFailed(true);
        }
      } finally {
        if (active) setLoading(false);
      }
    }, 300);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [query, open]);

  const statusColor = (outcome?: string | null) => {
    if (!outcome) return 'bg-muted-foreground/30';
    if (outcome === 'success') return 'bg-emerald-500';
    if (outcome === 'failure' || outcome === 'killed') return 'bg-red-500';
    return 'bg-yellow-500';
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg gap-0 p-0">
        <DialogHeader className="px-4 pt-4 pb-2">
          <DialogTitle className="text-base">{t('conversation.search.title')}</DialogTitle>
        </DialogHeader>
        <div className="px-4 pb-2">
          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              className="h-9 pl-8 text-sm"
              placeholder={t('conversation.search.placeholder')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              autoFocus
            />
          </div>
        </div>
        <div className="max-h-80 overflow-y-auto border-t">
          {loading ? (
            <div className="px-4 py-6 text-center text-sm text-muted-foreground">{t('common.loading')}</div>
          ) : failed ? (
            <div className="px-4 py-6 text-center text-sm text-destructive">{t('conversation.search.failed')}</div>
          ) : results.length === 0 ? (
            <div className="px-4 py-6 text-center text-sm text-muted-foreground">
              {query.trim().length >= 2 ? t('conversation.search.noResults') : t('conversation.search.placeholder')}
            </div>
          ) : (
            <div>
              <div className="px-4 py-2 text-xs text-muted-foreground">
                {t('conversation.search.resultCount', { count: results.length })}
              </div>
              {results.map((result) => (
                <button
                  key={`${result.projectId}/${result.taskId}`}
                  type="button"
                  className="flex w-full items-center gap-3 px-4 py-2.5 text-left hover:bg-accent transition-colors"
                  onClick={() => {
                    onSelectResult(result);
                    onOpenChange(false);
                  }}
                >
                  {result.runMode === 'direct' && result.agentIdentity ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <img
                          src={agentIconSrc(result.agentIdentity.iconKey)}
                          alt=""
                          className={agentIconClass(result.agentIdentity.iconKey, 'size-4 shrink-0')}
                        />
                      </TooltipTrigger>
                      <TooltipContent>{result.agentIdentity.displayName}</TooltipContent>
                    </Tooltip>
                  ) : (
                    <span className={`size-2 shrink-0 rounded-full ${statusColor(result.latestRun?.outcome)}`} />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">{result.title}</div>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <div className="truncate text-xs text-muted-foreground">
                          <SearchMatchPreview text={result.matchPreview} query={query} />
                        </div>
                      </TooltipTrigger>
                      <TooltipContent className="max-w-[360px] whitespace-pre-wrap break-words">{result.matchPreview}</TooltipContent>
                    </Tooltip>
                  </div>
                  {result.workspaceName ? (
                    <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-ui-micro text-muted-foreground">
                      {result.workspaceName}
                    </span>
                  ) : null}
                </button>
              ))}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
