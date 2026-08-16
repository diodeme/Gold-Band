// @vitest-environment jsdom

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  createAgent: vi.fn(),
  deleteAgent: vi.fn(),
  doctorAgent: vi.fn(),
  getAgentBindingUsage: vi.fn(),
  updateAgent: vi.fn(),
}));

vi.mock('../src/api', () => api);
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn() }));

import i18n from '../src/i18n';
import { AgentManagementPage } from '../src/pages/AgentManagementPage';
import type { AgentRegistryVm } from '../src/types';

let cleanup: (() => void) | null = null;

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.clearAllMocks();
});

const registry: AgentRegistryVm = {
  agents: [{
    agentType: 'claude-acp',
    displayName: 'Claude',
    command: 'npx',
    args: [],
    env: [],
    iconKey: 'claude',
    primaryAgentDir: '.claude',
    projectPrimaryAgentDir: null,
    compatibleAgentDirs: [],
    supportsSystemPrompt: true,
    externalSessionSyncSupported: false,
    externalSessionSyncEnabled: false,
    diagnostic: null,
  }],
  catalog: [],
};

function button(label: string, position: 'first' | 'last' = 'first') {
  const matches = Array.from(document.body.querySelectorAll('button'))
    .filter((candidate) => candidate.textContent?.trim() === label);
  return position === 'first' ? matches[0] : matches[matches.length - 1];
}

describe('Agent deletion usage confirmation', () => {
  it('keeps deletion disabled through loading and failure, then enables it after retry succeeds', async () => {
    await i18n.changeLanguage('zh-CN');
    let rejectUsage: ((reason: Error) => void) | null = null;
    api.getAgentBindingUsage
      .mockReturnValueOnce(new Promise((_, reject) => {
        rejectUsage = reject;
      }))
      .mockResolvedValueOnce({
        workflowTemplateCount: 1,
        taskCount: 2,
        scheduledTaskCount: 3,
        unknownTaskCount: 0,
        unknownScheduledTaskCount: 0,
      });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);
    cleanup = () => {
      act(() => root.unmount());
      container.remove();
    };

    await act(async () => root.render(
      <AgentManagementPage
        vm={registry}
        loading={false}
        onRefresh={() => undefined}
        onRegistryChange={() => undefined}
      />,
    ));
    await act(async () => {
      button('删除')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });

    expect(document.body.textContent).toContain('正在统计绑定引用');
    expect((button('确认删除') as HTMLButtonElement | undefined)?.disabled).toBe(true);

    await act(async () => rejectUsage?.(new Error('read failed')));

    expect(document.body.textContent).toContain('无法加载绑定引用');
    expect(button('重新统计')).toBeDefined();
    expect((button('确认删除') as HTMLButtonElement | undefined)?.disabled).toBe(true);

    await act(async () => {
      button('重新统计')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });

    expect(api.getAgentBindingUsage).toHaveBeenCalledTimes(2);
    expect(document.body.textContent).toContain('受影响的定时任务');
    expect((button('确认删除') as HTMLButtonElement | undefined)?.disabled).toBe(false);
  });

  it('shows unknown references without blocking explicit deletion', async () => {
    await i18n.changeLanguage('zh-CN');
    api.getAgentBindingUsage.mockResolvedValue({
      workflowTemplateCount: 0,
      taskCount: 1,
      scheduledTaskCount: 0,
      unknownTaskCount: 2,
      unknownScheduledTaskCount: 1,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);
    cleanup = () => {
      act(() => root.unmount());
      container.remove();
    };

    await act(async () => root.render(
      <AgentManagementPage
        vm={registry}
        loading={false}
        onRefresh={() => undefined}
        onRegistryChange={() => undefined}
      />,
    ));
    await act(async () => {
      button('删除')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });

    expect(document.body.textContent).toContain('无法确认的 Task');
    expect(document.body.textContent).toContain('引用关系无法完全确认');
    expect((button('确认删除') as HTMLButtonElement | undefined)?.disabled).toBe(false);
  });
});
