// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { AgentManagementPage } from '../src/pages/AgentManagementPage';
import { mockAgentRegistry } from '../src/mockData';
import i18n from '../src/i18n';

vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
let cleanup: (() => void) | undefined;
afterEach(() => cleanup?.());

it.each([true, false])('launch fields are read-only only for catalog agents: %s', async (builtin) => {
  const vm = structuredClone(mockAgentRegistry);
  vm.agents = [vm.agents[0]];
  if (!builtin) vm.agents[0].agentType = 'my-private-agent';
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  cleanup = () => { act(() => root.unmount()); container.remove(); };
  await act(async () => root.render(<AgentManagementPage vm={vm} loading={false} onRefresh={() => {}} onRegistryChange={() => {}} />));
  const edit = [...container.querySelectorAll('button')].find((button) => button.textContent === i18n.t('agentManagement.edit'))!;
  await act(async () => edit.click());
  const field = (key: string) => [...document.querySelectorAll('label')].find((label) => label.textContent?.startsWith(i18n.t(key)))!;
  const command = field('agentManagement.command').querySelector('input')!;
  const args = field('agentManagement.args').querySelector('textarea')!;
  expect(command.readOnly).toBe(builtin);
  expect(args.readOnly).toBe(builtin);
  expect(command.disabled).toBe(false);
  expect(args.disabled).toBe(false);
  expect(command.classList.contains('bg-muted/50')).toBe(builtin);
  expect(args.classList.contains('bg-muted/50')).toBe(builtin);
  expect(command.classList.contains('text-muted-foreground')).toBe(builtin);
  expect(args.classList.contains('text-muted-foreground')).toBe(builtin);
  expect(field('agentManagement.displayName').querySelector('input')!.readOnly).toBe(false);
  expect(field('agentManagement.env').querySelector('textarea')!.readOnly).toBe(false);
});
