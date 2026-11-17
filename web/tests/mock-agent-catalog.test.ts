import { describe, expect, it } from 'vitest';
import { mockAgentRegistry } from '@/mockData';

describe('mock Agent registry', () => {
  it('initializes the catalog with complete connection requirements', () => {
    const catalog = mockAgentRegistry.catalog;
    expect(catalog.length).toBeGreaterThan(0);
    expect(catalog.every((entry) => entry.connectionRequirements)).toBe(true);

    expect(catalog.find((entry) => entry.agentType === 'claude-acp')?.connectionRequirements)
      .toEqual({
        pathCommand: 'claude-agent-acp',
        pathArgs: [],
        installedCli: 'claude',
        npxFallback: { command: 'npx', args: ['-y', '@agentclientprotocol/claude-agent-acp@0.65.0'] },
        npxRequiresPreparedPackage: false,
      });
    expect(catalog.find((entry) => entry.agentType === 'amp-acp')?.connectionRequirements)
      .toMatchObject({
        pathCommand: 'amp-acp',
        pathArgs: [],
        installedCli: null,
        npxFallback: null,
      });
  });
});