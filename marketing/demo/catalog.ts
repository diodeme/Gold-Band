import { mockAgentRegistry, mockProfileList, mockWorkflowTemplates } from '@/mockData';
import type { AgentRegistryVm, DesktopLanguage } from '@/types';

export const demoAgentRegistry: AgentRegistryVm = {
  catalog: mockAgentRegistry.catalog.map((entry) => ({ ...entry, configured: true })),
  agents: mockAgentRegistry.catalog.map((entry) => {
    const configured = mockAgentRegistry.agents.find((agent) => agent.agentType === entry.agentType);
    const agent = configured ?? {
      agentType: entry.agentType, displayName: entry.defaultDisplayName,
      command: entry.defaultCommand, args: entry.defaultArgs, env: entry.defaultEnv,
      iconKey: entry.iconKey, primaryAgentDir: entry.primaryAgentDir,
      projectPrimaryAgentDir: entry.projectPrimaryAgentDir, compatibleAgentDirs: entry.compatibleAgentDirs,
      supportsSystemPrompt: entry.supportsSystemPrompt,
      externalSessionSyncSupported: entry.supportsExternalSessionSync, externalSessionSyncEnabled: false,
      diagnostic: null, supportedModels: [], supportedModes: [], configOptions: [],
    };
    return entry.agentType === 'claude-acp' || entry.agentType === 'codex-acp'
      ? { ...agent, mcpHttpSupported: true, mcpSseSupported: entry.agentType === 'claude-acp',
        diagnostic: { status: 'healthy' as const, available: true, reason: null, checkedAt: mockAgentRegistry.agents[0].diagnostic!.checkedAt } }
      : agent;
  }),
};

export const demoWorkflowTemplates = structuredClone(mockWorkflowTemplates);
for (const template of demoWorkflowTemplates.templates) {
  for (const binding of template.modelBindings.bindings) {
    const agent = demoAgentRegistry.agents.find((agent) => agent.agentType === binding.agentId);
    if (!agent?.supportedModes?.some((mode) => mode.id === binding.permissionModeId)) {
      binding.permissionModeId = agent?.supportedModes?.[0]?.id ?? null;
    }
  }
}

const profileSources = import.meta.glob<string>('../../src/prompts/*/profile/*.md', { query: '?raw', import: 'default' });
const profileFiles: Record<string, string> = { cleanup: 'clean', grill: 'GrillMe' };
const englishNames: Record<string, string> = {
  plan: 'Planning', dev: 'Development', 'dev-test': 'Development and testing', review: 'Review',
  test: 'Testing', accept: 'Acceptance', cleanup: 'Cleanup', interview: 'Interview', grill: 'Grill',
};
export function demoProfiles(language: DesktopLanguage) {
  return mockProfileList.profiles.map((profile) => {
    const key = profile.id.replace('pf-builtin-', '');
    return { ...profile, content: '', name: language === 'en' ? englishNames[key] ?? profile.name : profile.name,
      summary: language === 'en' ? englishNames[key] ?? profile.summary : profile.summary };
  });
}
export async function demoProfileContent(id: string, language: DesktopLanguage) {
  const key = id.replace('pf-builtin-', '');
  const source = profileSources[`../../src/prompts/${language === 'en' ? 'en' : 'zh-CN'}/profile/${profileFiles[key] ?? key}.md`];
  if (!source) throw { code: 'demo.resource-not-found', params: { id } };
  return source();
}
