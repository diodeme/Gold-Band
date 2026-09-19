import { expect, it } from 'vitest';
import { applyAgentDiagnosticUpdate } from '../src/lib/agent-diagnostic-update';
import { mockAgentRegistry } from '../src/mockData';

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
