import { describe, expect, it } from 'vitest';

import {
  mcpAgentSupportStatus,
  shouldShowCompatibilityLoading,
  type McpAgentCompatibility,
} from '../src/lib/mcp-agent-compatibility';

const healthyAgent: McpAgentCompatibility = {
  agentType: 'test-agent',
  label: 'Test Agent',
  iconKey: 'agent',
  diagnosticAvailable: true,
  mcpHttpSupported: true,
  mcpSseSupported: false,
};

describe('MCP agent compatibility state', () => {
  it('gates all transport compatibility behind agent health without treating failure as unsupported', () => {
    const unavailable = {
      ...healthyAgent,
      diagnosticAvailable: false,
      diagnosticReason: 'adapter not found',
    };

    expect(mcpAgentSupportStatus('stdio', unavailable)).toBe('unavailable');
    expect(mcpAgentSupportStatus('http', unavailable)).toBe('unavailable');
    expect(mcpAgentSupportStatus('sse', unavailable)).toBe('unavailable');
  });

  it('uses the persisted capability snapshot for healthy agents', () => {
    expect(mcpAgentSupportStatus('stdio', healthyAgent)).toBe('supported');
    expect(mcpAgentSupportStatus('http', healthyAgent)).toBe('supported');
    expect(mcpAgentSupportStatus('sse', healthyAgent)).toBe('unsupported');
    expect(mcpAgentSupportStatus('http', {
      ...healthyAgent,
      mcpHttpSupported: null,
    })).toBe('unknown');
  });

  it('keeps the previous known state visible while doctor refreshes', () => {
    expect(shouldShowCompatibilityLoading('supported', true)).toBe(false);
    expect(shouldShowCompatibilityLoading('unsupported', true)).toBe(false);
    expect(shouldShowCompatibilityLoading('unavailable', true)).toBe(false);
    expect(shouldShowCompatibilityLoading('unknown', true)).toBe(true);
  });
});
