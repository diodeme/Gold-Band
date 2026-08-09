import type { AgentRegistryVm, ManagedAgentVm, SkillMetaVm } from '../types';

export interface SkillAgentDisplayMeta {
  agentType: string;
  label: string;
  iconKey: string;
}

export interface ConfiguredSkillAgentMeta extends SkillAgentDisplayMeta {
  primaryAgentDir: string;
  projectPrimaryAgentDir: string | null;
  compatibleAgentDirs: string[];
}

export const GOLD_BAND_AGENT_META: SkillAgentDisplayMeta = {
  agentType: 'gold-band',
  label: 'Gold Band',
  iconKey: 'gold-band',
};

function skillAgentMeta(agent: ManagedAgentVm): ConfiguredSkillAgentMeta {
  return {
    agentType: agent.agentType,
    label: agent.displayName,
    iconKey: agent.iconKey,
    primaryAgentDir: agent.primaryAgentDir,
    projectPrimaryAgentDir: agent.projectPrimaryAgentDir,
    compatibleAgentDirs: agent.compatibleAgentDirs,
  };
}

export function configuredSkillAgents(registry: AgentRegistryVm | null): ConfiguredSkillAgentMeta[] {
  if (!registry) return [];
  const managedByType = new Map(registry.agents.map((agent) => [agent.agentType, agent]));
  const ordered: ConfiguredSkillAgentMeta[] = [];
  const seen = new Set<string>();

  for (const catalogAgent of registry.catalog) {
    const managed = managedByType.get(catalogAgent.agentType);
    if (!managed) continue;
    ordered.push(skillAgentMeta(managed));
    seen.add(managed.agentType);
  }
  for (const managed of registry.agents) {
    if (seen.has(managed.agentType)) continue;
    ordered.push(skillAgentMeta(managed));
  }
  return ordered;
}

export function skillSourceAgents(
  skill: SkillMetaVm,
  configuredAgents: ConfiguredSkillAgentMeta[],
) {
  if (skill.agentSource === '.gold-band') return [GOLD_BAND_AGENT_META];
  return configuredAgents.filter((agent) => (
    (skill.source === 'project'
      ? agent.projectPrimaryAgentDir ?? agent.primaryAgentDir
      : agent.primaryAgentDir) === skill.agentSource
    || agent.compatibleAgentDirs.includes(skill.agentSource)
  ));
}

export function skillAvailableAgentTypes(
  skill: SkillMetaVm,
  configuredAgents: ConfiguredSkillAgentMeta[],
) {
  const available = new Set(skill.syncedAgentTypes);
  for (const sourceMeta of skillSourceAgents(skill, configuredAgents)) {
    if (sourceMeta.agentType !== GOLD_BAND_AGENT_META.agentType) available.add(sourceMeta.agentType);
  }
  return [...available];
}

export function skillDisplayAgents(
  skill: SkillMetaVm,
  configuredAgents: ConfiguredSkillAgentMeta[],
) {
  const display: SkillAgentDisplayMeta[] = [];
  const seen = new Set<string>();
  for (const sourceMeta of skillSourceAgents(skill, configuredAgents)) {
    display.push(sourceMeta);
    seen.add(sourceMeta.agentType);
  }
  for (const agentType of skill.syncedAgentTypes) {
    const meta = configuredAgents.find((agent) => agent.agentType === agentType);
    if (!meta || seen.has(agentType)) continue;
    display.push(meta);
    seen.add(agentType);
  }
  return display;
}

export function selectableSyncAgents(
  skill: SkillMetaVm | null,
  configuredAgents: ConfiguredSkillAgentMeta[],
) {
  if (!skill || skill.agentSource === '.gold-band') return configuredAgents;
  const sourceAgentTypes = new Set(
    skillSourceAgents(skill, configuredAgents).map((agent) => agent.agentType),
  );
  const syncedAgentTypes = new Set(skill.syncedAgentTypes);
  return configuredAgents.filter((agent) => (
    !sourceAgentTypes.has(agent.agentType) || syncedAgentTypes.has(agent.agentType)
  ));
}
