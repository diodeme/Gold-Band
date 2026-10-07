import { expect, it } from 'vitest';
import { applyAgentDiagnosticUpdate, applyAgentModelConfigUpdate } from '../src/lib/agent-diagnostic-update';
import { mockAgentRegistry } from '../src/mockData';

it('merges only the probed model and rejects stale configuration or newer live observations', () => {
  const registry = structuredClone(mockAgentRegistry);
  const agent = registry.agents[0];
  agent.modelBoundCatalogs = { existing: [] };
  const incoming = { ...agent, modelBoundCatalogs: { target: [] } };
  const update = { agent: incoming, modelId: 'target' };
  const next = applyAgentModelConfigUpdate(registry, update)!;
  expect(next.agents[0].modelBoundCatalogs).toEqual({ existing: [], target: [] });
  expect(next.agents[1]).toBe(registry.agents[1]);
  expect(applyAgentModelConfigUpdate(next, update)?.agents[0]).toBe(next.agents[0]);
  expect(applyAgentModelConfigUpdate(registry, { ...update, agent: { ...incoming, command: 'changed' } })?.agents[0]).toBe(agent);
});

it('updates only the matching agent and rejects diagnostics for an old configuration', () => {
  const registry = structuredClone(mockAgentRegistry);
  registry.agents[0].diagnostic = null;
  const update = { ...registry.agents[0], diagnostic: { status: 'healthy', available: true, checkedAt: '100Z', error: null } };
  const next = applyAgentDiagnosticUpdate(registry, update)!;
  expect(next.agents[0]).toBe(update);
  expect(next.catalog).toBe(registry.catalog);
  expect(next.agents[1]).toBe(registry.agents[1]);
  expect(applyAgentDiagnosticUpdate(next, { ...update, command: 'old-command' })).toBe(next);
  expect(applyAgentDiagnosticUpdate(next, { ...update, diagnostic: { ...update.diagnostic, checkedAt: '99Z' } })).toBe(next);
});

it('applies a later modelBoundCatalogs observation without a newer doctor timestamp', () => {
  const registry = structuredClone(mockAgentRegistry);
  registry.agents[0] = {
    ...registry.agents[0],
    diagnostic: { status: 'healthy', available: true, checkedAt: '100Z', error: null },
    modelBoundCatalogs: {
      grok: [{ id: 'fast', category: 'model_config', name: 'Fast', options: [{ value: 'true', name: 'On' }] }],
    },
  };
  const update = {
    ...registry.agents[0],
    modelBoundCatalogs: {
      grok: [{ id: 'fast', category: 'model_config', name: 'Fast', options: [{ value: 'true', name: 'On' }] }],
      luna: [{ id: 'context', category: 'model_config', name: 'Context', options: [{ value: '1m', name: '1M' }] }],
    },
  };

  expect(applyAgentDiagnosticUpdate(registry, update)?.agents[0].modelBoundCatalogs).toEqual(update.modelBoundCatalogs);
});
