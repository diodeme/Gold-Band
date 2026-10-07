import type { AgentRegistryVm, ManagedAgentVm } from '@/types';

export const AGENT_MODEL_CONFIG_UPDATED_EVENT = 'gold-band:agent-model-config-updated';
export type AgentModelConfigUpdate = { agent: ManagedAgentVm; modelId: string };

export function applyAgentModelConfigUpdate(current: AgentRegistryVm | null, update: AgentModelConfigUpdate) {
  if (!current) return current;
  const catalog = update.agent.modelBoundCatalogs?.[update.modelId];
  if (!catalog) return current;
  return { ...current, agents: current.agents.map((agent) => {
    if (agent.agentType !== update.agent.agentType || configuration(agent) !== configuration(update.agent)) return agent;
    // A live update may have arrived while discovery was in flight. It takes precedence.
    if (Object.prototype.hasOwnProperty.call(agent.modelBoundCatalogs ?? {}, update.modelId)) return agent;
    return { ...agent, modelBoundCatalogs: { ...agent.modelBoundCatalogs, [update.modelId]: catalog } };
  }) };
}

function configuration(agent: ManagedAgentVm) {
  const { diagnostic: _diagnostic, supportedModes: _modes, supportedModels: _models,
    configOptions: _options, modelBoundCatalogs: _catalogs, mcpHttpSupported: _http, mcpSseSupported: _sse, ...config } = agent;
  return JSON.stringify(config);
}

export function applyAgentDiagnosticUpdate(current: AgentRegistryVm | null, incoming: ManagedAgentVm) {
  if (!current) return current;
  const index = current.agents.findIndex(agent => agent.agentType === incoming.agentType);
  if (index < 0) return current;
  const previous = current.agents[index];
  if (configuration(previous) !== configuration(incoming)
    || (previous.diagnostic && incoming.diagnostic
      && Number.parseInt(previous.diagnostic.checkedAt) > Number.parseInt(incoming.diagnostic.checkedAt))) return current;
  const agents = [...current.agents];
  agents[index] = incoming;
  return { ...current, agents };
}
