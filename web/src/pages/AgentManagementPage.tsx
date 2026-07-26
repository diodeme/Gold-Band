import { useEffect, useMemo, useState, type ChangeEvent, type InputHTMLAttributes, type TextareaHTMLAttributes } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { createAgent, deleteAgent, doctorAgent, updateAgent } from '../api';
import { displayAppError } from '../i18n';
import type { AgentRegistryVm, ManagedAgentInput, ManagedAgentVm, SupportedAgentTypeVm } from '../types';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Page, PageHeader } from '@/components/PageScaffold';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Switch } from '@/components/ui/switch';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { AlertTriangle, CheckCircle2, CircleHelp, LoaderCircle, Pencil, Plus, RefreshCw, Stethoscope, Trash2 } from 'lucide-react';
import { cn } from '@/lib/utils';
import { formatLocalDateTime } from '@/lib/datetime';

interface AgentManagementPageProps {
  vm: AgentRegistryVm | null;
  loading: boolean;
  onRefresh: () => void;
  onRegistryChange: (vm: AgentRegistryVm) => void;
}

type EditorMode = 'create' | 'edit';
type Notice = { tone: 'success' | 'error'; message: string };

const ACP_REGISTRY_URL = 'https://agentclientprotocol.com/get-started/registry';

const defaultForm = (): ManagedAgentInput => ({
  displayName: '',
  command: '',
  args: [],
  env: {},
  primaryAgentDir: '',
  compatibleAgentDirs: [],
  externalSessionSyncEnabled: false,
});
const formFromSupportedAgent = (agentType?: SupportedAgentTypeVm): ManagedAgentInput => agentType ? ({
  displayName: agentType.defaultDisplayName,
  command: agentType.defaultCommand,
  args: agentType.defaultArgs,
  env: Object.fromEntries(agentType.defaultEnv.map((entry) => [entry.key, entry.value])),
  primaryAgentDir: agentType.primaryAgentDir,
  compatibleAgentDirs: agentType.compatibleAgentDirs,
  externalSessionSyncEnabled: false,
}) : defaultForm();

export function AgentManagementPage({ vm, loading, onRefresh, onRegistryChange }: AgentManagementPageProps) {
  const { t } = useTranslation();
  const [sheetOpen, setSheetOpen] = useState(false);
  const [editorMode, setEditorMode] = useState<EditorMode>('create');
  const [selectedType, setSelectedType] = useState('');
  const [form, setForm] = useState<ManagedAgentInput>(defaultForm);
  const [argsText, setArgsText] = useState('');
  const [envText, setEnvText] = useState('');
  const [compatibleAgentDirsText, setCompatibleAgentDirsText] = useState('');
  const [initialEditInput, setInitialEditInput] = useState<ManagedAgentInput | null>(null);
  const [saving, setSaving] = useState(false);
  const [diagnosingType, setDiagnosingType] = useState<string | null>(null);
  const [automaticDiagnosingType, setAutomaticDiagnosingType] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ManagedAgentVm | null>(null);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [error, setError] = useState<string | null>(null);

  const supportedTypes = vm?.supportedTypes ?? [];
  const configuredTypes = useMemo(() => new Set(vm?.agents.map((agent) => agent.agentType) ?? []), [vm]);
  const currentInput = useMemo(
    () => buildAgentInput(form, argsText, envText, compatibleAgentDirsText),
    [argsText, compatibleAgentDirsText, envText, form],
  );
  const hasFormChanges = editorMode === 'create'
    || initialEditInput === null
    || hasManagedAgentInputChanged(initialEditInput, currentInput);

  useEffect(() => {
    if (!sheetOpen) {
      setForm(defaultForm());
      setArgsText('');
      setEnvText('');
      setCompatibleAgentDirsText('');
      setInitialEditInput(null);
      setError(null);
    }
  }, [sheetOpen]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 3600);
    return () => window.clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    if (!automaticDiagnosingType) return;
    const diagnostic = vm?.agents.find((agent) => agent.agentType === automaticDiagnosingType)?.diagnostic;
    if (!diagnostic) return;
    setAutomaticDiagnosingType(null);
    setNotice(diagnostic.available
      ? { tone: 'success', message: t('agentManagement.diagnosticComplete') }
      : { tone: 'error', message: t('agentManagement.diagnosticFailed', { reason: diagnostic.reason ?? t('agentManagement.diagnosticFailedFallback') }) });
  }, [automaticDiagnosingType, t, vm]);

  const openCreate = (agentType: SupportedAgentTypeVm) => {
    const nextForm = formFromSupportedAgent(agentType);
    setEditorMode('create');
    setSelectedType(agentType.agentType);
    setForm(nextForm);
    setArgsText(formatArgs(nextForm.args));
    setEnvText(formatEnv(Object.entries(nextForm.env).map(([key, value]) => ({ key, value }))));
    setCompatibleAgentDirsText(formatAgentDirs(nextForm.compatibleAgentDirs));
    setInitialEditInput(null);
    setError(null);
    setSheetOpen(true);
  };

  const openEdit = (agent: ManagedAgentVm) => {
    const nextForm = agentInputFromVm(agent);
    const nextArgsText = formatArgs(agent.args);
    const nextEnvText = formatEnv(agent.env);
    const nextCompatibleAgentDirsText = formatAgentDirs(agent.compatibleAgentDirs);
    setEditorMode('edit');
    setSelectedType(agent.agentType);
    setForm(nextForm);
    setArgsText(nextArgsText);
    setEnvText(nextEnvText);
    setCompatibleAgentDirsText(nextCompatibleAgentDirsText);
    setInitialEditInput(buildAgentInput(nextForm, nextArgsText, nextEnvText, nextCompatibleAgentDirsText));
    setError(null);
    setSheetOpen(true);
  };

  const submit = async () => {
    if (!selectedType.trim()) {
      setError(t('agentManagement.agentTypeRequired'));
      return;
    }
    if (editorMode === 'edit' && initialEditInput && !hasManagedAgentInputChanged(initialEditInput, currentInput)) {
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const next = editorMode === 'create'
        ? await createAgent(selectedType, currentInput)
        : await updateAgent(selectedType, currentInput);
      onRegistryChange(next);
      setAutomaticDiagnosingType(selectedType);
      setNotice({ tone: 'success', message: t('agentManagement.savedAndDiagnosing') });
      window.setTimeout(onRefresh, 250);
      setSheetOpen(false);
    } catch (nextError) {
      setError(displayAppError(t, nextError));
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
      setNotice({ tone: 'error', message: t('agentManagement.diagnosticFailed', { reason: displayAppError(t, nextError) }) });
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
      setError(displayAppError(t, nextError));
      setDeleteTarget(null);
    }
  };

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        title={<span className="text-title">{t('agentManagement.title')}</span>}
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
                    onClick={() => openCreate(agentType)}
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

      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto p-4 xl:p-5">
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
      {error && !sheetOpen ? <div className="rounded-xl border border-destructive/40 bg-destructive/5 px-4 py-3 text-sm text-destructive">{error}</div> : null}

      {vm && vm.agents.length > 0 ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {vm.agents.map((agent) => (
            <AgentCard
              key={agent.agentType}
              agent={agent}
              diagnosing={diagnosingType === agent.agentType || automaticDiagnosingType === agent.agentType}
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
        <SheetContent className="gap-0 overflow-hidden" resizeStorageKey="agent-management/editor" defaultSize={720} minSize={520} maxSize={960}>
          <SheetHeader className="border-b border-border/60 px-6 py-4">
            <SheetTitle>{editorMode === 'create' ? t('agentManagement.createTitle') : t('agentManagement.editTitle')}</SheetTitle>
            <SheetDescription>{editorMode === 'create' ? t('agentManagement.createDescription') : t('agentManagement.editDescription')}</SheetDescription>
          </SheetHeader>
          <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 py-4">
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
                className="min-h-24"
                value={argsText}
                placeholder={'-y\n@agentclientprotocol/claude-agent-acp@latest'}
                onChange={(event) => setArgsText(event.target.value)}
              />
            </Field>
            <Field label={t('agentManagement.env')} description={t('agentManagement.envDescription')}>
              <ConfigTextarea
                className="min-h-28"
                value={envText}
                placeholder={'ANTHROPIC_API_KEY=...\nNODE_OPTIONS=--max-old-space-size=4096'}
                onChange={(event) => setEnvText(event.target.value)}
              />
            </Field>
            <Field label={t('agentManagement.primaryAgentDir')} description={t('agentManagement.primaryAgentDirDescription')}>
              <TextInput
                value={form.primaryAgentDir}
                placeholder={t('agentManagement.primaryAgentDirPlaceholder')}
                onChange={(event: ChangeEvent<HTMLInputElement>) => setForm((current) => ({ ...current, primaryAgentDir: event.target.value }))}
              />
            </Field>
            <Field label={t('agentManagement.compatibleAgentDirs')} description={t('agentManagement.compatibleAgentDirsDescription')}>
              <ConfigTextarea
                className="min-h-20"
                value={compatibleAgentDirsText}
                placeholder={t('agentManagement.compatibleAgentDirsPlaceholder')}
                onChange={(event) => setCompatibleAgentDirsText(event.target.value)}
              />
            </Field>
            <div className="flex items-center justify-between gap-5 rounded-xl border border-border/60 bg-muted/10 px-4 py-3">
              <div className="min-w-0 space-y-1">
                <ExternalSessionSyncHeading
                  label={t('agentManagement.externalSessionSync')}
                  betaLabel={t('agentManagement.externalSessionSyncBeta')}
                  helpLabel={t('agentManagement.externalSessionSyncHelpLabel')}
                  helpText={t('agentManagement.externalSessionSyncHelp')}
                />
                <div className="text-xs leading-5 text-muted-foreground">{t('agentManagement.externalSessionSyncDescription')}</div>
              </div>
              <Switch
                id="external-session-sync"
                checked={form.externalSessionSyncEnabled}
                onCheckedChange={(checked) => setForm((current) => ({ ...current, externalSessionSyncEnabled: checked }))}
              />
            </div>
            {error ? <div className="rounded-lg border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive">{error}</div> : null}
            <div className="flex justify-end gap-2 pt-1">
              <Button variant="outline" onClick={() => setSheetOpen(false)}>{t('common.close')}</Button>
              <Button disabled={saving || !selectedType.trim() || !form.displayName.trim() || !form.command.trim() || !form.primaryAgentDir.trim() || !hasFormChanges} onClick={() => void submit()}>{t('common.save')}</Button>
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
      </div>
    </Page>
  );
}

function AgentCard({ agent, diagnosing, onEdit, onDelete, onDoctor }: { agent: ManagedAgentVm; diagnosing: boolean; onEdit: () => void; onDelete: () => void; onDoctor: () => void }) {
  const { t } = useTranslation();
  const diagnostic = agent.diagnostic;
  return (
    <AppCard className="h-full gap-3 px-4 py-4 sm:px-4">
      <div className="flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-3">
          <span className="grid size-10 shrink-0 place-items-center rounded-xl border border-border/60 bg-background">
            <img src={agentIconSrc(agent.iconKey)} alt="" className="size-6 object-contain" />
          </span>
          <div className="min-w-0 space-y-1">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="truncate text-sm font-semibold text-foreground">{agent.displayName}</h3>
              <Badge variant="secondary" className="rounded-full px-2 py-0 text-[11px]">{agent.agentType}</Badge>
            </div>
            <div className="min-h-10 overflow-hidden font-mono text-[11px] leading-5 text-muted-foreground [display:-webkit-box] [-webkit-box-orient:vertical] [-webkit-line-clamp:2]">{agent.command} {agent.args.join(' ')}</div>
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <DiagnosticBadge diagnostic={diagnostic} />
          {diagnostic?.status === 'unhealthy' ? <RegistryHelp reason={diagnostic.reason} /> : null}
        </div>
      </div>
      <div className="grid gap-2 text-sm text-muted-foreground sm:grid-cols-2">
        {buildAgentCardSummary(agent, t).map((item) => (
          <Info key={item.key} label={item.label} value={item.value} mono={item.mono} />
        ))}
      </div>
      <div className="mt-auto flex flex-wrap justify-end gap-2 pt-1">
        <Button size="sm" variant="outline" disabled={diagnosing} aria-busy={diagnosing} onClick={onDoctor}>
          {diagnosing ? <LoaderCircle className="animate-spin" /> : <Stethoscope />}
          {diagnosing ? t('agentManagement.diagnosing') : t('agentManagement.diagnose')}
        </Button>
        <Button size="sm" variant="outline" disabled={diagnosing} onClick={onEdit}><Pencil />{t('agentManagement.edit')}</Button>
        <Button size="sm" variant="outline" disabled={diagnosing} onClick={onDelete}><Trash2 />{t('agentManagement.delete')}</Button>
      </div>
    </AppCard>
  );
}

function RegistryHelp({ reason }: { reason?: string | null }) {
  const { t } = useTranslation();
  const openRegistry = async () => {
    try {
      await openUrl(ACP_REGISTRY_URL);
    } catch {
      window.open(ACP_REGISTRY_URL, '_blank', 'noopener,noreferrer');
    }
  };
  const openRegistryLink = (event: React.MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    void openRegistry();
  };
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button type="button" variant="ghost" size="icon" className="size-7 rounded-full text-muted-foreground hover:text-foreground" aria-label={t('agentManagement.registryHelpLabel')}>
            <CircleHelp className="size-4" />
          </Button>
        </TooltipTrigger>
        <TooltipContent side="left" sideOffset={8} className="w-56 space-y-1.5 whitespace-pre-wrap break-words px-2.5 py-2 text-[12px] leading-[1.45]">
          {reason ? (
            <div className="w-full space-y-1">
              <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{t('status.error')}</div>
              <div className="whitespace-pre-wrap break-words [text-wrap:wrap]">{reason}</div>
            </div>
          ) : null}
          <div className="w-full space-y-1 border-t border-border/60 pt-3">
            <div className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{t('agentManagement.registryHelpLabel')}</div>
            <Trans
              i18nKey="agentManagement.registryHelp"
              components={{
                registry: (
                  <a
                    className="font-medium text-primary underline-offset-4 hover:underline"
                    href={ACP_REGISTRY_URL}
                    onClick={openRegistryLink}
                  />
                ),
              }}
            />
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
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
    <div className="min-h-[84px] rounded-xl border border-border/60 bg-muted/10 px-3 py-2.5">
      <div className="text-[10px] uppercase tracking-[0.12em] text-muted-foreground">{label}</div>
      <div className={cn('mt-1 min-w-0 overflow-hidden text-[13px] leading-5 text-foreground [display:-webkit-box] [-webkit-box-orient:vertical] [-webkit-line-clamp:2]', mono && 'font-mono text-[11px]')}>{value}</div>
    </div>
  );
}

export function ExternalSessionSyncHeading({
  label,
  betaLabel,
  helpLabel,
  helpText,
}: {
  label: string;
  betaLabel: string;
  helpLabel: string;
  helpText: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <label htmlFor="external-session-sync" className="text-sm font-semibold text-foreground">{label}</label>
      <Badge variant="secondary" className="h-5 rounded-full px-1.5 py-0 text-[10px] font-semibold uppercase tracking-wide">
        {betaLabel}
      </Badge>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="size-5 rounded-full text-muted-foreground hover:text-foreground"
              aria-label={helpLabel}
            >
              <CircleHelp className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="top" sideOffset={6} className="max-w-64 text-xs leading-5">
            {helpText}
          </TooltipContent>
        </Tooltip>
      </TooltipProvider>
    </div>
  );
}

export function buildAgentCardSummary(agent: ManagedAgentVm, t: (key: string) => string) {
  return [
    { key: 'command', label: t('agentManagement.command'), value: agent.command, mono: true },
    { key: 'args', label: t('agentManagement.args'), value: agent.args.length > 0 ? agent.args.join(' ') : '-', mono: true },
    { key: 'env', label: t('agentManagement.env'), value: agent.env.length > 0 ? `${agent.env.length} ${t('agentManagement.entries')}` : '-', mono: false },
    { key: 'lastChecked', label: t('agentManagement.lastChecked'), value: formatLocalDateTime(agent.diagnostic?.checkedAt), mono: false },
  ];
}

function agentInputFromVm(agent: ManagedAgentVm): ManagedAgentInput {
  return {
    displayName: agent.displayName,
    command: agent.command,
    args: agent.args,
    env: Object.fromEntries(agent.env.map((entry) => [entry.key, entry.value])),
    primaryAgentDir: agent.primaryAgentDir,
    compatibleAgentDirs: agent.compatibleAgentDirs,
    externalSessionSyncEnabled: agent.externalSessionSyncEnabled,
  };
}

export function buildAgentInput(
  form: ManagedAgentInput,
  argsText: string,
  envText: string,
  compatibleAgentDirsText = formatAgentDirs(form.compatibleAgentDirs),
): ManagedAgentInput {
  return {
    displayName: form.displayName,
    command: form.command.trim(),
    args: parseArgs(argsText),
    env: parseEnv(envText),
    primaryAgentDir: form.primaryAgentDir.trim(),
    compatibleAgentDirs: parseAgentDirs(compatibleAgentDirsText, form.primaryAgentDir),
    externalSessionSyncEnabled: form.externalSessionSyncEnabled,
  };
}

export function hasManagedAgentInputChanged(initial: ManagedAgentInput, current: ManagedAgentInput): boolean {
  return managedAgentInputFingerprint(initial) !== managedAgentInputFingerprint(current);
}

function managedAgentInputFingerprint(input: ManagedAgentInput): string {
  return JSON.stringify({
    displayName: input.displayName,
    command: input.command.trim(),
    args: input.args,
    env: Object.entries(input.env).sort(([left], [right]) => left.localeCompare(right)),
    primaryAgentDir: input.primaryAgentDir.trim(),
    compatibleAgentDirs: [...new Set(input.compatibleAgentDirs.map((directory) => directory.trim()).filter(Boolean))]
      .filter((directory) => directory !== input.primaryAgentDir.trim()),
    externalSessionSyncEnabled: input.externalSessionSyncEnabled,
  });
}

function formatArgs(args: string[]) {
  return args.join('\n');
}

function formatEnv(env: ManagedAgentVm['env']) {
  return env.map((entry) => `${entry.key}=${entry.value}`).join('\n');
}

function formatAgentDirs(directories: string[]) {
  return directories.join('\n');
}

function parseAgentDirs(value: string, primaryAgentDir: string) {
  const primary = primaryAgentDir.trim();
  return [...new Set(value.split(/\r?\n|,/).map((item) => item.trim()).filter(Boolean))]
    .filter((directory) => directory !== primary);
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

function DiagnosticBadge({ diagnostic }: { diagnostic?: ManagedAgentVm['diagnostic'] }) {
  const { t } = useTranslation();
  const status = diagnostic?.status ?? 'unknown';
  const icon = status === 'healthy'
    ? <CheckCircle2 className="size-4 text-gold-success" />
    : status === 'unhealthy'
      ? <AlertTriangle className="size-4 text-destructive" />
      : <CircleHelp className="size-4 text-muted-foreground" />;
  return <Badge variant="outline" className="rounded-full px-2 py-0 text-[11px]">{icon}<span className="ml-1">{t(`agentManagement.status.${status}`)}</span></Badge>;
}

function agentIconSrc(iconKey: string) {
  return `/agent-icons/${iconKey}.svg`;
}
