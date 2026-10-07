/** @vitest-environment jsdom */
import { expect, it, vi } from 'vitest';
import { AGENT_MODEL_CONFIG_UPDATED_EVENT } from '@/lib/agent-diagnostic-update';
import type { ManagedAgentVm } from '@/types';
const fetchConfig = vi.hoisted(() => vi.fn());
vi.mock('@/api/client', () => ({ getRuntimeApi: () => ({ fetchAgentModelConfig: fetchConfig }) }));
import { fetchAgentModelConfig } from '@/api';

it('coalesces duplicate discovery, publishes the matching model once, and releases failed requests', async () => {
  const listener = vi.fn();
  window.addEventListener(AGENT_MODEL_CONFIG_UPDATED_EVENT, listener);
  try {
    let resolve!: (agent: ManagedAgentVm) => void;
    fetchConfig.mockReturnValueOnce(new Promise<ManagedAgentVm>((done) => { resolve = done; }));
    const first = fetchAgentModelConfig('agent-a', 'model');
    const duplicate = fetchAgentModelConfig('agent-a', 'model');
    expect(first).toBe(duplicate);
    expect(fetchConfig).toHaveBeenCalledTimes(1);
    resolve({ agentType: 'agent-a', modelBoundCatalogs: { model: [] } } as unknown as ManagedAgentVm);
    await first;
    expect(listener).toHaveBeenCalledTimes(1);
    expect((listener.mock.calls[0][0] as CustomEvent).detail.modelId).toBe('model');

    fetchConfig.mockResolvedValueOnce({ modelBoundCatalogs: { other: [] } });
    await expect(fetchAgentModelConfig('agent-a', 'missing')).rejects.toMatchObject({ code: 'acp.model-config-unavailable' });
    expect(listener).toHaveBeenCalledTimes(1);
    fetchConfig.mockResolvedValueOnce({ agentType: 'agent-a', modelBoundCatalogs: { missing: [] } });
    await fetchAgentModelConfig('agent-a', 'missing');
    expect(listener).toHaveBeenCalledTimes(2);
  } finally { window.removeEventListener(AGENT_MODEL_CONFIG_UPDATED_EVENT, listener); }
});
