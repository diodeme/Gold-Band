/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { OnboardingPage } from '@/onboarding/OnboardingPage';
import { OnboardingGate } from '@/onboarding/OnboardingGate';
import { createBrowserSetupApi } from '@/onboarding/browser-fixture';
import packagedPolicy from '../../configs/onboarding-environment-policy.json';
import { acpCompositeConfigSections } from '@/lib/acp-composite-config';
import type { EnvironmentCheck, SetupAgent, SetupApi, SetupEvent, SetupStatus } from '@/onboarding/types';
import i18n from '@/i18n';

const mocks = vi.hoisted(() => ({ api: vi.fn(), business: vi.fn() }));
vi.mock('@/api', () => ({ openExternalUrl: vi.fn() }));
vi.mock('@/onboarding/api', () => ({ setupApi: mocks.api }));
vi.mock('@/App', () => ({ App: () => { mocks.business(); return <div>Business application</div>; } }));
vi.mock('@/theme', () => ({ applyAppearance: vi.fn() }));
vi.mock('@/components/AgentIdentityLabel', () => ({ AgentIdentityLabel: ({ name }: { name: string }) => name }));
vi.mock('@/components/ui/select', () => ({
  Select: ({ value, onValueChange, disabled, children }: { value: string; onValueChange: (value: string) => void; disabled: boolean; children: React.ReactNode }) => <select aria-label="choice" value={value} onChange={(event) => onValueChange(event.target.value)} disabled={disabled}><option value="" />{children}</select>,
  SelectTrigger: () => null, SelectValue: () => null,
  SelectContent: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  SelectItem: ({ value, children }: { value: string; children: React.ReactNode }) => <option value={value}>{children}</option>,
}));
vi.mock('@/components/acp/AcpModelThoughtSelects', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/components/acp/AcpModelThoughtSelects')>(),
  AcpModelThoughtSelects: (props: React.ComponentProps<typeof import('@/components/acp/AcpModelThoughtSelects').AcpModelThoughtSelects>) => <>
    <select aria-label="model" value={props.modelValue ?? ''} disabled={props.disabled} onChange={(event) => props.onModelChange(event.target.value || null)}><option value="" />{props.models.map((model) => <option key={model.id} value={model.id}>{model.name}</option>)}</select>
    {acpCompositeConfigSections(props.configOptions, props.modelValue, props.configOptionValues, props.modelBoundCatalogs).map((section) => <label key={section.id}>{section.name}<select aria-label={section.id} value={section.value ?? ''} disabled={props.disabled} onChange={(event) => props.onConfigOptionChange?.(section.id, event.target.value || null)}><option value="" />{section.options.map((option) => <option key={option.value} value={option.value}>{option.name}</option>)}</select></label>)}
  </>,
}));
vi.mock('@/components/acp/AcpSingleConfigMenu', () => ({
  AcpSingleConfigMenu: (props: React.ComponentProps<typeof import('@/components/acp/AcpSingleConfigMenu').AcpSingleConfigMenu>) => <select aria-label="permission" value={props.value ?? ''} disabled={props.disabled} onChange={(event) => props.onValueChange(event.target.value || null)}><option value="" />{props.options.map((mode) => <option key={mode.id} value={mode.id}>{mode.name}</option>)}</select>,
}));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement; let root: Root; let api: SetupApi; let status: SetupStatus;
let receive: (event: SetupEvent) => void;
const done = vi.fn();
const unavailableEvent = (agentId: string, reason: Extract<SetupEvent, { type: 'unavailable' }>['reason']): SetupEvent => ({ type: 'unavailable', generation: 'generation-1', agentId, label: agentId, iconKey: agentId, reason });
const healthyEnvironment = () => ({ npxLaunchable: true, checks: (['git', 'node', 'npm', 'npx'] as const).map((tool) => ({ tool, status: 'ready' as const, version: tool === 'git' ? '2.53.0' : tool === 'node' ? '22.22.3' : '10.9.8', executablePath: null })) });

it('keeps the browser fixture aligned with the packaged npm policy', async () => {
  const fixture = await createBrowserSetupApi().status();
  expect(fixture.environmentPolicy.npm.ranges).toEqual(packagedPolicy.npm.ranges);
});

beforeEach(async () => {
  await i18n.changeLanguage('en');
  host = document.createElement('div'); document.body.append(host); root = createRoot(host);
  api = createBrowserSetupApi(); status = await api.status(); status.language = 'en';
  vi.spyOn(api, 'start').mockImplementation(async (listener) => { receive = listener; return 'generation-1'; });
  vi.spyOn(api, 'cancel'); vi.spyOn(api, 'preview'); vi.spyOn(api, 'apply'); done.mockReset(); mocks.business.mockReset();
  mocks.api.mockResolvedValue(api);
  window.location.hash = '#/onboarding';
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.restoreAllMocks(); });
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((item) => item.textContent?.includes(text))!;
async function click(text: string) { await act(async () => button(text).click()); }
async function render() { await act(async () => root.render(<OnboardingPage api={api} status={status} onDone={done} onExit={() => {}} />)); }
async function thirdStep() { await render(); await click('Next'); await click('Next'); }
async function select(index: number, value: string) {
  await act(async () => { const node = host.querySelectorAll('select')[index]; node.value = value; node.dispatchEvent(new Event('change', { bubbles: true })); });
}
async function selectNamed(label: string, value: string) {
  await act(async () => { const node = host.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!; node.value = value; node.dispatchEvent(new Event('change', { bubbles: true })); });
}
async function ready(id = 'local-agent') {
  await act(async () => receive({ type: 'ready', generation: 'generation-1', agent: { agentType: id, displayName: id, supportedModels: [], supportedModes: [] } }));
}

it('blocks business mounting until pending onboarding is completed, including deep links', async () => {
  window.location.hash = '#/tasks/private';
  await act(async () => root.render(<OnboardingGate />));
  expect(host.textContent).toContain('Prepare Git');
  expect(mocks.business).not.toHaveBeenCalled();
  expect(api.start).not.toHaveBeenCalled();
});

it('shows immediate progress and only warns when Node.js is unavailable', async () => {
  let resolve!: (value: Awaited<ReturnType<SetupApi['environment']>>) => void;
  vi.spyOn(api, 'environment').mockReturnValue(new Promise((done) => { resolve = done; }));
  await render();
  expect(host.textContent).toContain('Checking environment');
  expect(button('Next').disabled).toBe(true);
  await act(async () => resolve({ npxLaunchable: true, checks: [{ tool: 'git', status: 'ready', version: '2.53', executablePath: null }, { tool: 'node', status: 'missing', version: null, executablePath: null }] }));
  await click('Next');
  expect(host.querySelector('[aria-label="Setup progress"]')!.children[1].getAttribute('data-state')).toBe('warning');
  const missing = Array.from(host.querySelectorAll('span')).find((item) => item.textContent === 'Not installed')!;
  expect(missing.className).toContain('text-gold-warning');
  expect(missing.className).not.toContain('text-destructive');
  expect(button('Next').disabled).toBe(false);
  expect(host.textContent).not.toContain('Skip Git');
  await click('Next');
  expect(api.start).toHaveBeenCalledTimes(1);
});

it('explains that npx Agents are unavailable while allowing Agent detection', async () => {
  vi.spyOn(api, 'environment').mockResolvedValue(limitedEnvironment());
  await render();
  await click('Next');
  const notice = i18n.t('onboarding.npxUnavailable');
  expect(host.textContent).toContain(notice);
  expect(button('Next').disabled).toBe(false);
  await click('Next');
  expect(api.start).toHaveBeenCalledTimes(1);
  expect(host.textContent).not.toContain(notice);
});

it('shows the Node.js runtime actually used by npm and npx', async () => {
  vi.spyOn(api, 'environment').mockResolvedValue({ npxLaunchable: true, checks: [
    { tool: 'git', status: 'ready', version: '2.53.0', executablePath: null },
    { tool: 'node', status: 'ready', version: '24.22.0', executablePath: 'C:\\Node\\node.exe' },
    { tool: 'npx', status: 'incompatible', version: '11.19.1', executablePath: 'C:\\Node\\npx.cmd', nodeExecutablePath: 'D:\\Other Node\\node.exe', nodeVersion: '22.10.0', error: { code: 'setup.runtime-incompatible', params: { reason: 'node-mismatch', expectedNodePath: 'C:\\Node\\node.exe', actualNodePath: 'D:\\Other Node\\node.exe' } } },
  ] });
  await render(); await click('Next');
  expect(host.textContent).toContain('Node.js in use 22.10.0: D:\\Other Node\\node.exe');
  expect(button('Next').disabled).toBe(false);
});

it('shows completed Git before a slow runtime check finishes', async () => {
  vi.spyOn(api, 'environment').mockImplementation(async (_id, onChecked) => {
    onChecked?.({ tool: 'git', status: 'ready', version: '2.53.0', executablePath: 'C:\\Git\\git.exe' });
    return new Promise(() => {});
  });
  await render();
  expect(host.textContent).toContain('Git 2.53.0');
  expect(button('Next').disabled).toBe(false);
  await click('Next');
  expect(button('Next').disabled).toBe(true);
  expect(mocks.business).not.toHaveBeenCalled();
});

it('restores a deferred Git skip decision for a later setup run', async () => {
  status.state = { status: 'deferred', gitDecision: 'skip' };
  vi.spyOn(api, 'environment').mockResolvedValue({
    npxLaunchable: true,
    checks: [
      { tool: 'git', status: 'missing', version: null, executablePath: null },
      { tool: 'node', status: 'ready', version: '24.22.0', executablePath: null },
      { tool: 'npm', status: 'ready', version: '11.19.1', executablePath: null },
      { tool: 'npx', status: 'ready', version: '11.19.1', executablePath: null },
    ],
  });
  await render();
  expect(button('Next').disabled).toBe(false);
});

it('does not let late preliminary progress overwrite final runtime validation', async () => {
  let progress: ((check: EnvironmentCheck) => void) | undefined;
  vi.spyOn(api, 'environment').mockImplementation(async (_id, onChecked) => {
    progress = onChecked;
    return { npxLaunchable: false, checks: [{ tool: 'git', status: 'ready', version: '2.53.0', executablePath: null }, { tool: 'node', status: 'incompatible', version: '22.22.3', executablePath: null }] };
  });
  await render(); await click('Next');
  await act(async () => progress?.({ tool: 'npm', status: 'ready', version: '99.0.0', executablePath: null }));
  expect(host.textContent).toContain('Node.js 22.22.3');
  expect(host.textContent).not.toContain('npm 99.0.0');
});

it('reuses the completed environment check when entering and returning to Agent detection', async () => {
  const environment = vi.spyOn(api, 'environment');
  vi.mocked(api.start).mockImplementation(async (listener) => { receive = listener; return 'generation-1'; });
  await thirdStep();
  expect(host.textContent).toContain('Checking local Agents');
  expect(host.textContent).not.toContain('Checking environment');
  await ready(); await select(1, 'local-agent');
  expect(button('Preview changes').disabled).toBe(false);
  await click('Back');
  expect(button('Next').disabled).toBe(false);
  await click('Next');
  expect(button('Preview changes').disabled).toBe(true);
  await ready();
  await click('Preview changes'); await click('Apply and finish');
  expect(environment).toHaveBeenCalledTimes(1);
  expect(api.start).toHaveBeenCalledTimes(2);
  expect(api.apply).toHaveBeenCalledTimes(1);
});

it('checks the environment once before direct defaults detection starts', async () => {
  const environment = vi.spyOn(api, 'environment');
  await act(async () => root.render(<OnboardingPage api={api} status={status} onDone={done} onExit={() => {}} initialStep={2} />));
  expect(environment).toHaveBeenCalledTimes(1);
  expect(api.start).toHaveBeenCalledTimes(1);
});

it('does not start direct defaults detection before the initial environment result arrives', async () => {
  let complete!: (report: Awaited<ReturnType<SetupApi['environment']>>) => void;
  vi.spyOn(api, 'environment').mockImplementation(async () => new Promise((resolve) => { complete = resolve; }));
  await act(async () => root.render(<OnboardingPage api={api} status={status} onDone={done} onExit={() => {}} initialStep={2} />));
  expect(host.textContent).toContain('Checking environment');
  expect(api.start).not.toHaveBeenCalled();
  await act(async () => complete(healthyEnvironment()));
  expect(api.start).toHaveBeenCalledTimes(1);
  expect(host.textContent).toContain('Checking local Agents');
});

const limitedEnvironment = () => ({
  npxLaunchable: false,
  checks: [
    { tool: 'git' as const, status: 'ready' as const, version: '2.53.0', executablePath: 'C:\\Git\\git.exe' },
    { tool: 'node' as const, status: 'ready' as const, version: '22.22.3', executablePath: 'C:\\Node\\node.exe' },
    { tool: 'npm' as const, status: 'missing' as const, version: null, executablePath: null },
    { tool: 'npx' as const, status: 'missing' as const, version: null, executablePath: null },
  ],
});

const timedOutNodeEnvironment = () => ({
  npxLaunchable: true,
  checks: [{ tool: 'node' as const, status: 'timeout' as const, version: null, executablePath: 'C:\\Programs with spaces\\node.exe', nodeExecutablePath: null, error: { code: 'setup.probe-timeout', params: { tool: 'node' } } }],
});

it('starts Agent detection on the direct defaults entry even when the runtime has problems', async () => {
  vi.spyOn(api, 'environment').mockResolvedValue(timedOutNodeEnvironment());
  await act(async () => root.render(<OnboardingPage api={api} status={status} onDone={done} onExit={() => {}} initialStep={2} />));
  expect(api.start).toHaveBeenCalledTimes(1);
  expect(host.textContent).toContain('Checking local Agents');
  const runtime = host.querySelector('[aria-label="Setup progress"]')!.children[1];
  expect(runtime.getAttribute('data-state')).toBe('warning');
  await click('Back');
  expect(host.textContent).toContain('Timed out');
  expect(host.textContent).toContain('C:\\Programs with spaces\\node.exe');
  expect(button('Next').disabled).toBe(false);
});

it('does not classify an interrupted Agent scan as an empty result', async () => {
  await thirdStep();
  await act(async () => receive({ type: 'failed', generation: 'generation-1', code: 'setup.probe-failed', params: {} }));
  expect(host.textContent).not.toContain('No Agent is ready');
  expect(host.textContent).toContain('Agent detection failed');
});

it('warns about a stable unverified Node version without blocking a compatible toolchain', async () => {
  const report = {
    npxLaunchable: true,
    checks: [
      { tool: 'git' as const, status: 'ready' as const, version: '2.53.0', executablePath: 'C:\\Git\\git.exe' },
      { tool: 'node' as const, status: 'unverified' as const, version: '23.11.0', executablePath: 'C:\\Node\\node.exe', error: { code: 'setup.runtime-version-unverified', params: { tool: 'node', version: '23.11.0' } } },
      { tool: 'npm' as const, status: 'ready' as const, version: '10.9.8', executablePath: 'C:\\Node\\npm.cmd', nodeExecutablePath: 'C:\\Node\\node.exe' },
      { tool: 'npx' as const, status: 'ready' as const, version: '10.9.8', executablePath: 'C:\\Node\\npx.cmd', nodeExecutablePath: 'C:\\Node\\node.exe' },
    ],
  };
  vi.spyOn(api, 'environment').mockResolvedValue(report);
  await render();
  await click('Next');
  expect(host.textContent).toContain('Node.js 23.11.0');
  const warning = Array.from(host.querySelectorAll('span')).find((item) => item.textContent === 'Unverified version')!;
  expect(warning.className).toContain('text-gold-warning');
  expect(warning.className).not.toContain('text-destructive');
  expect(host.textContent).toContain('outside the currently verified ranges');
  expect(host.textContent).not.toContain('Unsupported version');
  expect(button('Next').disabled).toBe(false);
  await click('Next');
  expect(api.start).toHaveBeenCalledTimes(1);
});

it('rechecks a released runtime report once and resumes Agent detection while rejecting the old run', async () => {
  const environment = vi.spyOn(api, 'environment');
  await thirdStep(); const old = receive;
  await act(async () => receive({ type: 'failed', generation: 'generation-1', code: 'setup.environment-check-required', params: {} }));
  vi.mocked(api.start).mockImplementationOnce(async (listener) => { receive = listener; return 'generation-2'; });
  let complete!: (report: Awaited<ReturnType<SetupApi['environment']>>) => void;
  environment.mockImplementationOnce(async () => new Promise((resolve) => { complete = resolve; }));
  await click('Check runtime again');
  expect(host.textContent).toContain('Checking environment');
  expect(button('Check runtime again').disabled).toBe(true);
  await act(async () => old({ type: 'ready', generation: 'generation-1', agent: { agentType: 'stale', displayName: 'Stale' } }));
  expect(host.textContent).toContain('Checking environment');
  await act(async () => complete(healthyEnvironment()));
  expect(host.textContent).toContain('Checking local Agents');
  await act(async () => receive({ type: 'ready', generation: 'generation-2', agent: { agentType: 'recovered', displayName: 'Recovered' } }));
  expect(host.textContent).toContain('Recovered');
  expect(environment).toHaveBeenCalledTimes(2);
  expect(api.start).toHaveBeenCalledTimes(2);
  expect(host.textContent).not.toContain('Stale');
});

it('invalidates the apply preview when the backend environment report is unavailable', async () => {
  await thirdStep(); await ready(); await select(1, 'local-agent'); await click('Preview changes');
  vi.mocked(api.apply).mockRejectedValueOnce({ code: 'setup.environment-check-required', params: {} });
  await click('Apply and finish');
  expect(host.textContent).toContain('Complete the runtime environment check first');
  expect(host.querySelectorAll('select')[1].value).toBe('local-agent');
  expect(button('Preview changes').disabled).toBe(true);
  expect(host.textContent).not.toContain('4 execution configurations');
  expect(done).not.toHaveBeenCalled();
});

it('retains model, thought and permission drafts through a runtime recheck', async () => {
  await thirdStep();
  const agent: SetupAgent = { agentType: 'local-agent', displayName: 'Local', supportedModels: [{ id: 'model', name: 'Model' }], supportedModes: [{ id: 'ask', name: 'Ask' }], modelBoundCatalogs: { model: [{ id: 'reasoning', name: 'Reasoning', category: 'thought_level', currentValue: 'medium', options: [{ value: 'medium', name: 'Medium' }, { value: 'high', name: 'High' }] }] } };
  vi.spyOn(api, 'probeModel').mockResolvedValue(agent);
  await act(async () => receive({ type: 'ready', generation: 'generation-1', agent }));
  await select(1, 'local-agent'); await selectNamed('model', 'model'); await selectNamed('reasoning', 'high'); await selectNamed('permission', 'ask');
  await click('Preview changes');
  vi.mocked(api.apply).mockRejectedValueOnce({ code: 'setup.environment-check-required', params: {} });
  await click('Apply and finish');
  vi.mocked(api.start).mockImplementationOnce(async (listener) => { receive = listener; return 'generation-2'; });
  await click('Check runtime again');
  await act(async () => {
    receive({ type: 'checking', generation: 'generation-2', agentId: 'local-agent', label: 'Local' });
  });
  expect(button('Preview changes').disabled).toBe(true);
  await act(async () => receive({ type: 'ready', generation: 'generation-2', agent }));
  expect(host.querySelector<HTMLSelectElement>('[aria-label="model"]')!.value).toBe('model');
  expect(host.querySelector<HTMLSelectElement>('[aria-label="reasoning"]')!.value).toBe('high');
  expect(host.querySelector<HTMLSelectElement>('[aria-label="permission"]')!.value).toBe('ask');
  await click('Preview changes');
  expect(vi.mocked(api.preview).mock.lastCall?.[0]).toMatchObject({ profile: { agentId: 'local-agent', modelSelection: { modelId: 'model' }, configOptions: { reasoning: 'high' } }, permissionModeId: 'ask' });
});

it('keeps the passed runtime when leaving Agent detection and rejects its late results', async () => {
  const environment = vi.spyOn(api, 'environment');
  vi.mocked(api.start).mockImplementationOnce(async (listener) => { receive = listener; return 'generation-1'; });
  await thirdStep(); const old = receive;
  expect(host.textContent).toContain('Checking local Agents');
  await click('Back');
  expect(button('Check again').disabled).toBe(false);
  expect(button('Next').disabled).toBe(false);
  await act(async () => old({ type: 'ready', generation: 'generation-1', agent: { agentType: 'late', displayName: 'Late' } }));
  expect(button('Next').disabled).toBe(false);
  expect(environment).toHaveBeenCalledTimes(1);
  await click('Check again');
  expect(environment).toHaveBeenCalledTimes(2);
  expect(button('Next').disabled).toBe(false);
});

it('shows an empty result only after Agent detection completes normally', async () => {
  await thirdStep();
  const emptyMessage = i18n.t('onboarding.noAgents');
  expect(host.textContent).not.toContain(emptyMessage);
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  expect(host.textContent).toContain(emptyMessage);
});

it('clears pending probes when the detection batch finishes', async () => {
  await thirdStep();
  await act(async () => receive({ type: 'checking', generation: 'generation-1', agentId: 'unfinished', label: 'Unfinished Agent' }));
  expect(host.textContent).toContain(i18n.t('onboarding.discovering'));
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  expect(host.textContent).not.toContain(i18n.t('onboarding.discovering'));
  expect(host.textContent).toContain(i18n.t('onboarding.noAgents'));
  expect(button('Set up later')).toBeDefined();
});
it('offers set up later after detection completes even when an Agent is available', async () => {
  await act(async () => root.render(<OnboardingGate />));
  await click('Next');
  await click('Next');
  await ready();
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  expect(button('Set up later')).toBeDefined();
  await click('Set up later');
  await act(async () => { await vi.dynamicImportSettled(); });
  expect(host.textContent).toContain('Business application');
  expect((await api.status()).state.status).toBe('deferred');
});
it('marks Agent detection complete before model and permission choices are filled', async () => {
  await thirdStep();
  await ready();
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  expect(host.querySelectorAll('ol li')[2]?.getAttribute('data-state')).toBe('ready');
  expect(button('Preview changes').disabled).toBe(true);
});

it('lists installed but unusable Agents with structured reasons and hides uninstalled ones', async () => {
  await thirdStep();
  await act(async () => receive(unavailableEvent('goose', { kind: 'not-installed' })));
  await act(async () => receive(unavailableEvent('claude-acp', { kind: 'runtime-required', tool: 'npx' })));
  await act(async () => receive(unavailableEvent('qwen-code', { kind: 'doctor-failed', launch: 'local', error: { code: 'provider.auth-required', params: { methods: ['qoder-personal-access-token'], missingEnv: ['QODER_PERSONAL_ACCESS_TOKEN'] }, raw: { message: 'Authentication required' } } })));
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  expect(host.textContent).toContain(i18n.t('onboarding.allUnavailable'));
  expect(host.textContent).toContain('Unavailable Agents: 2');
  expect(host.textContent).toContain('Requires npm/npx');
  expect(host.textContent).toContain('QODER_PERSONAL_ACCESS_TOKEN');
  expect(host.textContent).not.toContain('Diagnosis failed: Authentication required');
  expect(host.textContent).not.toContain('goose');
  expect(host.querySelectorAll('select')[1].options.length).toBe(1);
  expect(button('Preview changes').disabled).toBe(true);
  await click('View runtime');
  expect(host.textContent).toContain('Check the runtime');
});

it('collapses unavailable Agents once one Agent is ready and keeps the user choice', async () => {
  await thirdStep();
  await act(async () => receive(unavailableEvent('claude-acp', { kind: 'runtime-required', tool: 'npx' })));
  expect(host.textContent).toContain('Requires npm/npx');
  await ready();
  expect(host.textContent).not.toContain('Requires npm/npx');
  await click('Unavailable Agents: 1');
  expect(host.textContent).toContain('Requires npm/npx');
  await ready('other-agent');
  expect(host.textContent).toContain('Requires npm/npx');
});

it('links an npx Agent diagnosis failure to the unverified Node.js version', async () => {
  vi.spyOn(api, 'environment').mockResolvedValue({ npxLaunchable: true, checks: [
    { tool: 'git', status: 'ready', version: '2.53.0', executablePath: null },
    { tool: 'node', status: 'unverified', version: '23.11.0', executablePath: null },
    { tool: 'npm', status: 'ready', version: '11.19.1', executablePath: null },
    { tool: 'npx', status: 'ready', version: '11.19.1', executablePath: null },
  ] });
  await thirdStep();
  await act(async () => receive(unavailableEvent('claude-acp', { kind: 'doctor-failed', launch: 'npx', error: { code: 'acp.adapter-exited', params: { reason: "SyntaxError: Unexpected token '??='" } } })));
  await act(async () => receive(unavailableEvent('qwen-code', { kind: 'doctor-failed', launch: 'local', error: null })));
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  expect(host.textContent).toContain("Diagnosis failed: SyntaxError: Unexpected token '??='");
  expect(host.textContent).toContain(`Node.js 23.11.0 is outside the verified range. Install ${status.environmentPolicy.node.recommendedVersion} instead.`);
  expect(host.textContent!.match(/outside the verified range/g)).toHaveLength(1);
});

it('removes an Agent withdrawn by a configuration change', async () => {
  await thirdStep(); await ready();
  await act(async () => receive({ type: 'withdrawn', generation: 'generation-1', agentId: 'local-agent' }));
  expect(host.querySelectorAll('select')[1].options.length).toBe(1);
  expect(host.textContent).not.toContain('Unavailable Agents');
});

it('streams only saved healthy Agents into the selector and applies a reviewed request', async () => {
  await thirdStep();
  await act(async () => receive(unavailableEvent('broken', { kind: 'not-installed' })));
  expect(host.querySelectorAll('select')[1].options.length).toBe(1);
  expect(Array.from(host.querySelectorAll('select')[1].options).some((option) => option.textContent?.includes('Broken Agent'))).toBe(false);
  expect(host.textContent).not.toContain('Broken Agent');
  expect(host.querySelector('details')).toBeNull();
  await ready(); await select(1, 'local-agent');
  await click('Preview changes');
  expect(api.preview).toHaveBeenCalledWith({ profile: { agentId: 'local-agent', modelSelection: { type: 'provider-default' }, configOptions: {}, modelBoundOverrides: {} }, permissionModeId: null });
  expect(host.textContent).toContain('4 execution configurations');
  await click('Apply and finish');
  expect(api.apply).toHaveBeenCalledTimes(1); expect(done).toHaveBeenCalledTimes(1);
  expect(api.cancel).toHaveBeenCalledWith('generation-1');
});

it('uses the workflow model, thought and permission controls without unrelated provider options', async () => {
  const agent: SetupAgent = {
    agentType: 'local-agent', displayName: 'Local Agent',
    supportedModels: [{ id: 'standard', name: 'Standard' }],
    supportedModes: [{ id: 'ask', name: 'Ask before changes' }, { id: 'edit', name: 'Allow edits' }],
    configOptions: [{ id: 'output', name: 'Output format', currentValue: 'text', options: [{ value: 'text', name: 'Text' }] }],
    modelBoundCatalogs: {
      standard: [
        { id: 'reasoning', name: 'Reasoning effort', category: 'thought_level', currentValue: 'medium', options: [{ value: 'medium', name: 'Medium' }, { value: 'high', name: 'High' }] },
        { id: 'context', name: 'Context window', category: 'model_config', currentValue: 'auto', options: [{ value: 'auto', name: 'Automatic' }] },
      ],
    },
  };
  vi.spyOn(api, 'probeModel').mockResolvedValue(agent);
  await thirdStep();
  await act(async () => receive({ type: 'ready', generation: 'generation-1', agent }));
  await select(1, 'local-agent'); await selectNamed('model', 'standard'); await selectNamed('reasoning', 'high'); await selectNamed('permission', 'edit');
  expect(api.probeModel).toHaveBeenCalledWith('generation-1', 'local-agent', 'standard');
  expect(host.textContent).not.toContain('Output format');
  expect(host.textContent).toContain('Reasoning effort');
  expect(host.textContent).toContain('Context window');
  const selection = { profile: { agentId: 'local-agent', modelSelection: { type: 'explicit', modelId: 'standard' }, configOptions: { reasoning: 'high' }, modelBoundOverrides: { standard: { reasoning: 'high' } } }, permissionModeId: 'edit' };
  await click('Preview changes');
  expect(api.preview).toHaveBeenCalledWith(selection);
  await click('Apply and finish');
  expect(api.apply).toHaveBeenCalledWith(expect.objectContaining({ selection }));
});

it.each([undefined, [{ id: 'output', options: [{ value: 'text', name: 'Text' }] }]])('finishes with the provider default when legacy diagnostics expose no standard model selector (%j)', async (configOptions) => {
  vi.spyOn(api, 'probeModel').mockRejectedValue({ code: 'acp.model-config-unavailable', params: {} });
  await thirdStep();
  await act(async () => receive({ type: 'ready', generation: 'generation-1', agent: {
    agentType: 'local-agent', displayName: 'Legacy Agent',
    supportedModels: null, supportedModes: [{ id: 'default', name: 'Default' }], configOptions,
  } }));
  await select(1, 'local-agent'); await selectNamed('permission', 'default');
  expect(host.textContent).toContain(i18n.t('onboarding.providerDefault'));
  expect(button('Preview changes').disabled).toBe(false);
  expect(api.probeModel).not.toHaveBeenCalled();
  const selection = { profile: { agentId: 'local-agent', modelSelection: { type: 'provider-default' }, configOptions: {}, modelBoundOverrides: {} }, permissionModeId: 'default' };
  await click('Preview changes');
  expect(api.preview).toHaveBeenCalledWith(selection);
  await click('Apply and finish');
  expect(api.apply).toHaveBeenCalledWith(expect.objectContaining({ selection }));
  expect(done).toHaveBeenCalledOnce();
});

it.each([{ id: 'model', category: 'model' }, { id: 'provider/model', category: 'model' }])('still blocks a failed standard model probe for selector %j', async (selector) => {
  vi.spyOn(api, 'probeModel').mockRejectedValue({ code: 'acp.model-config-unavailable', params: {} });
  await thirdStep();
  await act(async () => receive({ type: 'ready', generation: 'generation-1', agent: {
    agentType: 'local-agent', displayName: 'Standard Agent',
    supportedModels: [{ id: 'standard', name: 'Standard' }], supportedModes: [],
    configOptions: [{ ...selector, options: [{ value: 'standard', name: 'Standard' }] }],
  } }));
  await select(1, 'local-agent'); await selectNamed('model', 'standard');
  expect(api.probeModel).toHaveBeenCalledWith('generation-1', 'local-agent', 'standard');
  expect(host.querySelector<HTMLSelectElement>('[aria-label="model"]')!.value).toBe('');
  expect(button('Preview changes').disabled).toBe(true);
  expect(api.preview).not.toHaveBeenCalled();
  expect(done).not.toHaveBeenCalled();
});

it('restores model-specific thought choices when switching models, as the workflow editor does', async () => {
  const agent: SetupAgent = {
    agentType: 'local-agent', displayName: 'Local Agent',
    supportedModels: [{ id: 'standard', name: 'Standard' }, { id: 'fast', name: 'Fast' }], supportedModes: [],
    modelBoundCatalogs: {
      standard: [{ id: 'reasoning', category: 'thought_level', options: [{ value: 'high', name: 'High' }] }],
      fast: [{ id: 'thinking', category: 'thought_level', options: [{ value: 'low', name: 'Low' }] }],
    },
  };
  vi.spyOn(api, 'probeModel').mockResolvedValue(agent);
  await thirdStep(); await act(async () => receive({ type: 'ready', generation: 'generation-1', agent }));
  await select(1, 'local-agent'); await selectNamed('model', 'standard'); await selectNamed('reasoning', 'high');
  await selectNamed('model', 'fast'); await selectNamed('thinking', 'low'); await selectNamed('model', 'standard');
  expect(host.querySelector<HTMLSelectElement>('select[aria-label="reasoning"]')?.value).toBe('high');
  await click('Preview changes');
  expect(api.preview).toHaveBeenCalledWith({ profile: { agentId: 'local-agent', modelSelection: { type: 'explicit', modelId: 'standard' }, configOptions: { reasoning: 'high' }, modelBoundOverrides: { standard: { reasoning: 'high' } } }, permissionModeId: null });
});

it('does not resubmit unavailable overrides when rerunning setup with an existing default Agent', async () => {
  status.defaultExecutionProfile = {
    agentId: 'local-agent', modelSelection: { type: 'provider-default' },
    configOptions: { reasoning: 'high' }, modelBoundOverrides: { standard: { context: 'large' } },
  };
  await thirdStep(); await ready();
  expect(host.querySelectorAll('select')[1].value).toBe('local-agent');
  await click('Preview changes');
  const selection = { profile: { agentId: 'local-agent', modelSelection: { type: 'provider-default' }, configOptions: {}, modelBoundOverrides: {} }, permissionModeId: null };
  expect(api.preview).toHaveBeenCalledWith(selection);
  await click('Apply and finish');
  expect(api.apply).toHaveBeenCalledWith(expect.objectContaining({ selection }));
});

it('keeps the existing supported thought choice while excluding unrelated saved options', async () => {
  status.defaultExecutionProfile = {
    agentId: 'local-agent', modelSelection: { type: 'explicit', modelId: 'standard' },
    configOptions: { reasoning: 'high', output: 'json' }, modelBoundOverrides: { standard: { reasoning: 'high', output: 'json' } },
  };
  await thirdStep();
  await act(async () => receive({ type: 'ready', generation: 'generation-1', agent: {
    agentType: 'local-agent', displayName: 'Local Agent', supportedModels: [{ id: 'standard', name: 'Standard' }], supportedModes: [],
    configOptions: [{ id: 'output', currentValue: 'text', options: [{ value: 'json', name: 'JSON' }] }],
    modelBoundCatalogs: { standard: [{ id: 'reasoning', category: 'thought_level', options: [{ value: 'high', name: 'High' }] }] },
  } }));
  expect(host.querySelector<HTMLSelectElement>('select[aria-label="reasoning"]')?.value).toBe('high');
  await click('Preview changes');
  expect(api.preview).toHaveBeenCalledWith({ profile: { agentId: 'local-agent', modelSelection: { type: 'explicit', modelId: 'standard' }, configOptions: { reasoning: 'high' }, modelBoundOverrides: { standard: { reasoning: 'high' } } }, permissionModeId: null });
});

it('submits current-model thought options without inventing an unobserved per-model catalog', async () => {
  status.defaultExecutionProfile = { agentId: 'local-agent', modelSelection: { type: 'explicit', modelId: 'standard' }, configOptions: { reasoning: 'high' }, modelBoundOverrides: {} };
  await thirdStep();
  await act(async () => receive({ type: 'ready', generation: 'generation-1', agent: {
    agentType: 'local-agent', displayName: 'Local Agent', supportedModels: [{ id: 'standard', name: 'Standard' }], supportedModes: [],
    configOptions: [
      { id: 'model', category: 'model', currentValue: 'standard', options: [{ value: 'standard', name: 'Standard' }] },
      { id: 'reasoning', category: 'thought_level', options: [{ value: 'high', name: 'High' }] },
    ],
  } }));
  await click('Preview changes');
  expect(api.preview).toHaveBeenCalledWith({ profile: { agentId: 'local-agent', modelSelection: { type: 'explicit', modelId: 'standard' }, configOptions: { reasoning: 'high' }, modelBoundOverrides: {} }, permissionModeId: null });
});

it('invalidates a selected failed Agent without silently choosing another', async () => {
  await thirdStep(); await ready(); await ready('other-agent'); await select(1, 'local-agent');
  await act(async () => receive(unavailableEvent('local-agent', { kind: 'doctor-failed', launch: 'local', error: null })));
  expect(host.querySelectorAll('select')[1].value).toBe('');
  expect(button('Preview changes').disabled).toBe(true);
});

it('counts detected Agents before choosing a default and retains them when loading another batch', async () => {
  await thirdStep();
  await ready('opencode'); await ready('kimi');
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: 'next-batch' }));
  expect(host.querySelector('[role="status"]')?.textContent).toContain('2 available Agents detected');
  expect(host.querySelectorAll('select')[1].options.length).toBe(3);
  expect(button('Preview changes').disabled).toBe(true);
  await click('Load more');
  expect(api.start).toHaveBeenLastCalledWith(expect.any(Function), 'next-batch');
  expect(host.querySelectorAll('select')[1].options.length).toBe(3);
  await ready('third-agent');
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  expect(host.querySelector('[role="status"]')?.textContent).toContain('3 available Agents detected');
});

it('ignores old detection results after leaving the Agent step', async () => {
  await thirdStep(); const old = receive;
  await click('Back');
  expect(api.cancel).toHaveBeenCalledWith('generation-1');
  await act(async () => old({ type: 'ready', generation: 'generation-1', agent: { agentType: 'late', displayName: 'Late' } }));
  await click('Next');
  expect(host.querySelectorAll('select')[1].options.length).toBe(1);
  expect(button('Preview changes').disabled).toBe(true);
});

it('keeps the same operation id when retrying a failed apply', async () => {
  await thirdStep(); await ready(); await select(1, 'local-agent'); await click('Preview changes');
  vi.mocked(api.apply).mockRejectedValueOnce({ code: 'setup.commit-failed' });
  await click('Apply and finish'); const first = vi.mocked(api.apply).mock.calls[0][0];
  await click('Apply and finish');
  expect(vi.mocked(api.apply).mock.calls[1][0].operationId).toBe(first.operationId);
});


it('preserves a healthy selection during refresh but blocks applying until its check finishes', async () => {
  const environment = vi.spyOn(api, 'environment');
  await thirdStep(); await ready(); await select(1, 'local-agent');
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  await click('Check again');
  expect(button('Preview changes').disabled).toBe(true);
  expect(host.querySelectorAll('select')[1].value).toBe('');
  await act(async () => receive({ type: 'checking', generation: 'generation-1', agentId: 'local-agent', label: 'Local' }));
  expect(host.querySelectorAll('select')[1].value).toBe('');
  expect(button('Preview changes').disabled).toBe(true);
  await ready();
  expect(host.querySelectorAll('select')[1].value).toBe('local-agent');
  expect(button('Preview changes').disabled).toBe(false);
  expect(environment).toHaveBeenCalledTimes(1);
});

it('cancels the active environment request on unmount', async () => {
  vi.spyOn(api, 'cancelEnvironment');
  vi.spyOn(api, 'environment').mockImplementation(async () => new Promise(() => {}));
  await render();
  await act(async () => root.unmount());
  expect(api.cancelEnvironment).toHaveBeenCalledWith(expect.any(String));
  root = createRoot(host);
});

it('releases the completed environment report only when the onboarding page unmounts', async () => {
  const cancel = vi.spyOn(api, 'cancelEnvironment');
  const environment = vi.spyOn(api, 'environment');
  await thirdStep();
  const requestId = environment.mock.calls[0][0];
  expect(cancel).not.toHaveBeenCalled();
  await click('Back');
  expect(cancel).not.toHaveBeenCalled();
  await act(async () => root.unmount());
  expect(cancel).toHaveBeenCalledWith(requestId);
  root = createRoot(host);
});

it('shows the old bindings in the application preview', async () => {
  await thirdStep(); await ready(); await select(1, 'local-agent');
  vi.mocked(api.preview).mockResolvedValue({ fingerprint: 'preview', slotCount: 1, templates: [{ id: 'default', name: 'Default', slotCount: 1, previousBindings: [{ executionSlotId: 'default.worker', agentId: 'old-agent', modelId: 'old-model', autoAccept: false, configOptions: {}, modelBoundOverrides: {}, permissionModeId: null }] }] });
  await click('Preview changes');
  expect(host.textContent).toContain('Current bindings to replace');
  expect(host.textContent).toContain('old-agent / old-model');
});

it('enters the business application after applying through the bootstrap gate', async () => {
  await act(async () => root.render(<OnboardingGate />));
  await click('Next'); await click('Next'); await ready(); await select(1, 'local-agent');
  await click('Preview changes'); await click('Apply and finish');
  expect(host.textContent).toContain('Business application');
  expect(window.location.hash).toBe('');
  expect((await api.status()).state.status).toBe('completed');
});

it('opens the defaults entry directly for an existing installation', async () => {
  status.state = { status: 'existing-install' };
  window.location.hash = '#/onboarding/defaults';
  await act(async () => root.render(<OnboardingGate />));
  expect(host.textContent).toContain('Connect an Agent');
  expect(api.start).toHaveBeenCalledTimes(1);
  expect(mocks.business).not.toHaveBeenCalled();
});
it('enters the business application when no Agent is available and the user chooses to set up later', async () => {
  await act(async () => root.render(<OnboardingGate />));
  await click('Next');
  await click('Next');
  expect(button('Set up later')).toBeUndefined();
  await act(async () => receive({ type: 'finished', generation: 'generation-1', nextCursor: null }));
  expect(button('Set up later')).toBeDefined();
  await click('Set up later');
  await act(async () => { await vi.dynamicImportSettled(); });
  expect(host.textContent).toContain('Business application');
  expect(window.location.hash).toBe('');
  expect((await api.status()).state.status).toBe('deferred');
});

it('opens the business application on startup after setup was deferred', async () => {
  status.state = { status: 'deferred', gitDecision: 'undecided' };
  window.location.hash = '#/tasks';
  await act(async () => root.render(<OnboardingGate />));
  await act(async () => { await vi.dynamicImportSettled(); });
  expect(host.textContent).toContain('Business application');
  expect(mocks.business).toHaveBeenCalled();
  expect(api.start).not.toHaveBeenCalled();
});
