import { describe, expect, it } from 'vitest';

import type { SkillMetaVm, SupportedAgentTypeVm } from '../src/types';
import {
  GOLD_BAND_AGENT_META,
  selectableSyncAgents,
  skillAvailableAgentTypes,
  skillDisplayAgents,
  skillSourceAgent,
} from '../src/lib/skill-agent-display';

const configuredAgents: SupportedAgentTypeVm[] = [
  { agentType: 'claude-acp', label: 'Claude', iconKey: 'claude', skillsDirName: '.claude', supported: true, configured: true, defaultDisplayName: 'Claude', defaultCommand: 'npx', defaultArgs: [], defaultEnv: [] },
  { agentType: 'codex-acp', label: 'Codex', iconKey: 'codex', skillsDirName: '.codex', supported: true, configured: true, defaultDisplayName: 'Codex', defaultCommand: 'npx', defaultArgs: [], defaultEnv: [] },
];

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

describe('skill agent display helpers', () => {
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

    expect(skillSourceAgent(skill, configuredAgents)).toEqual(GOLD_BAND_AGENT_META);
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
});
