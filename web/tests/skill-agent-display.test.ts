import { describe, expect, it } from 'vitest';

import type { AgentCatalogEntryVm, AgentRegistryVm, ManagedAgentVm, SkillMetaVm } from '../src/types';
import {
  GOLD_BAND_AGENT_META,
  configuredSkillAgents,
  selectableSyncAgents,
  skillAvailableAgentTypes,
  skillDisplayAgents,
  skillSourceAgents,
} from '../src/lib/skill-agent-display';

const configuredAgents: AgentCatalogEntryVm[] = [
  { agentType: 'claude-acp', label: 'Claude', iconKey: 'claude', version: '1', description: '', repository: null, website: null, primaryAgentDir: '.claude', projectPrimaryAgentDir: null, compatibleAgentDirs: [], configured: true, supportsSystemPrompt: true, supportsExternalSessionSync: false, defaultDisplayName: 'Claude', defaultCommand: 'npx', defaultArgs: [], defaultEnv: [] },
  { agentType: 'codex-acp', label: 'Codex', iconKey: 'codex', version: '1', description: '', repository: null, website: null, primaryAgentDir: '.codex', projectPrimaryAgentDir: null, compatibleAgentDirs: ['.agents'], configured: true, supportsSystemPrompt: false, supportsExternalSessionSync: false, defaultDisplayName: 'Codex', defaultCommand: 'npx', defaultArgs: [], defaultEnv: [] },
];

const piAgent: AgentCatalogEntryVm = {
  agentType: 'pi-acp',
  label: 'Pi',
  iconKey: 'pi-acp',
  version: '1',
  description: '',
  repository: null,
  website: null,
  primaryAgentDir: '.pi/agent',
  projectPrimaryAgentDir: '.pi',
  compatibleAgentDirs: ['.agents'],
  configured: true,
  supportsSystemPrompt: false,
  supportsExternalSessionSync: false,
  defaultDisplayName: 'Pi',
  defaultCommand: 'npx',
  defaultArgs: [],
  defaultEnv: [],
};

function makeSkill(overrides: Partial<SkillMetaVm>): SkillMetaVm {
  return {
    name: 'test-skill',
    description: '',
    source: 'project',
    directoryPath: 'E:/AI_PROJECT/Gold-Band/.gold-band/skills/test-skill',
    agentSource: '.gold-band',
    loadWarnings: [],
    syncedAgentTypes: [],
    ...overrides,
  };
}

function makeManagedAgent(overrides: Partial<ManagedAgentVm> & Pick<ManagedAgentVm, 'agentType' | 'displayName'>): ManagedAgentVm {
  return {
    command: 'agent',
    args: [],
    env: [],
    iconKey: 'gold-band',
    primaryAgentDir: '.agent',
    projectPrimaryAgentDir: null,
    compatibleAgentDirs: [],
    supportsSystemPrompt: false,
    externalSessionSyncSupported: false,
    externalSessionSyncEnabled: false,
    ...overrides,
  };
}

describe('skill agent display helpers', () => {
  it('uses managed Agent configuration as the source of truth and appends custom Agents', () => {
    const customAgent = makeManagedAgent({
      agentType: '11',
      displayName: '11',
      iconKey: 'data:image/png;base64,custom',
      primaryAgentDir: '.custom-11',
    });
    const editedCodex = makeManagedAgent({
      agentType: 'codex-acp',
      displayName: 'My Codex',
      iconKey: 'data:image/png;base64,codex-custom',
      primaryAgentDir: '.codex-custom',
      projectPrimaryAgentDir: '.codex-project',
      compatibleAgentDirs: ['.shared-skills'],
    });
    const registry: AgentRegistryVm = {
      agents: [customAgent, editedCodex],
      catalog: configuredAgents,
    };

    expect(configuredSkillAgents(registry)).toEqual([
      {
        agentType: 'codex-acp',
        label: 'My Codex',
        iconKey: 'data:image/png;base64,codex-custom',
        primaryAgentDir: '.codex-custom',
        projectPrimaryAgentDir: '.codex-project',
        compatibleAgentDirs: ['.shared-skills'],
      },
      {
        agentType: '11',
        label: '11',
        iconKey: 'data:image/png;base64,custom',
        primaryAgentDir: '.custom-11',
        projectPrimaryAgentDir: null,
        compatibleAgentDirs: [],
      },
    ]);
  });

  it('includes source agent and synced agents together for display', () => {
    const skill = makeSkill({
      agentSource: '.claude',
      directoryPath: 'E:/AI_PROJECT/Gold-Band/.claude/skills/test-skill',
      syncedAgentTypes: ['codex-acp'],
    });

    expect(skillDisplayAgents(skill, configuredAgents)).toEqual([
      configuredAgents[0],
      configuredAgents[1],
    ]);
  });

  it('uses Gold Band as the source icon for self-managed skills', () => {
    const skill = makeSkill({
      syncedAgentTypes: ['claude-acp'],
    });

    expect(skillSourceAgents(skill, configuredAgents)).toEqual([GOLD_BAND_AGENT_META]);
    expect(skillDisplayAgents(skill, configuredAgents)).toEqual([
      GOLD_BAND_AGENT_META,
      configuredAgents[0],
    ]);
  });

  it('filters available agents by source plus synced targets', () => {
    const nativeSkill = makeSkill({
      agentSource: '.claude',
      directoryPath: 'E:/AI_PROJECT/Gold-Band/.claude/skills/test-skill',
      syncedAgentTypes: ['codex-acp'],
    });
    const managedSkill = makeSkill({
      syncedAgentTypes: ['codex-acp'],
    });

    expect(skillAvailableAgentTypes(nativeSkill, configuredAgents)).toEqual(['codex-acp', 'claude-acp']);
    expect(skillAvailableAgentTypes(managedSkill, configuredAgents)).toEqual(['codex-acp']);
  });

  it('excludes the native source agent from sync checkboxes', () => {
    const nativeSkill = makeSkill({
      agentSource: '.claude',
      directoryPath: 'E:/AI_PROJECT/Gold-Band/.claude/skills/test-skill',
    });

    expect(selectableSyncAgents(nativeSkill, configuredAgents)).toEqual([configuredAgents[1]]);
    expect(selectableSyncAgents(makeSkill({}), configuredAgents)).toEqual(configuredAgents);
  });

  it('treats compatible directory readers as native source agents', () => {
    const skill = makeSkill({
      agentSource: '.agents',
      directoryPath: 'C:/Users/test/.agents/skills/test-skill',
    });

    expect(skillSourceAgents(skill, configuredAgents)).toEqual([configuredAgents[1]]);
    expect(selectableSyncAgents(skill, configuredAgents)).toEqual([configuredAgents[0]]);
  });

  it('keeps a native reader in sync actions only while its redundant link exists', () => {
    const linkedSkill = makeSkill({
      agentSource: '.agents',
      directoryPath: 'C:/Users/test/.agents/skills/test-skill',
      syncedAgentTypes: ['codex-acp'],
    });
    const unlinkedSkill = makeSkill({
      agentSource: '.agents',
      directoryPath: 'C:/Users/test/.agents/skills/test-skill',
    });

    expect(selectableSyncAgents(linkedSkill, configuredAgents)).toEqual(configuredAgents);
    expect(selectableSyncAgents(unlinkedSkill, configuredAgents)).toEqual([configuredAgents[0]]);
  });

  it('matches split primary agent directories by skill scope', () => {
    const agents = [...configuredAgents, piAgent];
    const globalSkill = makeSkill({
      source: 'global',
      agentSource: '.pi/agent',
      directoryPath: 'C:/Users/test/.pi/agent/skills/test-skill',
    });
    const projectSkill = makeSkill({
      source: 'project',
      agentSource: '.pi',
      directoryPath: 'E:/AI_PROJECT/Gold-Band/.pi/skills/test-skill',
    });

    expect(skillSourceAgents(globalSkill, agents)).toEqual([piAgent]);
    expect(skillSourceAgents(projectSkill, agents)).toEqual([piAgent]);
  });

  it('does not treat a project primary directory as a global source', () => {
    const globalSkill = makeSkill({
      source: 'global',
      agentSource: '.pi',
      directoryPath: 'C:/Users/test/.pi/skills/test-skill',
    });

    expect(skillSourceAgents(globalSkill, [...configuredAgents, piAgent])).toEqual([]);
  });
});
