/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AcpModelThoughtSelects } from '@/components/acp/AcpModelThoughtSelects';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { ManagedAgentVm } from '@/types';
import i18n from '@/i18n';

const fetchConfig = vi.hoisted(() => vi.fn());
vi.mock('@/api', () => ({ fetchAgentModelConfig: fetchConfig }));
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;
const onModel = vi.fn();
const onOption = vi.fn();
beforeEach(async () => {
  await i18n.changeLanguage('zh-CN');
  fetchConfig.mockReset(); onModel.mockReset(); onOption.mockReset();
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  host = document.createElement('div'); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.unstubAllGlobals(); });
async function render(modelId: string, observed = false) {
  await act(async () => root.render(<TooltipProvider><AcpModelThoughtSelects
    agentType="cursor" models={[{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]}
    modelValue={modelId} configOptions={[]} modelBoundCatalogs={observed ? { [modelId]: [] } : {}}
    onModelChange={onModel} onConfigOptionChange={onOption}
  /></TooltipProvider>));
}
const discoveryButton = () => host.querySelector<HTMLButtonElement>('[data-slot="model-config-discovery"] button');

it('fetches only on click, shows loading immediately, and never changes the user selection', async () => {
  let resolve!: (agent: ManagedAgentVm) => void;
  fetchConfig.mockReturnValue(new Promise<ManagedAgentVm>((done) => { resolve = done; }));
  await render('a');
  expect(fetchConfig).not.toHaveBeenCalled();
  expect(discoveryButton()?.textContent).toBe('获取模型配置');
  await act(async () => discoveryButton()!.click());
  expect(discoveryButton()?.disabled).toBe(true);
  expect(discoveryButton()?.textContent).toBe('正在获取…');
  await vi.waitFor(() => expect(fetchConfig).toHaveBeenCalledWith('cursor', 'a'));
  await act(async () => { resolve({} as ManagedAgentVm); });
  expect(discoveryButton()).toBeNull();
  expect(onModel).not.toHaveBeenCalled(); expect(onOption).not.toHaveBeenCalled();
});

it('keeps failures retryable and does not apply late completion to a different model', async () => {
  fetchConfig.mockRejectedValueOnce({ code: 'acp.model-config-unavailable', params: {} });
  await render('a');
  await act(async () => discoveryButton()!.click());
  await vi.waitFor(() => expect(discoveryButton()?.textContent).toBe('重试'));
  expect(host.textContent).toContain('可以继续使用当前设置');
  let resolve!: (agent: ManagedAgentVm) => void;
  fetchConfig.mockReturnValueOnce(new Promise<ManagedAgentVm>((done) => { resolve = done; }));
  await act(async () => discoveryButton()!.click());
  await render('b');
  expect(discoveryButton()?.textContent).toBe('获取模型配置');
  await act(async () => { resolve({} as ManagedAgentVm); });
  expect(discoveryButton()?.textContent).toBe('获取模型配置');
  expect(onModel).not.toHaveBeenCalled(); expect(onOption).not.toHaveBeenCalled();
});

it('treats an observed empty model catalog as fetched', async () => {
  await render('a', true);
  expect(discoveryButton()).toBeNull();
  expect(fetchConfig).not.toHaveBeenCalled();
});
