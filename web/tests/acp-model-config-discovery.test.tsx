/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { AcpModelThoughtSelects } from '@/components/acp/AcpModelThoughtSelects';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpSelectConfigOptionVm, ManagedAgentVm } from '@/types';
import i18n from '@/i18n';

const fetchConfig = vi.hoisted(() => vi.fn());
vi.mock('@/api', () => ({ fetchAgentModelConfig: fetchConfig }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;
const onModel = vi.fn();
const onOption = vi.fn();
const THOUGHT: AcpSelectConfigOptionVm = {
  id: 'reasoning_effort', category: 'thought_level', name: 'Reasoning', currentValue: 'high',
  options: [{ value: 'high', name: 'High' }],
} as AcpSelectConfigOptionVm;
beforeEach(async () => {
  await i18n.changeLanguage('zh-CN');
  fetchConfig.mockReset(); onModel.mockReset(); onOption.mockReset();
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  host = document.createElement('div'); document.body.append(host); root = createRoot(host);
});
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.unstubAllGlobals(); });
async function render(modelId: string, catalogs: Record<string, AcpSelectConfigOptionVm[]> = {}) {
  await act(async () => root.render(<TooltipProvider><AcpModelThoughtSelects
    agentType="cursor" models={[{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]}
    modelValue={modelId} configOptions={[]} modelBoundCatalogs={catalogs}
    onModelChange={onModel} onConfigOptionChange={onOption}
  /></TooltipProvider>));
}
async function toggleMenu() {
  const trigger = host.querySelector<HTMLButtonElement>('button[data-slot="dropdown-menu-trigger"]')!;
  await act(async () => {
    const event = new MouseEvent('pointerdown', { bubbles: true, button: 0 });
    Object.defineProperties(event, { pointerId: { value: 1 }, pointerType: { value: 'mouse' } });
    trigger.dispatchEvent(event);
  });
}
const menu = () => document.querySelector('[data-slot="dropdown-menu-content"]');
const discoveryItem = () => document.querySelector<HTMLElement>('[data-acp-model-config-discovery]');

it('lives in the dropdown instead of the toolbar and only fetches on selection', async () => {
  await render('a');
  expect(host.textContent).not.toContain('获取模型配置');
  expect(discoveryItem()).toBeNull();
  await toggleMenu();
  expect(discoveryItem()?.textContent).toBe('获取模型配置');
  expect(menu()?.textContent).toContain('尚未获取此模型的配置');
  expect(fetchConfig).not.toHaveBeenCalled();
  // Pinned to the bottom of the scrolling menu so long model lists never hide it.
  const footer = document.querySelector('[data-acp-model-config-discovery-footer]')!;
  expect(menu()?.lastElementChild).toBe(footer);
  expect(footer.classList).toContain('sticky');
  expect(footer.classList).toContain('-bottom-1');
  expect(footer.classList).toContain('bg-popover');
});

it('shows loading immediately, keeps the menu open, and reveals fetched sections in place', async () => {
  let resolve!: (agent: ManagedAgentVm) => void;
  fetchConfig.mockReturnValue(new Promise<ManagedAgentVm>((done) => { resolve = done; }));
  await render('a');
  await toggleMenu();
  await act(async () => discoveryItem()!.click());
  expect(fetchConfig).toHaveBeenCalledWith('cursor', 'a');
  expect(menu()).not.toBeNull();
  expect(discoveryItem()?.textContent).toBe('正在获取…');
  expect(discoveryItem()?.hasAttribute('data-disabled')).toBe(true);
  await act(async () => discoveryItem()!.click());
  expect(fetchConfig).toHaveBeenCalledTimes(1);
  // Success: the agent registry update delivers the observed catalog; single menu becomes composite.
  await act(async () => { resolve({} as ManagedAgentVm); });
  await render('a', { a: [THOUGHT] });
  expect(menu()).not.toBeNull();
  expect(document.querySelector('[data-acp-composite-section="thought_level"]')).not.toBeNull();
  expect(discoveryItem()).toBeNull();
  expect(onModel).not.toHaveBeenCalled(); expect(onOption).not.toHaveBeenCalled();
});

it('keeps failures retryable across menu reopen and ignores late results for another model', async () => {
  fetchConfig.mockRejectedValueOnce({ code: 'acp.model-config-unavailable', params: {} });
  await render('a');
  await toggleMenu();
  await act(async () => discoveryItem()!.click());
  await vi.waitFor(() => expect(discoveryItem()?.textContent).toBe('重试'));
  expect(menu()?.textContent).toContain('可以继续使用当前设置');
  await toggleMenu();
  expect(menu()).toBeNull();
  await toggleMenu();
  expect(discoveryItem()?.textContent).toBe('重试');

  let resolve!: (agent: ManagedAgentVm) => void;
  fetchConfig.mockReturnValueOnce(new Promise<ManagedAgentVm>((done) => { resolve = done; }));
  await act(async () => discoveryItem()!.click());
  await render('b');
  expect(discoveryItem()?.textContent).toBe('获取模型配置');
  await act(async () => { resolve({} as ManagedAgentVm); });
  expect(discoveryItem()?.textContent).toBe('获取模型配置');
  expect(onModel).not.toHaveBeenCalled(); expect(onOption).not.toHaveBeenCalled();
});

it('follows the caller-supplied observation for session composite sections', async () => {
  const renderSession = (modelConfigObserved: boolean) => act(async () => root.render(<TooltipProvider><AcpModelThoughtSelects
    agentType="cursor" models={[{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]}
    modelValue="b" compositeSections={[]} modelConfigObserved={modelConfigObserved}
    onModelChange={onModel} onConfigOptionChange={onOption}
  /></TooltipProvider>));
  await renderSession(false);
  await toggleMenu();
  expect(discoveryItem()?.textContent).toBe('获取模型配置');
  fetchConfig.mockResolvedValue({} as ManagedAgentVm);
  await act(async () => discoveryItem()!.click());
  expect(fetchConfig).toHaveBeenCalledWith('cursor', 'b');
  await renderSession(true);
  expect(menu()).not.toBeNull();
  expect(discoveryItem()).toBeNull();
});

it('treats an observed empty model catalog as fetched', async () => {
  await render('a', { a: [] });
  await toggleMenu();
  expect(menu()).not.toBeNull();
  expect(discoveryItem()).toBeNull();
  expect(fetchConfig).not.toHaveBeenCalled();
});
