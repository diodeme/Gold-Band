// @vitest-environment jsdom
import { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { AgentManagementPage } from '../src/pages/AgentManagementPage';
import { mockAgentRegistry } from '../src/mockData';
import { doctorAgent } from '../src/api';
import i18n from '../src/i18n';
import type { AgentRegistryVm } from '../src/types';

vi.mock('../src/api', async (importOriginal) => ({
  ...await importOriginal<typeof import('../src/api')>(),
  doctorAgent: vi.fn(),
}));
vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
let cleanup: (() => void) | undefined;
afterEach(() => { cleanup?.(); vi.clearAllMocks(); });

it('a completed diagnostic clears its spinner and permits retry while another Agent has no result', async () => {
  const initial = structuredClone(mockAgentRegistry);
  initial.agents = [initial.agents[0], { ...initial.agents[0], agentType: 'codebuddy-code', displayName: 'CodeBuddy' }];
  for (const agent of initial.agents) agent.diagnostic = undefined;
  let finish!: (result: AgentRegistryVm) => void;
  vi.mocked(doctorAgent).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  function Harness() {
    const [vm, setVm] = useState(initial);
    return <AgentManagementPage vm={vm} loading={false} onRefresh={() => {}} onRegistryChange={setVm} />;
  }
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  cleanup = () => { act(() => root.unmount()); container.remove(); };
  await act(async () => root.render(<Harness />));
  const buttons = [...container.querySelectorAll('button')].filter((button) => button.textContent === i18n.t('agentManagement.diagnose'));
  await act(async () => buttons[1].click());
  expect(buttons[1].getAttribute('aria-busy')).toBe('true');
  expect(buttons[1].disabled).toBe(true);
  expect(buttons[0].disabled).toBe(false);
  await act(async () => buttons[1].click());
  expect(doctorAgent).toHaveBeenCalledTimes(1);

  const completed = structuredClone(initial);
  completed.agents[1].diagnostic = {
    status: 'unhealthy',
    available: false,
    checkedAt: new Date().toISOString(),
    error: {
      code: 'acp.adapter-exited',
      params: {
        method: 'initialize',
        exitCode: 1,
        reason: 'npm error code ENOENT\nnpm error path C:\\Users\\Administrator\\AppData\\Local\\npm-cache\\_npx\\dead\\package.json',
      },
    },
  };
  await act(async () => finish(completed));
  expect(buttons[1].getAttribute('aria-busy')).toBe('false');
  expect(buttons[1].disabled).toBe(false);
  expect(buttons[1].textContent).toBe(i18n.t('agentManagement.diagnose'));
  const localized = i18n.t('errors.acp.adapter-exited-with-code', { method: 'initialize', exitCode: 1 });
  expect(container.textContent).toContain(i18n.t('agentManagement.diagnosticFailed', { reason: localized }));
  expect(container.textContent).toContain('ENOENT');
  expect(container.textContent).toContain('package.json');
  expect(container.textContent).not.toContain('ACP adapter transport interrupted');
  expect(buttons[0].disabled).toBe(false);
});
