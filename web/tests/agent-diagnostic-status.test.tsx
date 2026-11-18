/** @vitest-environment jsdom */
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
vi.mock('../src/api', () => ({
  createAgent: vi.fn(),
  deleteAgent: vi.fn(),
  doctorAgent: vi.fn(),
  getAgentBindingUsage: vi.fn(),
  previewAgentCacheRepair: vi.fn(),
  repairAgentCache: vi.fn(),
  updateAgent: vi.fn(),
}));
import i18n from '../src/i18n';
import { AgentConnectionGuide, DiagnosticBadge } from '../src/pages/AgentManagementPage';
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
let root: Root;
let container: HTMLDivElement;
beforeEach(async () => {
  await i18n.changeLanguage('zh-CN');
  container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
});
it('shows a stale diagnostic as needing recheck instead of normal', async () => {
  await act(async () => root.render(<DiagnosticBadge diagnostic={{ status: 'stale', available: false, checkedAt: '2026-11-12T00:00:00Z' }} />));
  expect(container.textContent).toContain('需重新诊断');
  expect(container.textContent).not.toContain('正常');
});

it('shows launch, PATH and check requirements before creating an Agent', async () => {
  await act(async () => root.render(<AgentConnectionGuide
    requirements={{ pathCommand: 'dsh', pathArgs: ['--profile', 'acp'], installedCli: null, npxFallback: null, npxRequiresPreparedPackage: false }}
    command="dsh"
    args={['--profile', 'acp']}
    diagnostic={null}
    mode="create"
    formChanged
    checking={false}
    readOnly={false}
  />));
  expect(container.textContent).toContain('接入条件');
  expect(container.textContent).toContain('dsh --profile acp');
  expect(container.textContent).toContain('PATH');
  expect(container.textContent).toContain('保存后会自动执行接入检查');
  expect(container.textContent).toContain('initialize');
});

it('shows the missing authentication variable instead of the raw provider message', async () => {
  await i18n.changeLanguage('zh-CN');
  await act(async () => root.render(<AgentConnectionGuide
    requirements={{ pathCommand: 'qodercli', pathArgs: ['--acp'], installedCli: 'qodercli', npxFallback: { command: 'npx', args: ['-y', '@qoder-ai/qodercli@1.1.86', '--acp'] }, npxRequiresPreparedPackage: true }}
    command="npx"
    args={['-y', '@qoder-ai/qodercli@1.1.86', '--acp']}
    diagnostic={{
      status: 'unhealthy',
      available: false,
      checkedAt: '2026-11-16T00:00:00Z',
      error: {
        code: 'provider.auth-required',
        params: { methods: ['qoder-personal-access-token'], missingEnv: ['QODER_PERSONAL_ACCESS_TOKEN'] },
        raw: { message: 'Authentication required' },
      },
    }}
    mode="edit"
    formChanged={false}
    checking={false}
    readOnly={false}
  />));

  expect(container.textContent).toContain('QODER_PERSONAL_ACCESS_TOKEN');
  expect(container.textContent).not.toContain('Authentication required');
});

it('marks an edited configuration as needing recheck and does not offer a stale immediate check', async () => {
  const check = vi.fn();
  await act(async () => root.render(<AgentConnectionGuide
    requirements={{ pathCommand: 'claude-agent-acp', pathArgs: [], installedCli: 'claude', npxFallback: { command: 'npx', args: ['-y', '@agentclientprotocol/claude-agent-acp@0.90.0'] }, npxRequiresPreparedPackage: false }}
    command="npx"
    args={['-y', '@agentclientprotocol/claude-agent-acp@0.90.0']}
    diagnostic={{ status: 'healthy', available: true, checkedAt: '2026-11-12T00:00:00Z' }}
    mode="edit"
    formChanged
    checking={false}
    readOnly={false}
    onCheck={check}
  />));
  expect(container.textContent).toContain('claude-agent-acp');
  expect(container.textContent).toContain('当前表单已修改');
  expect(container.textContent).toContain('需重新诊断');
  expect(Array.from(container.querySelectorAll('button')).some((button) => button.textContent?.includes('立即检查'))).toBe(false);
  expect(check).not.toHaveBeenCalled();
});
