import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { ExecutionPlanSaveTarget } from '@/types';
import type { ExecutionPlanPhase, ExecutionPlanSession, ExecutionPlanSide } from '@/lib/execution-plan-editor';
import { executionPlanTargets } from '@/lib/execution-plan-editor';
import { Button } from '@/components/ui/button';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { cn } from '@/lib/utils';

interface ExecutionPlanSaveBarProps<T> {
  session: ExecutionPlanSession<T>;
  onTargetChange: (target: ExecutionPlanSaveTarget) => void;
  onSave: (target: ExecutionPlanSaveTarget) => void;
  onSelectSide: (side: ExecutionPlanSide) => void;
  onSplit: () => void;
  onOpenUnsplit: () => void;
  className?: string;
  onCancelUnsplit: () => void;
  onConfirmUnsplit: (side: ExecutionPlanSide) => void;
  onReload: () => void;
  onRecover?: () => void;
  dirty?: boolean;
  onRevert?: () => void;
  onViewIssues?: () => void;
}

export function ExecutionPlanSaveBar<T>({
  session,
  onTargetChange,
  onSave,
  onSelectSide,
  onSplit,
  onOpenUnsplit,
  className,
  onCancelUnsplit,
  onConfirmUnsplit,
  onReload,
  onRecover,
  dirty = false,
  onRevert,
  onViewIssues,
}: ExecutionPlanSaveBarProps<T>) {
  const { t } = useTranslation();
  const targets = executionPlanTargets(session);
  const busy = session.phase === 'loading' || session.phase === 'preflight' || session.phase === 'saving';
  const target = targets.includes(session.preferredTarget) ? session.preferredTarget : targets[0];
  return (
    <div className={cn('flex min-w-0 flex-col gap-2 border-b border-border/60 px-4 py-2', className)} data-execution-plan-save-bar={session.phase}>
      <div className="flex min-w-0 flex-col gap-2" data-execution-plan-actions>
        {session.split ? (
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <Tabs value={session.activeSide} onValueChange={(value) => onSelectSide(value as ExecutionPlanSide)}>
              <TabsList className="h-8">
                <TabsTrigger value="current">{t('executionPlan.currentRun')}</TabsTrigger>
                <TabsTrigger value="next">{t('executionPlan.nextRun')}</TabsTrigger>
              </TabsList>
            </Tabs>
            <Button type="button" variant="ghost" size="sm" className="shrink-0" onClick={onOpenUnsplit}>{t('executionPlan.unsplit')}</Button>
          </div>
        ) : session.diverged ? (
          <p className="min-w-0 text-xs leading-5 text-muted-foreground">{t('executionPlan.diverged')}</p>
        ) : null}
        <div className="flex w-full min-w-0 items-center gap-2" data-execution-plan-save-actions>
          {session.diverged && !session.split ? (
            <Button type="button" variant="outline" size="sm" className="shrink-0" data-execution-plan-split onClick={onSplit}>
              {t('executionPlan.split')}
            </Button>
          ) : null}
          <div className="min-w-0 max-w-64 flex-1">
            <Select value={target} onValueChange={(value) => onTargetChange(value as ExecutionPlanSaveTarget)} disabled={busy}>
              <SelectTrigger size="sm" className="h-8 w-full min-w-0" aria-label={t('executionPlan.saveTarget')}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {targets.map((item) => (
                  <SelectItem key={item} value={item}>{t(`executionPlan.targets.${item}`)}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <Button type="button" size="sm" className="shrink-0" disabled={busy} onClick={() => onSave(target)}>
            {busy ? <Loader2 className="size-3.5 animate-spin" /> : null}
            {busy ? t('executionPlan.saving') : t('executionPlan.save')}
          </Button>
          {dirty && onRevert ? (
            <Button type="button" variant="outline" size="sm" className="shrink-0" data-execution-plan-revert disabled={busy} onClick={onRevert}>
              {t('executionPlan.revert')}
            </Button>
          ) : null}
        </div>
      </div>
      <PhaseNotice phase={session.phase} errorCode={session.errorCode} errorNodeIds={session.errorNodeIds ?? []} partialTargets={session.partialTargets} onReload={onReload} onRecover={onRecover} onViewIssues={onViewIssues} />
      <Dialog open={session.unsplitOpen} onOpenChange={(open) => { if (!open) onCancelUnsplit(); }}>
        <DialogContent data-execution-plan-unsplit="dialog">
          <DialogHeader>
            <DialogTitle>{t('executionPlan.unsplitTitle')}</DialogTitle>
            <DialogDescription>{t('executionPlan.unsplitDescription')}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onCancelUnsplit}>{t('executionPlan.cancel')}</Button>
            <Button type="button" variant="outline" onClick={() => onConfirmUnsplit('current')}>{t('executionPlan.keepCurrent')}</Button>
            <Button type="button" onClick={() => onConfirmUnsplit('next')}>{t('executionPlan.keepNext')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function PhaseNotice({
  phase,
  errorCode,
  errorNodeIds,
  partialTargets,
  onReload,
  onRecover,
  onViewIssues,
}: {
  phase: ExecutionPlanPhase;
  errorCode: string | null;
  errorNodeIds: string[];
  partialTargets: Array<{ target: string; committed: boolean; errorCode?: string | null }>;
  onReload: () => void;
  onRecover?: () => void;
  onViewIssues?: () => void;
}) {
  const { t } = useTranslation();
  if (phase === 'conflict' || phase === 'error') {
    const nodeId = errorNodeIds.length > 0 ? errorNodeIds.join(', ') : null;
    const namedContinuableAgent = phase === 'error'
      && errorCode === 'conversation.execution-plan.agent-identity-changed'
      && nodeId;
    return (
      <div className="flex min-w-0 flex-wrap items-center gap-2 text-xs text-destructive">
        <span>
          {namedContinuableAgent
            ? t('errors.conversation.execution-plan.agent-identity-changed-node', { nodeId })
            : t(`errors.${errorCode ?? 'conversation.execution-plan.validation-failed'}`, { defaultValue: t('executionPlan.saveFailed') })}
        </span>
        {phase === 'conflict' ? <Button type="button" variant="outline" size="sm" onClick={onReload}>{t('executionPlan.reload')}</Button> : null}
        {phase === 'error' && errorCode === 'conversation.execution-plan.validation-failed' && onViewIssues ? (
          <Button type="button" variant="outline" size="sm" data-execution-plan-view-issues onClick={onViewIssues}>{t('executionPlan.viewIssues')}</Button>
        ) : null}
      </div>
    );
  }
  if (phase === 'partial') {
    return (
      <div className="flex min-w-0 flex-col gap-1 text-xs">
        {partialTargets.map((target) => (
          <span key={target.target} data-execution-plan-target={target.target} data-committed={target.committed ? 'true' : 'false'}>
            {t(`executionPlan.targets.${target.target}`)}: {target.committed ? t('executionPlan.committed') : t(`errors.${target.errorCode ?? 'conversation.execution-plan.partial-commit'}`, { defaultValue: t('executionPlan.notCommitted') })}
          </span>
        ))}
        <div className="flex gap-2">
          <Button type="button" variant="outline" size="sm" onClick={onReload}>{t('executionPlan.reload')}</Button>
          {onRecover ? <Button type="button" variant="outline" size="sm" onClick={onRecover}>{t('executionPlan.recover')}</Button> : null}
        </div>
      </div>
    );
  }
  if (phase === 'loading' || phase === 'preflight' || phase === 'saving') {
    return <span className="text-xs text-muted-foreground">{t('executionPlan.working')}</span>;
  }
  return null;
}
