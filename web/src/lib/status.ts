import { cn } from '@/lib/utils';

const runningTones = ['running', 'in_progress', 'active'];
const successTones = ['completed', 'complete', 'success', 'succeeded', 'valid', 'passed'];
const warningTones = ['warning', 'pending', 'paused', 'resumable', 'missing', 'missing-workflow', 'skipped'];
const dangerTones = ['failed', 'failure', 'error', 'invalid', 'killed', 'cancelled', 'canceled'];
const stoppableRunStatuses = ['running', 'paused'];

export function isRunStoppable(status?: string | null) {
  return stoppableRunStatuses.includes((status ?? '').toLowerCase());
}

export function normalizeTone(value?: string | null, explicitTone?: string | null) {
  const tone = (explicitTone ?? value ?? 'neutral').toLowerCase();
  if (runningTones.includes(tone)) return 'running';
  if (successTones.includes(tone)) return 'success';
  if (warningTones.includes(tone)) return 'warning';
  if (dangerTones.includes(tone)) return 'danger';
  return 'neutral';
}

export function statusBadgeClass(value?: string | null, explicitTone?: string | null) {
  const tone = normalizeTone(value, explicitTone);
  return cn(
    'font-semibold uppercase tracking-[0.12em]',
    tone === 'running' && 'border-gold-running/35 bg-gold-running/10 text-gold-running',
    tone === 'success' && 'border-gold-success/35 bg-gold-success/10 text-gold-success',
    tone === 'warning' && 'border-gold-warning/35 bg-gold-warning/10 text-gold-warning',
    tone === 'danger' && 'border-gold-danger/35 bg-gold-danger/10 text-gold-danger',
    tone === 'neutral' && 'border-border bg-secondary text-muted-foreground',
  );
}

export function toneSurfaceClass(value?: string | null, explicitTone?: string | null) {
  const tone = normalizeTone(value, explicitTone);
  return cn(
    tone === 'running' && 'border-gold-running/30 bg-gold-running/10 text-gold-running',
    tone === 'success' && 'border-gold-success/30 bg-gold-success/10 text-gold-success',
    tone === 'warning' && 'border-gold-warning/30 bg-gold-warning/10 text-gold-warning',
    tone === 'danger' && 'border-gold-danger/30 bg-gold-danger/10 text-gold-danger',
    tone === 'neutral' && 'border-border bg-card text-muted-foreground',
  );
}

export function graphNodeClass(value?: string | null, explicitTone?: string | null, current = false, selected = false, hasArtifacts = false, hasAttachments = false) {
  return cn(
    'h-auto min-h-[116px] flex-col items-stretch justify-start rounded-xl border bg-card p-4 text-left shadow-sm hover:bg-accent/40',
    toneSurfaceClass(value, explicitTone),
    hasArtifacts && 'border-gold-warning/45 bg-gold-warning/10',
    hasAttachments && 'shadow-[0_0_0_1px_rgba(148,163,184,0.22)]',
    current && 'ring-2 ring-primary/50',
    selected && 'border-primary bg-primary/10 text-primary shadow-[0_0_0_1px_rgba(245,158,11,0.25)]',
  );
}
