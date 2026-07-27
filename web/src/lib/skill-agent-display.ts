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

export function skillSourceAgents(
  skill: SkillMetaVm,
  configuredAgents: SupportedAgentTypeVm[],
) {
  if (skill.agentSource === '.gold-band') {
    return [GOLD_BAND_AGENT_META];
  }

  return configuredAgents.filter((agent) => (
    agent.primaryAgentDir === skill.agentSource
    || agent.compatibleAgentDirs.includes(skill.agentSource)
  ));
}

export function skillAvailableAgentTypes(
  skill: SkillMetaVm,
  configuredAgents: SupportedAgentTypeVm[],
) {
  const available = new Set(skill.syncedAgentTypes);
  for (const sourceMeta of skillSourceAgents(skill, configuredAgents)) {
    if (sourceMeta.agentType !== GOLD_BAND_AGENT_META.agentType) {
      available.add(sourceMeta.agentType);
    }
  }
  return [...available];
}

export function skillDisplayAgents(
  skill: SkillMetaVm,
  configuredAgents: SupportedAgentTypeVm[],
) {
  const display: SkillAgentDisplayMeta[] = [];
  const seen = new Set<string>();

  for (const sourceMeta of skillSourceAgents(skill, configuredAgents)) {
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
  if (!skill || skill.agentSource === '.gold-band') {
    return configuredAgents;
  }
  const sourceAgentTypes = new Set(
    skillSourceAgents(skill, configuredAgents).map((agent) => agent.agentType),
  );
  const syncedAgentTypes = new Set(skill.syncedAgentTypes);
  return configuredAgents.filter((agent) => (
    !sourceAgentTypes.has(agent.agentType) || syncedAgentTypes.has(agent.agentType)
  ));
}
