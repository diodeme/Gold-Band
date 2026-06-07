import { useState, useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { Search } from 'lucide-react';
import type { ConversationSearchResultVm } from '../../types';
import { searchConversationTasks } from '../../api';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';

interface ConversationSearchDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSelectResult: (result: ConversationSearchResultVm) => void;
}

export function ConversationSearchDialog({ open, onOpenChange, onSelectResult }: ConversationSearchDialogProps) {
  const { t } = useTranslation();
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<ConversationSearchResultVm[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open) {
      setQuery('');
      setResults([]);
      return;
    }
    const trimmed = query.trim();
    if (trimmed.length < 2) {
      setResults([]);
      return;
    }
    const timer = setTimeout(async () => {
      setLoading(true);
      try {
        const data = await searchConversationTasks(trimmed, 20);
        setResults(data);
      } catch {
        setResults([]);
      } finally {
        setLoading(false);
      }
    }, 300);
    return () => clearTimeout(timer);
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
                  <span className={`size-2 shrink-0 rounded-full ${statusColor(result.latestRun?.outcome)}`} />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">{result.title}</div>
                    <div className="truncate text-xs text-muted-foreground">{result.requirementPreview}</div>
                  </div>
                  {result.workspaceName ? (
                    <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">
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
