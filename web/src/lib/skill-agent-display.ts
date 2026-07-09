import type { SkillMetaVm, SupportedAgentTypeVm } from '../types';

export interface SkillAgentDisplayMeta {
  agentType: string;
  label: string;
  iconKey: string;
}

export const GOLD_BAND_AGENT_META: SkillAgentDisplayMeta = {
  agentType: 'gold-band',
  label: 'Gold Band',
  iconKey: 'gold-band',
};

export function skillSourceAgent(
  skill: SkillMetaVm,
  configuredAgents: SupportedAgentTypeVm[],
) {
  if (skill.agentSource === '.gold-band') {
    return GOLD_BAND_AGENT_META;
  }

  return configuredAgents.find((agent) => agent.skillsDirName === skill.agentSource) ?? null;
}

export function skillAvailableAgentTypes(
  skill: SkillMetaVm,
  configuredAgents: SupportedAgentTypeVm[],
) {
  const available = new Set(skill.syncedAgentTypes);
  const sourceMeta = skillSourceAgent(skill, configuredAgents);
  if (sourceMeta && sourceMeta.agentType !== GOLD_BAND_AGENT_META.agentType) {
    available.add(sourceMeta.agentType);
  }
  return [...available];
}

export function skillDisplayAgents(
  skill: SkillMetaVm,
  configuredAgents: SupportedAgentTypeVm[],
) {
  const display: SkillAgentDisplayMeta[] = [];
  const seen = new Set<string>();

  const sourceMeta = skillSourceAgent(skill, configuredAgents);
  if (sourceMeta) {
    display.push(sourceMeta);
    seen.add(sourceMeta.agentType);
  }

  for (const agentType of skill.syncedAgentTypes) {
    const meta = configuredAgents.find((agent) => agent.agentType === agentType);
    if (!meta || seen.has(agentType)) {
      continue;
    }
    display.push(meta);
    seen.add(agentType);
  }

  return display;
}

export function selectableSyncAgents(
  skill: SkillMetaVm | null,
  configuredAgents: SupportedAgentTypeVm[],
) {
  const sourceMeta = skill ? skillSourceAgent(skill, configuredAgents) : null;
  if (!sourceMeta || sourceMeta.agentType === GOLD_BAND_AGENT_META.agentType) {
    return configuredAgents;
  }
  return configuredAgents.filter((agent) => agent.agentType !== sourceMeta.agentType);
}
