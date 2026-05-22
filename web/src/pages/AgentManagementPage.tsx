import { useEffect, useMemo, useState, type ChangeEvent, type InputHTMLAttributes, type TextareaHTMLAttributes } from 'react';
import { useTranslation } from 'react-i18next';
import { createAgent, deleteAgent, doctorAgent, updateAgent } from '../api';
import type { AgentRegistryVm, ManagedAgentInput, ManagedAgentVm, SupportedAgentTypeVm } from '../types';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Page, PageHeader } from '@/components/PageScaffold';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Textarea } from '@/components/ui/textarea';
import { AlertTriangle, CheckCircle2, CircleHelp, LoaderCircle, Pencil, Plus, RefreshCw, Stethoscope, Trash2 } from 'lucide-react';
import { cn } from '@/lib/utils';

interface AgentManagementPageProps {
  vm: AgentRegistryVm | null;
  loading: boolean;
  onRefresh: () => void;
  onRegistryChange: (vm: AgentRegistryVm) => void;
}

type EditorMode = 'create' | 'edit';
type Notice = { tone: 'success' | 'error'; message: string };

const defaultForm = (): ManagedAgentInput => ({ displayName: '', command: '', args: [], env: {} });

export function AgentManagementPage({ vm, loading, onRefresh, onRegistryChange }: AgentManagementPageProps) {
  const { t } = useTranslation();
  const [sheetOpen, setSheetOpen] = useState(false);
  const [editorMode, setEditorMode] = useState<EditorMode>('create');
  const [selectedType, setSelectedType] = useState('');
  const [form, setForm] = useState<ManagedAgentInput>(defaultForm);
  const [argsText, setArgsText] = useState('');
  const [envText, setEnvText] = useState('');
  const [saving, setSaving] = useState(false);
  const [diagnosingType, setDiagnosingType] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ManagedAgentVm | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [error, setError] = useState<string | null>(null);

  const supportedTypes = vm?.supportedTypes ?? [];
  const configuredTypes = useMemo(() => new Set(vm?.agents.map((agent) => agent.agentType) ?? []), [vm]);

  useEffect(() => {
    if (!sheetOpen) {
      setForm(defaultForm());
      setArgsText('');
      setEnvText('');
      setError(null);
    }
  }, [sheetOpen]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 3600);
    return () => window.clearTimeout(timer);
  }, [notice]);

  const openCreate = (agentType: string) => {
    setEditorMode('create');
    setSelectedType(agentType);
    setForm(defaultForm());
    setArgsText('');
    setEnvText('');
    setError(null);
    setSheetOpen(true);
  };

  const openEdit = (agent: ManagedAgentVm) => {
    setEditorMode('edit');
    setSelectedType(agent.agentType);
    setForm({
      displayName: agent.displayName,
      command: agent.command,
      args: agent.args,
      env: Object.fromEntries(agent.env.map((entry) => [entry.key, entry.value])),
    });
    setArgsText(formatArgs(agent.args));
    setEnvText(formatEnv(agent.env));
    setError(null);
    setSheetOpen(true);
  };

  const submit = async () => {
    if (!selectedType.trim()) {
      setError(t('agentManagement.agentTypeRequired'));
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const input = buildAgentInput(form, argsText, envText);
      const next = editorMode === 'create'
        ? await createAgent(selectedType, input)
        : await updateAgent(selectedType, input);
      onRegistryChange(next);
      setSheetOpen(false);
    } catch (nextError) {
      setError(String(nextError));
    } finally {
      setSaving(false);
    }
  };

  const runDoctor = async (agentType: string) => {
    setDiagnosingType(agentType);
    setError(null);
    setNotice(null);
    try {
      const next = await doctorAgent(agentType);
      onRegistryChange(next);
      const diagnostic = next.agents.find((agent) => agent.agentType === agentType)?.diagnostic;
      setNotice(diagnostic?.available
        ? { tone: 'success', message: t('agentManagement.diagnosticComplete') }
        : { tone: 'error', message: t('agentManagement.diagnosticFailed', { reason: diagnostic?.reason ?? t('agentManagement.diagnosticFailedFallback') }) });
    } catch (nextError) {
      setNotice({ tone: 'error', message: t('agentManagement.diagnosticFailed', { reason: String(nextError) }) });
    } finally {
      setDiagnosingType(null);
    }
  };

  const confirmDelete = async () => {
    if (!deleteTarget) return;
    try {
      onRegistryChange(await deleteAgent(deleteTarget.agentType));
      setDeleteTarget(null);
    } catch (nextError) {
      setError(String(nextError));
      setDeleteTarget(null);
    }
  };

  return (
    <Page className="space-y-6 p-8">
      <PageHeader
        eyebrow={t('agentManagement.eyebrow')}
        title={t('agentManagement.title')}
        subtitle={t('agentManagement.subtitle')}
        actions={(
          <>
            <Button variant="outline" disabled={loading} onClick={onRefresh}>
              <RefreshCw className={cn(loading && 'animate-spin')} />
              {t('common.refresh')}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button>
                  <Plus />
                  {t('agentManagement.addAgent')}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-56">
                {supportedTypes.map((agentType) => (
                  <DropdownMenuItem
                    key={agentType.agentType}
                    disabled={!agentType.supported || agentType.configured}
                    onClick={() => openCreate(agentType.agentType)}
                  >
                    <div className="flex min-w-0 flex-1 items-center justify-between gap-3">
                      <span className="truncate">{agentType.label}</span>
                      {!agentType.supported ? <Badge variant="secondary">{t('agentManagement.pending')}</Badge> : agentType.configured ? <Badge variant="secondary">{t('agentManagement.configured')}</Badge> : null}
                    </div>
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        )}
      />

      {notice ? (
        <Alert
          className={cn(
            'rounded-xl px-4 py-3',
            notice.tone === 'success'
              ? 'border-gold-success/35 bg-gold-success/10 text-gold-success'
              : 'border-destructive/45 bg-destructive/10 text-destructive',
          )}
        >
          {notice.tone === 'success' ? <CheckCircle2 /> : <AlertTriangle />}
          <AlertDescription className="text-sm font-medium text-current">
            {notice.message}
          </AlertDescription>
        </Alert>
      ) : null}
      {error ? <div className="rounded-xl border border-destructive/40 bg-destructive/5 px-4 py-3 text-sm text-destructive">{error}</div> : null}

      {vm && vm.agents.length > 0 ? (
        <div className="grid gap-4 xl:grid-cols-2">
          {vm.agents.map((agent) => (
            <AgentCard
              key={agent.agentType}
              agent={agent}
              diagnosing={diagnosingType === agent.agentType}
              onEdit={() => openEdit(agent)}
              onDelete={() => setDeleteTarget(agent)}
              onDoctor={() => void runDoctor(agent.agentType)}
            />
          ))}
        </div>
      ) : (
        <AppCard>
          <EmptyState>{loading ? t('common.loading') : t('agentManagement.empty')}</EmptyState>
        </AppCard>
      )}

      <Sheet open={sheetOpen} onOpenChange={setSheetOpen}>
        <SheetContent className="w-[720px] max-w-[96vw] overflow-hidden sm:max-w-[720px]">
          <SheetHeader className="border-b border-border/60 px-6 py-5">
            <SheetTitle>{editorMode === 'create' ? t('agentManagement.createTitle') : t('agentManagement.editTitle')}</SheetTitle>
            <SheetDescription>{editorMode === 'create' ? t('agentManagement.createDescription') : t('agentManagement.editDescription')}</SheetDescription>
          </SheetHeader>
          <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 pb-6">
            <Field label={t('agentManagement.agentType')}>
              <TextInput value={selectedType} disabled />
            </Field>
            <Field label={t('agentManagement.displayName')}>
              <TextInput value={form.displayName} onChange={(event: ChangeEvent<HTMLInputElement>) => setForm((current) => ({ ...current, displayName: event.target.value }))} />
            </Field>
            <Field label={t('agentManagement.command')}>
              <TextInput value={form.command} onChange={(event: ChangeEvent<HTMLInputElement>) => setForm((current) => ({ ...current, command: event.target.value }))} />
            </Field>
            <Field label={t('agentManagement.args')} description={t('agentManagement.argsDescription')}>
              <ConfigTextarea
                className="min-h-32"
                value={argsText}
                placeholder={'-y\n@agentclientprotocol/claude-agent-acp@latest'}
                onChange={(event) => setArgsText(event.target.value)}
              />
            </Field>
            <Field label={t('agentManagement.env')} description={t('agentManagement.envDescription')}>
              <ConfigTextarea
                className="min-h-44"
                value={envText}
                placeholder={'ANTHROPIC_API_KEY=...\nNODE_OPTIONS=--max-old-space-size=4096'}
                onChange={(event) => setEnvText(event.target.value)}
              />
            </Field>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="outline" onClick={() => setSheetOpen(false)}>{t('common.close')}</Button>
              <Button disabled={saving || !selectedType.trim() || !form.displayName.trim() || !form.command.trim()} onClick={() => void submit()}>{t('common.save')}</Button>
            </div>
          </div>
        </SheetContent>
      </Sheet>

      <AlertDialog open={Boolean(deleteTarget)} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('agentManagement.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('agentManagement.deleteDescription', { agent: deleteTarget?.displayName ?? deleteTarget?.agentType ?? '' })}</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction onClick={() => void confirmDelete()}>{t('agentManagement.deleteAction')}</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </Page>
  );
}

function AgentCard({ agent, diagnosing, onEdit, onDelete, onDoctor }: { agent: ManagedAgentVm; diagnosing: boolean; onEdit: () => void; onDelete: () => void; onDoctor: () => void }) {
  const { t } = useTranslation();
  const diagnostic = agent.diagnostic;
  return (
    <AppCard className="gap-5 px-5 sm:px-6">
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 items-start gap-4">
          <span className="grid size-12 shrink-0 place-items-center rounded-2xl border border-border/60 bg-background">
            <img src={agentIconSrc(agent.iconKey)} alt="" className="size-8 object-contain" />
          </span>
          <div className="min-w-0 space-y-2">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="truncate text-base font-semibold text-foreground">{agent.displayName}</h3>
              <Badge variant="secondary" className="rounded-full px-2.5">{agent.agentType}</Badge>
            </div>
            <div className="font-mono text-xs text-muted-foreground">{agent.command} {agent.args.join(' ')}</div>
          </div>
        </div>
        <DiagnosticBadge diagnostic={diagnostic} />
      </div>
      <div className="grid gap-3 text-sm text-muted-foreground sm:grid-cols-2">
        <Info label={t('agentManagement.command')} value={agent.command} mono />
        <Info label={t('agentManagement.args')} value={agent.args.length > 0 ? agent.args.join(' ') : '-'} mono />
        <Info label={t('agentManagement.env')} value={agent.env.length > 0 ? `${agent.env.length} ${t('agentManagement.entries')}` : '-'} />
        <Info label={t('agentManagement.lastChecked')} value={formatLocalTimestamp(diagnostic?.checkedAt)} />
      </div>
      {diagnostic?.reason ? <div className="rounded-xl border border-border/60 bg-muted/20 px-3 py-3 text-sm text-muted-foreground">{diagnostic.reason}</div> : null}
      <div className="flex flex-wrap justify-end gap-2">
        <Button variant="outline" disabled={diagnosing} aria-busy={diagnosing} onClick={onDoctor}>
          {diagnosing ? <LoaderCircle className="animate-spin" /> : <Stethoscope />}
          {diagnosing ? t('agentManagement.diagnosing') : t('agentManagement.diagnose')}
        </Button>
        <Button variant="outline" onClick={onEdit}><Pencil />{t('agentManagement.edit')}</Button>
        <Button variant="outline" onClick={onDelete}><Trash2 />{t('agentManagement.delete')}</Button>
      </div>
    </AppCard>
  );
}

function Field({ label, description, children }: { label: string; description?: string; children: React.ReactNode }) {
  return (
    <label className="block space-y-2">
      <div className="space-y-1">
        <div className="text-sm font-semibold text-foreground">{label}</div>
        {description ? <div className="text-xs text-muted-foreground">{description}</div> : null}
      </div>
      {children}
    </label>
  );
}

function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={cn('h-10 w-full rounded-md border border-border/60 bg-background px-3 text-sm text-foreground shadow-sm outline-none transition focus:border-primary focus:ring-2 focus:ring-ring/40 disabled:cursor-not-allowed disabled:opacity-60', props.className)} />;
}

function ConfigTextarea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <Textarea {...props} className={cn('resize-y border-border/70 bg-card/70 font-mono text-sm leading-6 shadow-inner outline-none placeholder:text-muted-foreground/55 focus-visible:ring-primary/35', props.className)} />;
}

function Info({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="rounded-xl border border-border/60 bg-muted/10 px-3 py-3">
      <div className="text-[11px] uppercase tracking-[0.14em] text-muted-foreground">{label}</div>
      <div className={cn('mt-1.5 min-w-0 break-all text-sm text-foreground', mono && 'font-mono text-xs')}>{value}</div>
    </div>
  );
}

function buildAgentInput(form: ManagedAgentInput, argsText: string, envText: string): ManagedAgentInput {
  return {
    displayName: form.displayName,
    command: form.command,
    args: parseArgs(argsText),
    env: parseEnv(envText),
  };
}

function formatArgs(args: string[]) {
  return args.join('\n');
}

function formatEnv(env: ManagedAgentVm['env']) {
  return env.map((entry) => `${entry.key}=${entry.value}`).join('\n');
}

function parseArgs(value: string) {
  return value.split(/\s+/).map((item) => item.trim()).filter(Boolean);
}

function parseEnv(value: string) {
  return Object.fromEntries(value.split(/\r?\n/).map((line) => line.trim()).filter(Boolean).map((line) => {
    const index = line.indexOf('=');
    return index === -1 ? [line, ''] : [line.slice(0, index).trim(), line.slice(index + 1).trim()];
  }).filter(([key]) => key));
}

function formatLocalTimestamp(value?: string | null) {
  if (!value) return '-';
  const epoch = /^(\d+)Z?$/.exec(value.trim());
  const date = epoch ? new Date(Number(epoch[1]) * 1000) : new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const pad = (part: number) => part.toString().padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function DiagnosticBadge({ diagnostic }: { diagnostic?: ManagedAgentVm['diagnostic'] }) {
  const { t } = useTranslation();
  const status = diagnostic?.status ?? 'unknown';
  const icon = status === 'healthy'
    ? <CheckCircle2 className="size-4 text-gold-success" />
    : status === 'unhealthy'
      ? <AlertTriangle className="size-4 text-destructive" />
      : <CircleHelp className="size-4 text-muted-foreground" />;
  return <Badge variant="outline" className="rounded-full px-2.5">{icon}<span className="ml-1">{t(`agentManagement.status.${status}`)}</span></Badge>;
}

function agentIconSrc(iconKey: string) {
  return `/agent-icons/${iconKey}.svg`;
}
