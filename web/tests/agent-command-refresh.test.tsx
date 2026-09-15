// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { useAgentCommands } from '../src/hooks/useAgentCommands';
import { getAgentCommandCatalog } from '../src/api';
import type { AcpCommandCatalogVm } from '../src/types';

const handlers = vi.hoisted(() => new Set<(event: { payload: unknown }) => void>());
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (_name, handler) => {
  handlers.add(handler); return () => handlers.delete(handler);
}) }));
vi.mock('../src/api/shared', () => ({ isTauriRuntime: () => true }));
vi.mock('../src/api', () => ({ getAgentCommandCatalog: vi.fn() }));
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); handlers.clear(); vi.clearAllMocks(); });

it('shares requests and ignores diagnostics for another agent or workspace', async () => {
  vi.mocked(getAgentCommandCatalog).mockResolvedValue(null);
  function Consumer() { useAgentCommands('codex-acp', 'C:/repo'); return null; }
  const host = document.createElement('div');
  const root = createRoot(host);
  dispose = () => act(() => root.unmount());
  await act(async () => root.render(<><Consumer /><Consumer /></>));
  expect(getAgentCommandCatalog).toHaveBeenCalledTimes(1);
  vi.mocked(getAgentCommandCatalog).mockClear();
  await act(async () => {
    for (const handler of handlers) handler({ payload: { agentType: 'claude-acp', workspacePath: 'C:/repo' } });
    for (const handler of handlers) handler({ payload: { agentType: 'codex-acp', workspacePath: 'C:/other' } });
  });
  expect(getAgentCommandCatalog).not.toHaveBeenCalled();
  await act(async () => {
    for (const handler of handlers) handler({ payload: { agentType: 'codex-acp', workspacePath: 'C:/repo' } });
  });
  expect(getAgentCommandCatalog).toHaveBeenCalledTimes(1);
});

it('filters production project events and coalesces an in-flight burst into one trailing read', async () => {
  const catalog = { projectId: 'project-1', commands: [], skillCommands: [] } as unknown as AcpCommandCatalogVm;
  let finish!: (catalog: AcpCommandCatalogVm) => void;
  vi.mocked(getAgentCommandCatalog).mockResolvedValueOnce(catalog)
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValue(catalog);
  function Consumer() { useAgentCommands('codex-acp', 'C:/repo'); return null; }
  const root = createRoot(document.createElement('div'));
  dispose = () => act(() => root.unmount());
  await act(async () => root.render(<><Consumer /><Consumer /></>));
  const emit = (projectId: string) => {
    for (const handler of handlers) handler({ payload: { agentType: 'codex-acp', projectId } });
  };
  await act(async () => emit('project-2'));
  expect(getAgentCommandCatalog).toHaveBeenCalledTimes(1);
  await act(async () => emit('project-1'));
  await act(async () => { for (let i = 0; i < 20; i++) emit('project-1'); });
  expect(getAgentCommandCatalog).toHaveBeenCalledTimes(2);
  await act(async () => finish(catalog));
  expect(getAgentCommandCatalog).toHaveBeenCalledTimes(3);
});

it('does not publish a late catalog after changing workspace', async () => {
  let finish!: (catalog: AcpCommandCatalogVm) => void;
  vi.mocked(getAgentCommandCatalog).mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValue({ commands: [{ name: 'new-command', description: 'new' }] } as AcpCommandCatalogVm);
  function Consumer({ path }: { path: string }) {
    const { commands } = useAgentCommands('codex-acp', path);
    return <div>{commands.map(command => command.name).join(',')}</div>;
  }
  const host = document.createElement('div');
  const root = createRoot(host);
  dispose = () => act(() => root.unmount());
  await act(async () => root.render(<Consumer path="C:/old" />));
  await act(async () => root.render(<Consumer path="C:/new" />));
  expect(host.textContent).toContain('new-command');
  await act(async () => finish({ commands: [{ name: 'old-command', description: 'old' }] } as AcpCommandCatalogVm));
  expect(host.textContent).toContain('new-command');
  expect(host.textContent).not.toContain('old-command');
});
