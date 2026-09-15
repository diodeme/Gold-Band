import type { AgentRegistryVm, ManagedAgentVm } from '@/types';

function configuration(agent: ManagedAgentVm) {
  const { diagnostic: _diagnostic, supportedModes: _modes, supportedModels: _models,
    configOptions: _options, mcpHttpSupported: _http, mcpSseSupported: _sse, ...config } = agent;
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
