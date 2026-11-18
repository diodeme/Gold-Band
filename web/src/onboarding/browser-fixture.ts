import type { EnvironmentReport, SetupAgent, SetupApi, SetupEvent, SetupStatus } from './types';
import packagedPolicy from '../../../configs/onboarding-environment-policy.json';

let browserApi: SetupApi | undefined;
export function browserSetupApi(): SetupApi {
  return browserApi ??= createBrowserSetupApi();
}

/** Explicit development fixture: it never diagnoses or writes the host installation. */
export function createBrowserSetupApi(): SetupApi {
  const fixture = new URLSearchParams(location.search).get('onboardingFixture');
  const status: SetupStatus = {
    platform: 'windows',
    state: { status: 'pending', gitDecision: 'undecided' }, gitMinimumVersion: '2.36.0', language: 'zh-cn', defaultExecutionProfile: null,
    appearance: { schemaVersion: 2, themeId: 'builtin.gold-band', colorScheme: 'dark', visualQualityByTheme: {} },
    environmentPolicy: { node: { ranges: ['>=22.20.0 <23', '>=24 <25'], recommendedVersion: '24.22.0', helpUrl: 'https://nodejs.org/en/download' }, npm: { ranges: packagedPolicy.npm.ranges }, gitHelpUrl: 'https://git-scm.com/downloads' },
  };
  const agent: SetupAgent = {
    agentType: 'fixture-agent', displayName: 'Local Agent',
    supportedModels: fixture === 'legacy-models' ? null : [{ id: 'fixture-model', name: 'Standard' }],
    supportedModes: [{ id: 'default', name: 'Ask before changes' }],
    configOptions: [{ id: 'output', name: 'Output format', currentValue: 'text', options: [{ value: 'text', name: 'Text' }] }],
    modelBoundCatalogs: fixture === 'legacy-models' ? null : {
      'fixture-model': [{ id: 'reasoning', name: 'Reasoning effort', category: 'thought_level', currentValue: 'medium', options: [{ value: 'medium', name: 'Medium' }, { value: 'high', name: 'High' }] }],
    },
  };
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  let environment: { requestId: string; report?: EnvironmentReport } | null = null;
  type Check = EnvironmentReport['checks'][number];
  const missingTools: Check['tool'][] = fixture === 'npm-limited' ? ['npm', 'npx'] : fixture === 'node-missing' ? ['node'] : [];
  const report = (): EnvironmentReport => {
    const checks = (['git', 'node', 'npm', 'npx'] as const).map((tool): Check => {
      const path = `C:\\Users\\示例用户\\Programs with spaces\\${tool}.exe`;
      if (missingTools.includes(tool)) return { tool, status: 'missing', version: null, executablePath: null, error: { code: 'setup.runtime-missing', params: { tool } } };
      if (tool === 'node' && fixture === 'unverified-node') return { tool, status: 'unverified', version: '23.11.0', executablePath: path, error: { code: 'setup.runtime-version-unverified', params: { tool, version: '23.11.0' } } };
      const runtimeNode = tool === 'npm' || tool === 'npx' ? { nodeExecutablePath: 'C:\\Users\\示例用户\\Programs with spaces\\node.exe', nodeVersion: fixture === 'unverified-node' ? '23.11.0' : '24.22.0' } : {};
      return { tool, status: 'ready', version: tool === 'git' ? '2.53.0' : tool === 'node' ? '24.22.0' : '11.19.1', executablePath: path, ...runtimeNode };
    });
    return { checks, npxLaunchable: !missingTools.includes('npx') };
  };
  const requireEnvironment = () => {
    if (!environment?.report) throw { code: 'setup.environment-check-required', params: {} };
  };
  const unavailableEvents = (generation: string, npxLaunchable: boolean): SetupEvent[] => [
    { type: 'unavailable', generation, agentId: 'goose', label: 'Goose', iconKey: 'goose', reason: { kind: 'not-installed' } },
    { type: 'unavailable', generation, agentId: 'qwen-code', label: 'Qwen Code', iconKey: 'qwen-code', reason: { kind: 'doctor-failed', launch: 'local', error: { code: 'provider.auth-required', params: { methods: ['qwen-personal-access-token'], missingEnv: ['QWEN_API_KEY'] }, raw: { message: 'Authentication required' } } } },
    npxLaunchable
      ? { type: 'unavailable', generation, agentId: 'claude-acp', label: 'Claude', iconKey: 'claude', reason: { kind: 'doctor-failed', launch: 'npx', error: { code: 'acp.adapter-exited', params: { reason: "SyntaxError: Unexpected token '??='" } } } }
      : { type: 'unavailable', generation, agentId: 'claude-acp', label: 'Claude', iconKey: 'claude', reason: { kind: 'runtime-required', tool: 'npx' } },
  ];
  return {
    status: async () => status,
    environment: async (requestId) => {
      environment = { requestId };
      await Promise.resolve();
      const result = report();
      if (environment?.requestId === requestId) environment.report = result;
      return result;
    },
    skipGit: async () => { status.state = { status: 'pending', gitDecision: 'skip' }; },
    deferSetup: async () => { if (status.state.status === 'pending') status.state = { status: 'deferred', gitDecision: status.state.gitDecision }; },
    cancelEnvironment: async (requestId) => { if (environment?.requestId === requestId) environment = null; },
    start: async (receive) => {
      requireEnvironment();
      const generation = crypto.randomUUID();
      const npxLaunchable = environment?.report?.npxLaunchable ?? false;
      if (fixture !== 'all-unavailable' && fixture !== 'no-agents') receive({ type: 'checking', generation, agentId: agent.agentType, label: agent.displayName });
      timers.set(generation, setTimeout(() => {
        if (fixture !== 'all-unavailable' && fixture !== 'no-agents') receive({ type: 'ready', generation, agent });
        if (fixture !== 'no-agents') unavailableEvents(generation, npxLaunchable).forEach(receive);
        receive({ type: 'finished', generation, nextCursor: null });
      }, 600));
      return generation;
    },
    cancel: async (generation) => { clearTimeout(timers.get(generation)); timers.delete(generation); },
    probeModel: async () => {
      if (fixture === 'legacy-models') throw { code: 'acp.model-config-unavailable', params: {} };
      return agent;
    },
    preview: async () => ({ fingerprint: 'fixture-preview', slotCount: 4, templates: [{ id: 'default', name: 'Default workflow', slotCount: 3, previousBindings: [] }, { id: 'lightweight', name: 'Lightweight workflow', slotCount: 1, previousBindings: [] }] }),
    apply: async (request) => { requireEnvironment(); status.state = { status: 'completed' }; status.defaultExecutionProfile = request.selection.profile; },
    preferences: async (language, appearance) => { status.language = language; status.appearance = appearance; },
  };
}
