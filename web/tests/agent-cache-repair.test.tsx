/** @vitest-environment jsdom */
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { beforeEach, afterEach, expect, it, vi } from 'vitest';
const api = vi.hoisted(() => ({ previewAgentCacheRepair: vi.fn(), repairAgentCache: vi.fn(), doctorAgent: vi.fn() }));
vi.mock('../src/api', () => api);
import i18n from '../src/i18n';
import { AgentCacheRepair } from '../src/components/AgentCacheRepair';
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
let root: Root;
let container: HTMLDivElement;
const changed = vi.fn();
const busy = vi.fn();
beforeEach(async () => {
  vi.clearAllMocks();
  await i18n.changeLanguage('zh-CN');
  container = document.createElement('div'); document.body.append(container); root = createRoot(container);
  api.previewAgentCacheRepair.mockResolvedValue({ token: 'snapshot-1', paths: ['C:/cache/_npx/a', 'C:/cache/_npx/b'] });
  api.repairAgentCache.mockResolvedValue([{ path: 'C:/cache/_npx/a', errorCode: null }]);
  api.doctorAgent.mockResolvedValue({ agents: [{ agentType: 'claude-acp', diagnostic: { available: true } }] });
  await act(async () => root.render(<AgentCacheRepair agentType="claude-acp" disabled={false} onRegistryChange={changed} onBusyChange={busy} />));
});
afterEach(async () => { await act(async () => root.unmount()); container.remove(); });
async function click(text: string) {
  const button = [...document.querySelectorAll('button')].find((b) => b.textContent === text);
  expect(button).toBeTruthy(); await act(async () => button!.click());
}
it('requires confirmation, submits identity and token only, and diagnoses after cleanup', async () => {
  await click('修复缓存');
  expect(api.repairAgentCache).not.toHaveBeenCalled();
  expect(document.body.textContent).toContain('其他使用此缓存的应用');
  expect(document.body.textContent).toContain('正在使用这些缓存的 Agent 可能中断');
  expect(document.body.textContent).toContain('C:/cache/_npx/b');
  await click('清理并重新检测');
  expect(api.repairAgentCache).toHaveBeenCalledExactlyOnceWith('claude-acp', 'snapshot-1');
  expect(api.doctorAgent).toHaveBeenCalledExactlyOnceWith('claude-acp');
  expect(changed).toHaveBeenCalledTimes(1);
  expect(document.body.textContent).toContain('Agent 检测通过');
});
it('shows loading immediately and cancellation never deletes', async () => {
  let resolve!: (value: { token: string; paths: string[] }) => void;
  api.previewAgentCacheRepair.mockReturnValue(new Promise((r) => { resolve = r; }));
  await click('修复缓存');
  expect(document.body.textContent).toContain('正在检查缓存目录');
  await act(async () => resolve({ token: 't', paths: ['C:/cache/_npx/a'] }));
  await click('关闭');
  expect(api.repairAgentCache).not.toHaveBeenCalled();
  expect(busy).toHaveBeenLastCalledWith(false);
});
it('reports partial failures without claiming recovery or diagnosing', async () => {
  api.repairAgentCache.mockResolvedValue([{ path: 'a', errorCode: null }, { path: 'b', errorCode: 'npx-cache.cleanup-failed' }]);
  await click('修复缓存'); await click('清理并重新检测');
  expect(document.querySelector('[role=alert]')?.textContent).toContain('缓存清理失败');
  expect(api.doctorAgent).not.toHaveBeenCalled();
});
it('prevents repeated destructive requests while repairing', async () => {
  let resolve!: (value: { path: string; errorCode: null }[]) => void;
  api.repairAgentCache.mockReturnValue(new Promise((r) => { resolve = r; }));
  await click('修复缓存'); await click('清理并重新检测');
  expect(document.body.textContent).toContain('正在清理缓存');
  expect([...document.querySelectorAll('button')].find((b) => b.textContent === '清理并重新检测')).toBeUndefined();
  await act(async () => resolve([{ path: 'a', errorCode: null }]));
  expect(api.repairAgentCache).toHaveBeenCalledTimes(1);
});
