import { useEffect, useState } from 'react';
import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { getAcpCompactionSummary } from '@/api';
import { displayAppError } from '@/i18n';
import { Button } from '@/components/ui/button';
import { ReadonlyMarkdownDocument } from '@/components/acp/ACPChatDialog';
import type { CompactionSummaryWorkspaceResource } from './right-workspace-context';

type SummaryState = { status: 'loading' } | { status: 'ready'; markdown: string | null } | { status: 'error'; message: string };

export function CompactionSummaryWorkspacePanel({ resource }: { resource: CompactionSummaryWorkspaceResource }) {
  const { t } = useTranslation();
  const [state, setState] = useState<SummaryState>({ status: 'loading' });
  const [request, setRequest] = useState(0);
  const [viewMode, setViewMode] = useState<'rendered' | 'raw'>('rendered');
  useEffect(() => {
    let active = true;
    const { locator } = resource;
    setState({ status: 'loading' });
    void getAcpCompactionSummary(locator, { branchId: locator.branchId, eventId: locator.eventId })
      .then(({ markdown }) => { if (active) setState({ status: 'ready', markdown }); })
      .catch((error) => { if (active) setState({ status: 'error', message: displayAppError(t, error) }); });
    return () => { active = false; };
  }, [resource.key, request, t]);
  return (
    <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col overflow-hidden" data-right-workspace-resource="compaction-summary">
      {state.status === 'loading' ? (
        <div role="status" className="flex flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="size-4 animate-spin" />{t('acp.compactionSummaryLoading')}
        </div>
      ) : state.status === 'error' ? (
        <div role="alert" className="flex flex-1 flex-col items-center justify-center gap-3 p-4 text-sm">
          <span className="break-words">{state.message}</span>
          <Button variant="outline" onClick={() => setRequest(value => value + 1)}>{t('common.retry')}</Button>
        </div>
      ) : state.markdown ? (
        <ReadonlyMarkdownDocument documentKey={resource.key} content={state.markdown} viewMode={viewMode} onViewModeChange={setViewMode} />
      ) : (
        <div className="flex flex-1 items-center justify-center p-4 text-sm text-muted-foreground">{t('acp.compactionSummaryUnavailable')}</div>
      )}
    </div>
  );
}
