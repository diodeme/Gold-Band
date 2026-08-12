import { useEffect, useMemo, useRef, useState, type ChangeEvent, type InputHTMLAttributes, type TextareaHTMLAttributes } from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { createAgent, deleteAgent, doctorAgent, updateAgent } from '../api';
import { displayAppError } from '../i18n';
import type { AgentCatalogEntryVm, AgentRegistryVm, ManagedAgentInput, ManagedAgentVm } from '../types';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Page, PageHeader } from '@/components/PageScaffold';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from '@/components/ui/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Textarea } from '@/components/ui/textarea';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { AlertTriangle, Bot, CheckCircle2, CircleHelp, ImagePlus, LoaderCircle, Pencil, Plus, RefreshCw, RotateCcw, Split, Stethoscope, Trash2 } from 'lucide-react';
import { cn } from '@/lib/utils';
import { formatLocalDateTime } from '@/lib/datetime';
import { AGENT_ICON_ACCEPT, DEFAULT_AGENT_ICON_KEY, agentIconClass, agentIconSrc, readAgentIconFile } from '@/lib/agent-icons';

interface AgentManagementPageProps {
  vm: AgentRegistryVm | null;
  loading: boolean;
  onRefresh: () => void;
  onRegistryChange: (vm: AgentRegistryVm) => void;
}

export type AgentEditorContext = {
  mode: 'create' | 'edit';
  source: 'catalog' | 'custom';
  defaultIconKey: string;
  defaultIconLabel: string;
};

export type AgentEditorState = {
  open: boolean;
  context: AgentEditorContext;
  selectedType: string;
  form: ManagedAgentInput;
  argsText: string;
  envText: string;
  compatibleAgentDirsText: string;
  initialEditInput: ManagedAgentInput | null;
};

export type AgentDeleteDialogState = {
  open: boolean;
  target: ManagedAgentVm | null;
};
type Notice = { tone: 'success' | 'error'; message: string };

const ACP_REGISTRY_URL = 'https://agentclientprotocol.com/get-started/registry';
export const agentAddMenuItemClassName = 'rounded-md transition-colors hover:bg-accent hover:text-accent-foreground data-[selected=true]:!bg-transparent data-[selected=true]:!text-foreground data-[selected=true]:hover:!bg-accent data-[selected=true]:hover:!text-accent-foreground';
export const agentEditorSheetPresentation = {
  modal: false,
  showOverlay: false,
} as const;

const defaultForm = (): ManagedAgentInput => ({
  displayName: '',
  icon: DEFAULT_AGENT_ICON_KEY,
  command: '',
  args: [],
  env: {},
  primaryAgentDir: '',
  projectPrimaryAgentDir: null,
  compatibleAgentDirs: [],
  externalSessionSyncSupported: false,
  externalSessionSyncEnabled: false,
});

const defaultEditorState = (): AgentEditorState => ({
  open: false,
  context: {
    mode: 'create',
    source: 'custom',
    defaultIconKey: DEFAULT_AGENT_ICON_KEY,
    defaultIconLabel: 'Gold Band Logo',
  },
  selectedType: '',
  form: defaultForm(),
  argsText: '',
  envText: '',
  compatibleAgentDirsText: '',
  initialEditInput: null,
});

export function closeAgentEditorState(state: AgentEditorState): AgentEditorState {
  return { ...state, open: false };
}

export function closeAgentDeleteDialogState(state: AgentDeleteDialogState): AgentDeleteDialogState {
  return { ...state, open: false };
}

const formFromCatalogAgent = (agentType?: AgentCatalogEntryVm): ManagedAgentInput => agentType ? ({
  displayName: agentType.defaultDisplayName,
  icon: agentType.iconKey,
  command: agentType.defaultCommand,
  args: agentType.defaultArgs,
  env: Object.fromEntries(agentType.defaultEnv.map((entry) => [entry.key, entry.value])),
  primaryAgentDir: agentType.primaryAgentDir,
  projectPrimaryAgentDir: agentType.projectPrimaryAgentDir,
  compatibleAgentDirs: agentType.compatibleAgentDirs,
  externalSessionSyncSupported: agentType.supportsExternalSessionSync,
  externalSessionSyncEnabled: false,
}) : defaultForm();

export function AgentManagementPage({ vm, loading, onRefresh, onRegistryChange }: AgentManagementPageProps) {
  const { t } = useTranslation();
  const [editor, setEditor] = useState<AgentEditorState>(defaultEditorState);
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [diagnosingType, setDiagnosingType] = useState<string | null>(null);
  const [automaticDiagnosingType, setAutomaticDiagnosingType] = useState<string | null>(null);
  const [deleteDialog, setDeleteDialog] = useState<AgentDeleteDialogState>({ open: false, target: null });
  const [notice, setNotice] = useState<Notice | null>(null);
  const [error, setError] = useState<string | null>(null);
  const iconFileInputRef = useRef<HTMLInputElement>(null);

  const catalog = vm?.catalog ?? [];
  const configuredTypes = useMemo(() => new Set(vm?.agents.map((agent) => agent.agentType) ?? []), [vm]);
  const currentInput = useMemo(
    () => buildAgentInput(editor.form, editor.argsText, editor.envText, editor.compatibleAgentDirsText),
    [editor.argsText, editor.compatibleAgentDirsText, editor.envText, editor.form],
  );
  const hasFormChanges = editor.context.mode === 'create'
    || editor.initialEditInput === null
    || hasManagedAgentInputChanged(editor.initialEditInput, currentInput);

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

  const openCreate = (agentType: AgentCatalogEntryVm) => {
    const nextForm = formFromCatalogAgent(agentType);
    setEditor({
      open: true,
      context: {
        mode: 'create',
        source: 'catalog',
        defaultIconKey: agentType.iconKey,
        defaultIconLabel: agentType.label,
      },
      selectedType: agentType.agentType,
      form: nextForm,
      argsText: formatArgs(nextForm.args),
      envText: formatEnv(Object.entries(nextForm.env).map(([key, value]) => ({ key, value }))),
      compatibleAgentDirsText: formatAgentDirs(nextForm.compatibleAgentDirs),
      initialEditInput: null,
    });
    setError(null);
    setAddMenuOpen(false);
  };

  const openCustomCreate = () => {
    const nextForm = defaultForm();
    setEditor({
      open: true,
      context: {
        mode: 'create',
        source: 'custom',
        defaultIconKey: DEFAULT_AGENT_ICON_KEY,
        defaultIconLabel: 'Gold Band Logo',
      },
      selectedType: '',
      form: nextForm,
      argsText: '',
      envText: '',
      compatibleAgentDirsText: '',
      initialEditInput: null,
    });
    setError(null);
    setAddMenuOpen(false);
  };

  const openEdit = (agent: ManagedAgentVm) => {
    const catalogAgent = catalog.find((entry) => entry.agentType === agent.agentType);
    const nextForm = agentInputFromVm(agent);
    const nextArgsText = formatArgs(agent.args);
    const nextEnvText = formatEnv(agent.env);
    const nextCompatibleAgentDirsText = formatAgentDirs(agent.compatibleAgentDirs);
    setEditor({
      open: true,
      context: {
        mode: 'edit',
        source: catalogAgent ? 'catalog' : 'custom',
        defaultIconKey: catalogAgent?.iconKey ?? DEFAULT_AGENT_ICON_KEY,
        defaultIconLabel: catalogAgent?.label ?? 'Gold Band Logo',
      },
      selectedType: agent.agentType,
      form: nextForm,
      argsText: nextArgsText,
      envText: nextEnvText,
      compatibleAgentDirsText: nextCompatibleAgentDirsText,
      initialEditInput: buildAgentInput(nextForm, nextArgsText, nextEnvText, nextCompatibleAgentDirsText),
    });
    setError(null);
  };

  const selectLocalIcon = async (file?: File) => {
    if (!file) return;
    setError(null);
    try {
      const icon = await readAgentIconFile(file);
      setEditor((current) => ({ ...current, form: { ...current.form, icon } }));
    } catch (nextError) {
      const code = nextError instanceof Error ? nextError.message : 'agent-icon.invalid-image-data';
      setError(t(`agentManagement.iconErrors.${code}`, { defaultValue: t('agentManagement.iconErrors.fallback') }));
    } finally {
      if (iconFileInputRef.current) iconFileInputRef.current.value = '';
    }
  };

  const submit = async () => {
    const agentType = editor.selectedType;
    if (!agentType.trim()) {
      setError(t('agentManagement.agentTypeRequired'));
      return;
    }
    if (editor.context.mode === 'edit' && editor.initialEditInput && !hasManagedAgentInputChanged(editor.initialEditInput, currentInput)) {
      return;
    }
    setSaving(true);
    setError(null);
    try {
      const next = editor.context.mode === 'create'
        ? await createAgent(agentType, currentInput)
        : await updateAgent(agentType, currentInput);
      onRegistryChange(next);
      setAutomaticDiagnosingType(agentType);
      setNotice({ tone: 'success', message: t('agentManagement.savedAndDiagnosing') });
      window.setTimeout(onRefresh, 250);
      setEditor(closeAgentEditorState);
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
    const target = deleteDialog.target;
    if (!target) return;
    try {
      onRegistryChange(await deleteAgent(target.agentType));
      setDeleteDialog(closeAgentDeleteDialogState);
    } catch (nextError) {
      setError(displayAppError(t, nextError));
      setDeleteDialog(closeAgentDeleteDialogState);
    }
  };

  return (
    <Page flush className="flex flex-col">
      <PageHeader
        variant="integrated"
        icon={<Bot />}
        title={<span className="text-title">{t('agentManagement.title')}</span>}
        actions={(
          <>
            <Button variant="outline" size="sm" disabled={loading} onClick={onRefresh}>
              <RefreshCw className={cn(loading && 'animate-spin')} />
              {t('common.refresh')}
            </Button>
            <Popover open={addMenuOpen} onOpenChange={setAddMenuOpen}>
              <PopoverTrigger asChild>
                <Button size="sm">
                  <Plus />
                  {t('agentManagement.addAgent')}
                </Button>
              </PopoverTrigger>
              <PopoverContent align="end" className="w-80 p-0">
                <Command>
                  <CommandInput placeholder={t('agentManagement.searchAgents')} />
                  <CommandList>
                    <CommandEmpty>{t('agentManagement.noMatchingAgents')}</CommandEmpty>
                    <CommandGroup>
                      <CommandItem value={t('agentManagement.customAgent')} className={agentAddMenuItemClassName} onSelect={openCustomCreate}>
                        <Bot className="size-4" />
                        <span>{t('agentManagement.customAgent')}</span>
                      </CommandItem>
                    </CommandGroup>
                    <CommandSeparator />
                    <CommandGroup>
                      {catalog.map((agentType) => (
                        <CommandItem
                          key={agentType.agentType}
                          value={`${agentType.label} ${agentType.agentType}`}
                          className={agentAddMenuItemClassName}
                          disabled={agentType.configured}
                          onSelect={() => openCreate(agentType)}
                        >
                          <img src={agentIconSrc(agentType.iconKey)} alt="" className={agentIconClass(agentType.iconKey, 'size-4')} />
                          <span className="min-w-0 flex-1 truncate">{agentType.label}</span>
                          {agentType.configured ? <Badge variant="secondary">{t('agentManagement.configured')}</Badge> : null}
                        </CommandItem>
                      ))}
                    </CommandGroup>
                  </CommandList>
                </Command>
              </PopoverContent>
            </Popover>
          </>
        )}
      />

      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 pb-6 pt-4">
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
      {error && !editor.open ? <div className="rounded-xl border border-destructive/40 bg-destructive/5 px-4 py-3 text-sm text-destructive">{error}</div> : null}

      {vm && vm.agents.length > 0 ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {vm.agents.map((agent) => (
            <AgentCard
              key={agent.agentType}
              agent={agent}
              diagnosing={diagnosingType === agent.agentType || automaticDiagnosingType === agent.agentType}
              onEdit={() => openEdit(agent)}
              onDelete={() => setDeleteDialog({ open: true, target: agent })}
              onDoctor={() => void runDoctor(agent.agentType)}
            />
          ))}
        </div>
      ) : (
        <AppCard>
          <EmptyState>{loading ? t('common.loading') : t('agentManagement.empty')}</EmptyState>
        </AppCard>
      )}

      <Sheet modal={agentEditorSheetPresentation.modal} open={editor.open} onOpenChange={(open) => {
        if (!open) setEditor(closeAgentEditorState);
      }}>
        <SheetContent showOverlay={agentEditorSheetPresentation.showOverlay} className="gap-0 overflow-hidden" resizeStorageKey="agent-management/editor" defaultSize={720} minSize={520} maxSize={960}>
          <SheetHeader className="border-b border-border/60 px-6 py-4">
            <SheetTitle>{editor.context.mode === 'create' ? t('agentManagement.createTitle') : t('agentManagement.editTitle')}</SheetTitle>
            <SheetDescription>{editor.context.mode === 'create' ? t('agentManagement.createDescription') : t('agentManagement.editDescription')}</SheetDescription>
          </SheetHeader>
          <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-6 py-4">
            <Field label={t('agentManagement.agentId')} description={t('agentManagement.agentIdDescription')}>
              <AgentIdInput
                value={editor.selectedType}
                disabled={!isAgentIdEditable(editor.context)}
                placeholder={t('agentManagement.customAgentIdPlaceholder')}
                onValueChange={(selectedType) => setEditor((current) => ({ ...current, selectedType }))}
              />
            </Field>
            <Field label={t('agentManagement.displayName')}>
              <TextInput value={editor.form.displayName} onChange={(event: ChangeEvent<HTMLInputElement>) => setEditor((current) => ({ ...current, form: { ...current.form, displayName: event.target.value } }))} />
            </Field>
            <Field label={t('agentManagement.command')}>
              <TextInput value={editor.form.command} onChange={(event: ChangeEvent<HTMLInputElement>) => setEditor((current) => ({ ...current, form: { ...current.form, command: event.target.value } }))} />
            </Field>
            <Field label={t('agentManagement.args')} description={t('agentManagement.argsDescription')}>
              <ConfigTextarea
                className="min-h-24"
                value={editor.argsText}
                placeholder={'-y\n@agentclientprotocol/claude-agent-acp@latest'}
                onChange={(event) => setEditor((current) => ({ ...current, argsText: event.target.value }))}
              />
            </Field>
            <Field label={t('agentManagement.env')} description={t('agentManagement.envDescription')}>
              <ConfigTextarea
                className="min-h-28"
                value={editor.envText}
                placeholder={'ANTHROPIC_API_KEY=...\nNODE_OPTIONS=--max-old-space-size=4096'}
                onChange={(event) => setEditor((current) => ({ ...current, envText: event.target.value }))}
              />
            </Field>
            <FieldActionGroup
              label={t('agentManagement.icon')}
              description={editor.context.source === 'catalog'
                ? t('agentManagement.catalogIconDescription', { agent: editor.context.defaultIconLabel })
                : t('agentManagement.iconDescription')}
            >
              <div className="flex items-center gap-3">
                <span className="grid size-10 shrink-0 place-items-center rounded-xl border border-border/60 bg-background">
                  <img src={agentIconSrc(editor.form.icon)} alt="" className={agentIconClass(editor.form.icon, 'size-6')} />
                </span>
                <Button
                  type="button"
                  variant="ghost"
                  className="shrink-0 bg-transparent"
                  onClick={() => iconFileInputRef.current?.click()}
                >
                  <ImagePlus />
                  {t('agentManagement.selectLocalIcon')}
                </Button>
                {editor.form.icon.trim() !== editor.context.defaultIconKey ? (
                  <Button
                    type="button"
                    variant="ghost"
                    className="shrink-0 bg-transparent"
                    onClick={() => setEditor((current) => ({ ...current, form: { ...current.form, icon: current.context.defaultIconKey } }))}
                  >
                    <RotateCcw />
                    {t('agentManagement.useDefaultIcon')}
                  </Button>
                ) : null}
                <input
                  ref={iconFileInputRef}
                  type="file"
                  accept={AGENT_ICON_ACCEPT}
                  className="hidden"
                  aria-label={t('agentManagement.selectLocalIcon')}
                  onChange={(event) => void selectLocalIcon(event.target.files?.[0])}
                />
              </div>
            </FieldActionGroup>
            <FieldActionGroup
              label={t('agentManagement.primaryAgentDir')}
              description={t('agentManagement.primaryAgentDirDescription')}
              action={(
                <TooltipProvider>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        type="button"
                        variant={editor.form.projectPrimaryAgentDir === null ? 'ghost' : 'secondary'}
                        size="icon"
                        className={cn(
                          'size-8 shrink-0 rounded-md',
                          editor.form.projectPrimaryAgentDir !== null && 'text-primary ring-1 ring-primary/25',
                        )}
                        aria-label={t('agentManagement.splitPrimaryAgentDirs')}
                        aria-pressed={editor.form.projectPrimaryAgentDir !== null}
                        onClick={() => setEditor((current) => ({
                          ...current,
                          form: {
                            ...current.form,
                            projectPrimaryAgentDir: current.form.projectPrimaryAgentDir === null
                              ? current.form.primaryAgentDir
                              : null,
                          },
                        }))}
                      >
                        <Split className="size-4" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side="top" sideOffset={6} className="text-xs">
                      {t('agentManagement.splitPrimaryAgentDirs')}
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              )}
            >
              {editor.form.projectPrimaryAgentDir === null ? (
                <TextInput
                  value={editor.form.primaryAgentDir}
                  placeholder={t('agentManagement.primaryAgentDirPlaceholder')}
                  onChange={(event: ChangeEvent<HTMLInputElement>) => setEditor((current) => ({ ...current, form: { ...current.form, primaryAgentDir: event.target.value } }))}
                />
              ) : (
                <div className="grid gap-3 sm:grid-cols-2">
                  <label className="space-y-1.5">
                    <span className="text-xs font-medium text-muted-foreground">{t('agentManagement.globalPrimaryAgentDir')}</span>
                    <TextInput
                      value={editor.form.primaryAgentDir}
                      placeholder={t('agentManagement.globalPrimaryAgentDirPlaceholder')}
                      onChange={(event: ChangeEvent<HTMLInputElement>) => setEditor((current) => ({ ...current, form: { ...current.form, primaryAgentDir: event.target.value } }))}
                    />
                  </label>
                  <label className="space-y-1.5">
                    <span className="text-xs font-medium text-muted-foreground">{t('agentManagement.projectPrimaryAgentDir')}</span>
                    <TextInput
                      value={editor.form.projectPrimaryAgentDir}
                      placeholder={t('agentManagement.projectPrimaryAgentDirPlaceholder')}
                      onChange={(event: ChangeEvent<HTMLInputElement>) => setEditor((current) => ({ ...current, form: { ...current.form, projectPrimaryAgentDir: event.target.value } }))}
                    />
                  </label>
                </div>
              )}
            </FieldActionGroup>
            <Field label={t('agentManagement.compatibleAgentDirs')} description={t('agentManagement.compatibleAgentDirsDescription')}>
              <ConfigTextarea
                className="min-h-20"
                value={editor.compatibleAgentDirsText}
                placeholder={t('agentManagement.compatibleAgentDirsPlaceholder')}
                onChange={(event) => setEditor((current) => ({ ...current, compatibleAgentDirsText: event.target.value }))}
              />
            </Field>
            {error ? <div className="rounded-lg border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm text-destructive">{error}</div> : null}
            <div className="flex justify-end gap-2 pt-1">
              <Button variant="outline" onClick={() => setEditor(closeAgentEditorState)}>{t('common.close')}</Button>
              <Button disabled={saving || !editor.selectedType.trim() || !editor.form.displayName.trim() || !editor.form.command.trim() || !hasFormChanges} onClick={() => void submit()}>{t('common.save')}</Button>
            </div>
          </div>
        </SheetContent>
      </Sheet>

      <AlertDialog open={deleteDialog.open} onOpenChange={(open) => {
        if (!open) setDeleteDialog(closeAgentDeleteDialogState);
      }}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('agentManagement.deleteTitle')}</AlertDialogTitle>
            <AlertDialogDescription>{t('agentManagement.deleteDescription', { agent: deleteDialog.target?.displayName ?? deleteDialog.target?.agentType ?? '' })}</AlertDialogDescription>
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
            <img src={agentIconSrc(agent.iconKey)} alt="" className={agentIconClass(agent.iconKey, 'size-6')} />
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

function FieldActionGroup({ label, description, action, children }: { label: string; description?: string; action?: React.ReactNode; children: React.ReactNode }) {
  return (
    <fieldset className="min-w-0 space-y-2 border-0 p-0">
      <legend className="sr-only">{label}</legend>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 space-y-1">
          <div aria-hidden="true" className="text-sm font-semibold text-foreground">{label}</div>
          {description ? <div className="text-xs text-muted-foreground">{description}</div> : null}
        </div>
        {action}
      </div>
      {children}
    </fieldset>
  );
}

export function AgentIdInput({
  value,
  disabled,
  placeholder,
  onValueChange,
}: {
  value: string;
  disabled: boolean;
  placeholder: string;
  onValueChange: (value: string) => void;
}) {
  const composingRef = useRef(false);
  return (
    <TextInput
      value={value}
      disabled={disabled}
      placeholder={placeholder}
      onChange={(event) => onValueChange(agentIdInputValue(event.target.value, composingRef.current))}
      onCompositionStart={() => {
        composingRef.current = true;
      }}
      onCompositionEnd={(event) => {
        composingRef.current = false;
        onValueChange(agentIdInputValue(event.currentTarget.value, false));
      }}
    />
  );
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
    icon: agent.iconKey,
    command: agent.command,
    args: agent.args,
    env: Object.fromEntries(agent.env.map((entry) => [entry.key, entry.value])),
    primaryAgentDir: agent.primaryAgentDir,
    projectPrimaryAgentDir: agent.projectPrimaryAgentDir,
    compatibleAgentDirs: agent.compatibleAgentDirs,
    externalSessionSyncSupported: agent.externalSessionSyncSupported,
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
    icon: form.icon.trim() || DEFAULT_AGENT_ICON_KEY,
    command: form.command.trim(),
    args: parseArgs(argsText),
    env: parseEnv(envText),
    primaryAgentDir: form.primaryAgentDir.trim(),
    projectPrimaryAgentDir: form.projectPrimaryAgentDir === null
      ? null
      : form.projectPrimaryAgentDir.trim(),
    compatibleAgentDirs: parseAgentDirs(
      compatibleAgentDirsText,
      [form.primaryAgentDir, form.projectPrimaryAgentDir ?? ''],
    ),
    externalSessionSyncSupported: form.externalSessionSyncSupported,
    externalSessionSyncEnabled: form.externalSessionSyncSupported && form.externalSessionSyncEnabled,
  };
}

export function hasManagedAgentInputChanged(initial: ManagedAgentInput, current: ManagedAgentInput): boolean {
  return managedAgentInputFingerprint(initial) !== managedAgentInputFingerprint(current);
}

export function isAgentIdEditable(context: Pick<AgentEditorContext, 'mode' | 'source'>): boolean {
  return context.mode === 'create' && context.source === 'custom';
}

export function agentIdInputValue(value: string, composing: boolean): string {
  return composing ? value : value.toLowerCase().replace(/[^a-z0-9-]/g, '');
}

function managedAgentInputFingerprint(input: ManagedAgentInput): string {
  return JSON.stringify({
    displayName: input.displayName,
    icon: input.icon.trim() || DEFAULT_AGENT_ICON_KEY,
    command: input.command.trim(),
    args: input.args,
    env: Object.entries(input.env).sort(([left], [right]) => left.localeCompare(right)),
    primaryAgentDir: input.primaryAgentDir.trim(),
    projectPrimaryAgentDir: input.projectPrimaryAgentDir === null
      ? null
      : input.projectPrimaryAgentDir.trim(),
    compatibleAgentDirs: [...new Set(input.compatibleAgentDirs.map((directory) => directory.trim()).filter(Boolean))]
      .filter((directory) => ![input.primaryAgentDir.trim(), input.projectPrimaryAgentDir?.trim()].includes(directory)),
    externalSessionSyncSupported: input.externalSessionSyncSupported,
    externalSessionSyncEnabled: input.externalSessionSyncSupported && input.externalSessionSyncEnabled,
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

function parseAgentDirs(value: string, primaryAgentDirs: string[]) {
  const primary = new Set(primaryAgentDirs.map((directory) => directory.trim()).filter(Boolean));
  return [...new Set(value.split(/\r?\n|,/).map((item) => item.trim()).filter(Boolean))]
    .filter((directory) => !primary.has(directory));
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
