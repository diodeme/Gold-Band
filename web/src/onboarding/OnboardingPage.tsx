import { useCallback, useEffect, useReducer, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertCircle, ArrowLeft, ArrowRight, Check, CircleHelp, ExternalLink, Loader2, Moon, RefreshCw, Sun, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { AgentIdentityLabel } from '@/components/AgentIdentityLabel';
import { DEFAULT_AGENT_ICON_KEY } from '@/lib/agent-icons';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { AcpModelThoughtSelects, updateAcpConfigOptionOverride } from '@/components/acp/AcpModelThoughtSelects';
import { AcpSingleConfigMenu } from '@/components/acp/AcpSingleConfigMenu';
import { acpCompositeConfigSections, acpShowsModelConfigSelect, hasAuthoringModelBoundCatalog, projectAuthoringConfigOverrides, rememberAcpModelBoundOverrides, switchAcpModelBoundOverrides } from '@/lib/acp-composite-config';
import { openExternalUrl } from '@/api';
import { isTauriRuntime } from '@/api/shared';
import { DESKTOP_LANGUAGE_OPTIONS } from '@/languages';
import { loadI18nLanguage } from '@/i18n';
import { applyAppearance } from '@/theme';
import type { AppErrorVm, DesktopLanguage } from '@/types';
import type { EnvironmentCheck, SetupApi, SetupApply, SetupEvent, SetupPreview, SetupSelection, SetupStatus } from './types';
import { initialDetectionState, setupDetectionReducer, setupFailure } from './detection-state';
import { EnvironmentCheckResult } from './EnvironmentCheckResult';
import { UnavailableAgents } from './UnavailableAgents';

const STEP_KEYS = ['git', 'runtime', 'agent'] as const;

export function OnboardingPage({ api, status, onDone, onExit, initialStep = 0 }: { api: SetupApi; status: SetupStatus; onDone: () => Promise<void>; onExit: () => void; initialStep?: number }) {
  const { t } = useTranslation();
  const [step, setStep] = useState(initialStep);
  const [detection, dispatch] = useReducer(setupDetectionReducer, initialDetectionState);
  const { environment: environmentState, agents, unavailable, pending, generation, nextCursor } = detection;
  const { report: environment, progress: environmentProgress } = environmentState;
  const checking = environmentState.phase === 'checking';
  const persistedGitDecision = status.state.status === 'pending' || status.state.status === 'deferred' ? status.state.gitDecision : 'undecided';
  const [gitSkipped, setGitSkipped] = useState(persistedGitDecision === 'skip');
  const environmentReady = environmentState.phase === 'ready' && Boolean(environment);
  const environmentFailed = environmentState.phase === 'failed';
  const runtimeWarning = environmentReady && Boolean(environment?.checks.some((check) => check.tool !== 'git' && check.status !== 'ready'));
  const npxUnavailable = environmentReady && environment?.npxLaunchable === false;
  const detecting = detection.phase === 'detecting';
  const [selectedId, setSelectedId] = useState(status.defaultExecutionProfile?.agentId ?? '');
  const [modelId, setModelId] = useState(status.defaultExecutionProfile?.modelSelection.type === 'explicit' ? status.defaultExecutionProfile.modelSelection.modelId : '');
  const [modelConfig, setModelConfig] = useState({ configOptions: status.defaultExecutionProfile?.configOptions ?? {}, modelBoundOverrides: status.defaultExecutionProfile?.modelBoundOverrides ?? {} });
  const [permission, setPermission] = useState('');
  const [modelBusy, setModelBusy] = useState(false);
  const [preview, setPreview] = useState<SetupPreview>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<AppErrorVm | null>(null);
  const [language, setLanguage] = useState(status.language);
  const [appearance, setAppearance] = useState(status.appearance);
  const [preferencesBusy, setPreferencesBusy] = useState(false);
  const sequence = useRef(0);
  const modelSequence = useRef(0);
  const activeGeneration = useRef('');
  const environmentRequest = useRef<{ id: string; pending: boolean } | null>(null);
  const environmentRefreshQueued = useRef(false);
  const request = useRef<SetupApply | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const stop = useCallback(() => {
    sequence.current += 1; modelSequence.current += 1; setModelBusy(false);
    const previous = activeGeneration.current;
    activeGeneration.current = '';
    dispatch({ type: 'detection-stopped' });
    if (previous) void api.cancel(previous).catch(() => {});
  }, [api]);
  const reportError = useCallback((error: unknown) => {
    const failure = setupFailure(error);
    if (failure.code === 'setup.environment-check-required') {
      stop(); dispatch({ type: 'environment-failed' }); setPreview(undefined); request.current = null;
    }
    setError(failure);
  }, [stop]);
  const invalidatePreview = () => { setPreview(undefined); request.current = null; };

  const checkEnvironment = useCallback(async function runEnvironmentCheck() {
    if (environmentRequest.current?.pending) { environmentRefreshQueued.current = true; return; }
    stop(); setPreview(undefined); request.current = null;
    const id = crypto.randomUUID();
    environmentRequest.current = { id, pending: true };
    dispatch({ type: 'environment-started' }); setError(null);
    try {
      const result = await api.environment(id, (check) => {
        if (environmentRequest.current?.id === id) dispatch({ type: 'environment-progress', check });
      });
      if (environmentRequest.current?.id === id) dispatch({ type: 'environment-completed', report: result });
    } catch (error) {
      if (environmentRequest.current?.id === id) { dispatch({ type: 'environment-failed' }); reportError(error); }
    } finally {
      if (environmentRequest.current?.id === id) {
        environmentRequest.current.pending = false;
        if (environmentRefreshQueued.current) { environmentRefreshQueued.current = false; void runEnvironmentCheck(); }
      }
    }
  }, [api, reportError, stop]);
  useEffect(() => {
    void checkEnvironment();
    return () => { const current = environmentRequest.current; environmentRequest.current = null; environmentRefreshQueued.current = false; if (current) void api.cancelEnvironment(current.id).catch(() => {}); };
  }, [api, checkEnvironment]);
  useEffect(() => { heading.current?.focus(); }, [step]);

  const start = useCallback(async (after?: string) => {
    stop();
    const current = sequence.current;
    dispatch({ type: 'detection-started', append: Boolean(after) }); setPreview(undefined); request.current = null; setError(null);
    const receive = (event: SetupEvent) => {
      if (sequence.current !== current) return;
      if (activeGeneration.current && activeGeneration.current !== event.generation) return;
      activeGeneration.current = event.generation;
      dispatch({ type: 'event', event });
      if (event.type === 'failed') reportError(event);
    };
    try {
      const id = await api.start(receive, after);
      if (sequence.current !== current) { await api.cancel(id); return; }
      activeGeneration.current = id; dispatch({ type: 'detection-accepted', generation: id });
    } catch (error) { if (sequence.current === current) { dispatch({ type: 'detection-failed' }); reportError(error); } }
  }, [api, stop, reportError]);
  useEffect(() => {
    if (step !== 2 || !environmentReady) return;
    void start();
    return stop;
  }, [step, environmentReady, start, stop]);
  useEffect(() => { const leave = () => stop(); window.addEventListener('pagehide', leave); return () => window.removeEventListener('pagehide', leave); }, [stop]);

  const selected = agents[selectedId];
  const models = selected?.supportedModels ?? [];
  const modes = selected?.supportedModes ?? [];
  const configSections = acpCompositeConfigSections(selected?.configOptions, modelId, modelConfig.configOptions, selected?.modelBoundCatalogs);
  const canSelect = Boolean(environmentReady && selected && generation && (detection.phase === 'detecting' || detection.phase === 'completed') && !pending[selectedId] && !modelBusy && (!models.length || models.some((model) => model.id === modelId)) && (!modes.length || modes.some((mode) => mode.id === permission)));
  const selection = (): SetupSelection => {
    const configOptions = Object.fromEntries(configSections.flatMap((section) => section.value ? [[section.id, section.value]] : []));
    return { profile: { agentId: selectedId, modelSelection: models.length ? { type: 'explicit', modelId } : { type: 'provider-default' }, configOptions, modelBoundOverrides: hasAuthoringModelBoundCatalog(selected?.modelBoundCatalogs, modelId) && Object.keys(configOptions).length ? { [modelId]: configOptions } : {} }, permissionModeId: permission || null };
  };
  const selectAgent = (id: string) => {
    modelSequence.current += 1; setModelBusy(false); setSelectedId(id); setModelId(''); setModelConfig({ configOptions: {}, modelBoundOverrides: {} }); invalidatePreview();
    setPermission(agents[id]?.configOptions?.find((option) => option.category === 'mode')?.currentValue ?? '');
  };
  const selectModel = async (id: string) => {
    const current = ++modelSequence.current; const run = sequence.current;
    const switched = switchAcpModelBoundOverrides({ remembered: modelConfig.modelBoundOverrides, previousModelId: modelId, nextModelId: id, currentOverrides: modelConfig.configOptions, configOptions: selected?.configOptions, modelBoundCatalogs: selected?.modelBoundCatalogs });
    setModelConfig({ configOptions: switched.overrides, modelBoundOverrides: switched.remembered });
    setModelId(id); invalidatePreview(); setModelBusy(true); setError(null);
    try {
      const result = await api.probeModel(generation, selectedId, id);
      if (current === modelSequence.current && run === sequence.current) dispatch({ type: 'model', agent: result });
    } catch (error) { if (current === modelSequence.current && run === sequence.current) { setModelId(''); reportError(error); } }
    finally { if (current === modelSequence.current && run === sequence.current) setModelBusy(false); }
  };
  const review = async () => {
    setBusy(true); setError(null);
    try {
      const chosen = selection(); const result = await api.preview(chosen);
      setPreview(result);
      request.current = { selection: chosen, previewFingerprint: result.fingerprint, operationId: crypto.randomUUID(), gitSkipped };
    } catch (error) { reportError(error); } finally { setBusy(false); }
  };
  const apply = async () => {
    if (!request.current || !canSelect) return;
    setBusy(true); setError(null);
    try { await api.apply(request.current); stop(); await onDone(); }
    catch (error) { reportError(error); if (typeof error === 'object' && error && 'code' in error && error.code === 'setup.preview-stale') invalidatePreview(); }
    finally { setBusy(false); }
  };
  const deferSetup = async () => {
    setBusy(true); setError(null);
    try { await api.deferSetup(); stop(); await onDone(); }
    catch (error) { reportError(error); }
    finally { setBusy(false); }
  };
  const preferences = async (nextLanguage: DesktopLanguage, dark?: boolean) => {
    const nextAppearance = dark === undefined ? appearance : { ...appearance, colorScheme: dark ? 'dark' as const : 'light' as const };
    setPreferencesBusy(true);
    try { await api.preferences(nextLanguage, nextAppearance); await loadI18nLanguage(nextLanguage); applyAppearance(nextAppearance); setLanguage(nextLanguage); setAppearance(nextAppearance); }
    catch (error) { reportError(error); } finally { setPreferencesBusy(false); }
  };
  const exit = async () => {
    stop();
    if (status.state.status !== 'pending') { onExit(); return; }
    if (isTauriRuntime()) { const { getCurrentWindow } = await import('@tauri-apps/api/window'); await getCurrentWindow().close(); }
  };
  const gitCheck = environmentProgress.git ?? environment?.checks.find((check) => check.tool === 'git');
  const gitReady = (environmentState.phase === 'ready' || Boolean(environmentProgress.git)) && gitCheck?.status === 'ready';
  const tools: EnvironmentCheck['tool'][] = step === 0 ? ['git'] : ['node', 'npm', 'npx'];
  const message = (failure: string | AppErrorVm) => t(`onboarding.errors.${typeof failure === 'string' ? failure : failure.code}`, { ...(typeof failure === 'string' ? {} : failure.params), defaultValue: t('onboarding.repairHint') });
  const readyCount = Object.keys(agents).length;
  const unavailableAgents = Object.values(unavailable);
  const agentStatus = checking ? 'onboarding.checking' : environmentFailed ? 'onboarding.runtimeBlocked' : detection.phase === 'failed' ? 'onboarding.detectionFailed' : detecting || Object.keys(pending).length ? 'onboarding.discovering' : readyCount ? 'onboarding.discovered' : detection.phase === 'completed' ? (unavailableAgents.length ? 'onboarding.allUnavailable' : 'onboarding.noAgents') : 'onboarding.discovering';
  const agentStepState = detecting ? 'checking' : detection.phase === 'failed' || environmentFailed ? 'failed' : detection.phase === 'completed' ? (readyCount ? 'ready' : 'failed') : 'pending';
  const runtimeStepState = checking ? 'checking' : environmentState.phase === 'idle' ? 'pending' : environmentFailed ? 'failed' : runtimeWarning ? 'warning' : 'ready';
  const stepStates = [gitSkipped ? 'skipped' : gitReady ? 'ready' : checking ? 'checking' : environmentState.phase === 'idle' ? 'pending' : 'failed', runtimeStepState, agentStepState];
  const showDefer = step === 2 && environmentReady && !checking && !detecting && (detection.phase === 'completed' || detection.phase === 'failed');

  return <div className="flex h-dvh min-h-0 flex-col bg-background text-foreground">
    <header data-tauri-drag-region className="flex shrink-0 items-center justify-between gap-3 border-b px-5 py-3">
      <span data-tauri-drag-region className="font-semibold">Gold Band</span>
      <div className="flex items-center gap-2">
        <Select value={language} disabled={preferencesBusy || busy} onValueChange={(value) => void preferences(value as DesktopLanguage)}><SelectTrigger className="w-32" aria-label={t('onboarding.language')}><SelectValue /></SelectTrigger><SelectContent>{DESKTOP_LANGUAGE_OPTIONS.map((item) => <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>)}</SelectContent></Select>
        <Button variant="ghost" size="icon" disabled={preferencesBusy || busy} aria-label={t('onboarding.theme')} onClick={() => void preferences(language, appearance.colorScheme !== 'dark')}>{appearance.colorScheme === 'dark' ? <Sun /> : <Moon />}</Button>
        <Button variant="ghost" size="icon" disabled={busy} aria-label={t('onboarding.exit')} onClick={() => void exit()}><X /></Button>
      </div>
    </header>
    <div className="shrink-0 px-6 pt-5 sm:px-10 sm:pt-7"><div className="mx-auto max-w-3xl">
        <ol className="mb-5 flex flex-wrap items-center gap-3 text-sm text-muted-foreground" aria-label={t('onboarding.progress')}>{STEP_KEYS.map((key, index) => <li key={key} data-state={stepStates[index]} className={`flex items-center gap-2 ${step === index ? 'font-medium text-foreground' : ''}`} aria-current={step === index ? 'step' : undefined}><span className="flex size-6 shrink-0 items-center justify-center rounded-full border text-xs">{stepStates[index] === 'checking' ? <Loader2 className="size-3 animate-spin" /> : stepStates[index] === 'ready' ? <Check className="size-3" /> : stepStates[index] === 'failed' ? <AlertCircle className="size-3 text-destructive" /> : stepStates[index] === 'warning' ? <AlertCircle className="size-3 text-gold-warning" /> : index + 1}</span><span>{t(`onboarding.steps.${key}`)}</span><span className={stepStates[index] === 'skipped' ? 'text-xs' : 'sr-only'}>{t(`onboarding.status.${stepStates[index]}`)}</span>{index < 2 && <span className="ml-1 text-muted-foreground/40">/</span>}</li>)}</ol>
        <h1 ref={heading} tabIndex={-1} className="text-2xl font-semibold tracking-tight outline-none">{t(`onboarding.titles.${STEP_KEYS[step]}`)}</h1>
        <p className="mb-5 mt-3 text-sm leading-6 text-muted-foreground">{t(`onboarding.descriptions.${STEP_KEYS[step]}`)}</p>
    </div></div>
    <main className="min-h-0 flex-1 overflow-y-auto"><div className="mx-auto max-w-3xl px-6 pb-8 sm:px-10">
        {error && <p role="alert" className="mb-5 text-sm text-destructive">{message(error)}</p>}
        {step < 2 ? <>
          <div aria-busy={checking} className="divide-y">
            {tools.map((tool) => {
              const check = environmentProgress[tool] ?? environment?.checks.find((item) => item.tool === tool);
              const waiting = checking && (!environmentProgress[tool] || (tool !== 'git' && check?.status === 'ready'));
              return <EnvironmentCheckResult key={tool} tool={tool} check={check} waiting={waiting} />;
            })}
          </div>
          {step === 0 && <p className="mt-4 text-sm text-muted-foreground">{t('onboarding.gitVersion', { version: status.gitMinimumVersion })}</p>}
          {step === 1 && <p className="mt-4 text-sm text-muted-foreground">{t('onboarding.runtimeVersion', { version: status.environmentPolicy.node.recommendedVersion, ranges: status.environmentPolicy.node.ranges.join(' / ') })}</p>}
          {step === 1 && npxUnavailable && <p role="status" className="mt-4 text-sm text-gold-warning">{t('onboarding.npxUnavailable')}</p>}
          <div className="mt-6 flex flex-wrap gap-2"><Button variant="outline" disabled={checking} onClick={() => void checkEnvironment()}><RefreshCw className="size-4" />{t('onboarding.recheck')}</Button><Button variant="ghost" onClick={() => void openExternalUrl(step === 0 ? status.environmentPolicy.gitHelpUrl : status.environmentPolicy.node.helpUrl)}><ExternalLink className="size-4" />{t('onboarding.installGuide')}</Button></div>
        </> : <>
          <div className="mb-6 flex flex-wrap items-center justify-between gap-3"><p role="status" className="flex items-center gap-2 text-sm text-muted-foreground">{(checking || detecting) && <Loader2 className="size-4 animate-spin" />}{t(agentStatus, { count: readyCount })}</p><Button size="sm" variant={environmentFailed ? 'outline' : 'ghost'} disabled={busy || detecting || checking} onClick={() => void (environmentFailed ? checkEnvironment() : start())}><RefreshCw className="size-4" />{t(environmentFailed || checking ? 'onboarding.recheckRuntime' : 'onboarding.recheck')}</Button></div>
          {showDefer && <p className="mb-6 text-sm text-muted-foreground">{t('onboarding.deferHint')}</p>}
          <div className="space-y-5">
            <label className="block space-y-2 text-sm"><span>{t('onboarding.defaultAgent')}</span><Select value={selected ? selectedId : ''} disabled={busy || !environmentReady || !readyCount} onValueChange={selectAgent}><SelectTrigger aria-label={t('onboarding.defaultAgent')}><SelectValue placeholder={t('onboarding.chooseAgent')} /></SelectTrigger><SelectContent>{Object.values(agents).map((agent) => <SelectItem key={agent.agentType} value={agent.agentType}><AgentIdentityLabel iconKey={agent.iconKey ?? DEFAULT_AGENT_ICON_KEY} name={agent.displayName} /></SelectItem>)}</SelectContent></Select></label>
            {selected && <>
              {acpShowsModelConfigSelect(models, selected.configOptions, modelId, selected.modelBoundCatalogs) && <div className="space-y-2 text-sm"><span>{t('onboarding.defaultModel')}</span><AcpModelThoughtSelects
                models={models} modelValue={modelId} configOptions={selected.configOptions} modelBoundCatalogs={selected.modelBoundCatalogs} configOptionValues={modelConfig.configOptions}
                compact triggerClassName="w-full max-w-none rounded-md" showUnspecifiedModel={false} disabled={busy || modelBusy || !environmentReady || !generation}
                onModelChange={(value) => { if (value) void selectModel(value); }}
                onConfigOptionChange={(optionId, value) => {
                  const current = projectAuthoringConfigOverrides(modelConfig.configOptions, selected.configOptions, selected.modelBoundCatalogs, modelId);
                  const next = updateAcpConfigOptionOverride(current, optionId, value);
                  setModelConfig({ configOptions: next, modelBoundOverrides: rememberAcpModelBoundOverrides(modelConfig.modelBoundOverrides, modelId, next) }); invalidatePreview();
                }}
              /></div>}
              {!models.length && <p className="text-sm text-muted-foreground">{t('onboarding.providerDefault')}</p>}
            </>}
            {modelBusy && <p role="status" className="flex items-center gap-2 text-sm"><Loader2 className="size-4 animate-spin" />{t('onboarding.checkingModel')}</p>}
            {selected && modes.length > 0 && <div className="space-y-2 text-sm"><span>{t('onboarding.permission')}</span><AcpSingleConfigMenu label={t('acp.permissionMode')} value={permission} options={modes} unspecifiedLabel={t('onboarding.choosePermission')} showUnspecified={false} compact triggerClassName="w-full max-w-none rounded-md" disabled={busy || !environmentReady} onValueChange={(value) => { setPermission(value ?? ''); invalidatePreview(); }} /></div>}
          </div>
          <UnavailableAgents agents={unavailableAgents} readyCount={readyCount} environment={environment} recommendedVersion={status.environmentPolicy.node.recommendedVersion} onViewRuntime={() => setStep(1)} />
          {preview && <section className="mt-7 border-t pt-5" aria-label={t('onboarding.preview')}><h2 className="font-medium">{t('onboarding.preview')}</h2><p className="mt-2 text-sm text-muted-foreground">{t('onboarding.previewSummary', { count: preview.templates.length, slots: preview.slotCount })}</p><ul className="mt-3 divide-y">{preview.templates.map((template) => <li className="py-3 text-sm" key={template.id}><div className="flex justify-between gap-3"><span>{template.name}</span><span className="shrink-0 text-muted-foreground">{t('onboarding.slots', { count: template.slotCount })}</span></div>{template.previousBindings.length > 0 && <details className="mt-2 text-muted-foreground"><summary className="cursor-pointer">{t('onboarding.previousBindings')}</summary><ul className="mt-2 space-y-2">{template.previousBindings.map((binding) => <li className="break-words" key={binding.executionSlotId}>{t('onboarding.previousBinding', { slot: binding.executionSlotId, agent: agents[binding.agentId]?.displayName ?? binding.agentId, model: binding.modelId ?? t('onboarding.defaultModelLabel') })}</li>)}</ul></details>}</li>)}</ul></section>}
          {nextCursor && <Button variant="ghost" className="mt-3" disabled={detecting || busy} onClick={() => void start(nextCursor)}>{t('onboarding.loadMore')}</Button>}
        </>}
      </div>
    </main>
    <footer className="flex shrink-0 items-center justify-between gap-3 border-t px-5 py-4 sm:px-10">
      <Button variant="ghost" disabled={busy} onClick={() => step > 0 ? setStep(step - 1) : void openExternalUrl(status.environmentPolicy.gitHelpUrl)}>{step > 0 ? <ArrowLeft className="size-4" /> : <CircleHelp className="size-4" />}{t(step > 0 ? 'onboarding.back' : 'onboarding.help')}</Button>
      <div className="flex items-center gap-2">{step === 0 && !gitReady && <Button variant="ghost" disabled={busy} onClick={async () => { setBusy(true); try { await api.skipGit(); setGitSkipped(true); setStep(1); } catch (error) { reportError(error); } finally { setBusy(false); } }}>{t('onboarding.skipGit')}</Button>}
        {step === 2 && showDefer && <Button variant="ghost" disabled={busy} onClick={() => void deferSetup()}>{busy && <Loader2 className="size-4 animate-spin" />}{t('onboarding.setUpLater')}</Button>}
        {step < 2 ? <Button disabled={step === 0 ? !gitReady && !gitSkipped : !environmentReady} onClick={() => setStep(step + 1)}>{t('onboarding.next')}<ArrowRight className="size-4" /></Button> : <Button disabled={busy || !canSelect} onClick={() => void (preview ? apply() : review())}>{busy && <Loader2 className="size-4 animate-spin" />}{t(preview ? 'onboarding.apply' : 'onboarding.review')}<ArrowRight className="size-4" /></Button>}
      </div>
    </footer>
  </div>;
}
