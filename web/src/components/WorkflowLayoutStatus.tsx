import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { WorkflowLayoutState } from '@/hooks/useWorkflowLayout';

/**
 * Canvas overlay for the first layout of a graph and for layout failures. Later
 * relayouts keep the previous drawing visible instead of falling back to loading.
 */
export function WorkflowLayoutStatus({ state }: { state: Pick<WorkflowLayoutState, 'layout' | 'pending' | 'failed'> }) {
  const { t } = useTranslation();
  if (state.failed) {
    return (
      <div role="alert" className="pointer-events-none absolute inset-0 z-10 flex items-center justify-center text-sm text-destructive">
        {t('graph.layoutFailed')}
      </div>
    );
  }
  if (!state.pending || state.layout) return null;
  return (
    <div role="status" className="workflow-layout-pending pointer-events-none absolute inset-0 z-10 flex items-center justify-center gap-2 text-sm text-muted-foreground">
      <Loader2 className="size-4 animate-spin" />
      {t('graph.layoutPending')}
    </div>
  );
}
