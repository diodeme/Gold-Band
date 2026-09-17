/** @vitest-environment jsdom */

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const browser = vi.hoisted(() => ({
  openWebTarget: vi.fn().mockResolvedValue({ status: 'opened', kind: 'browser', pageId: 'page-1' }),
}));

vi.mock('@/components/workspace/browser/open-web-target', () => ({
  openWebTarget: (...args: unknown[]) => browser.openWebTarget(...args),
}));

vi.mock('@/components/ui/popover', async () => {
  const React = await import('react');
  const PopoverContext = React.createContext<{ open: boolean; setOpen: (open: boolean) => void } | null>(null);
  return {
    Popover: ({ children }: { children: React.ReactNode }) => {
      const [open, setOpen] = React.useState(false);
      return <PopoverContext.Provider value={{ open, setOpen }}>{children}</PopoverContext.Provider>;
    },
    PopoverTrigger: ({ children, asChild }: { children: React.ReactNode; asChild?: boolean }) => {
      const ctx = React.useContext(PopoverContext);
      const onClick = () => ctx?.setOpen(!ctx.open);
      if (asChild && React.isValidElement(children)) {
        const child = children as React.ReactElement<{ onClick?: (event: React.MouseEvent) => void }>;
        return React.cloneElement(child, {
          onClick: (event: React.MouseEvent) => {
            child.props.onClick?.(event);
            onClick();
          },
        });
      }
      return <button type="button" onClick={onClick}>{children}</button>;
    },
    PopoverContent: ({ children }: { children: React.ReactNode }) => {
      const ctx = React.useContext(PopoverContext);
      return ctx?.open ? <div data-agent-diagnostic-help-content="true">{children}</div> : null;
    },
    PopoverAnchor: ({ children }: { children?: React.ReactNode }) => <>{children}</>,
    PopoverHeader: ({ children }: { children?: React.ReactNode }) => <div>{children}</div>,
    PopoverTitle: ({ children }: { children?: React.ReactNode }) => <div>{children}</div>,
    PopoverDescription: ({ children }: { children?: React.ReactNode }) => <p>{children}</p>,
  };
});

vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);

import i18n from '../src/i18n';
import { AgentManagementPage } from '../src/pages/AgentManagementPage';
import {
  RightWorkspaceProvider,
  createDraftConversationWorkspaceScope,
} from '../src/components/workspace/right-workspace-context';
import { mockAgentRegistry } from '../src/mockData';
import type { AgentRegistryVm } from '../src/types';

const ACP_REGISTRY_URL = 'https://agentclientprotocol.com/get-started/registry';
const agentManagementSource = readFileSync(
  path.resolve(process.cwd(), 'web/src/pages/AgentManagementPage.tsx'),
  'utf8',
);
const workspaceShellSource = readFileSync(
  path.resolve(process.cwd(), 'web/src/components/workspace/WorkspaceShell.tsx'),
  'utf8',
);

let cleanup: (() => void) | undefined;

afterEach(() => {
  cleanup?.();
  cleanup = undefined;
  vi.clearAllMocks();
});

beforeEach(async () => {
  await i18n.changeLanguage('zh-CN');
  for (const method of ['hasPointerCapture', 'setPointerCapture', 'releasePointerCapture', 'scrollIntoView']) {
    if (method in HTMLElement.prototype) continue;
    Object.defineProperty(HTMLElement.prototype, method, {
      configurable: true,
      value: method === 'hasPointerCapture' ? () => false : () => {},
    });
  }
});

function unhealthyRegistry(): AgentRegistryVm {
  const vm = structuredClone(mockAgentRegistry);
  vm.agents = [{
    ...vm.agents[0],
    diagnostic: {
      status: 'unhealthy',
      available: false,
      checkedAt: '2026-09-18T01:00:00.000Z',
      error: {
        code: 'acp.session-request-failed',
        params: { method: 'session/new' },
        raw: {
          code: -32000,
          message: 'Authentication required',
          data: { category: 'auth' },
        },
      },
    },
  }];
  return vm;
}

async function renderAgents(vm: AgentRegistryVm) {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  cleanup = () => {
    act(() => root.unmount());
    container.remove();
  };
  await act(async () => root.render(
    <RightWorkspaceProvider scope={createDraftConversationWorkspaceScope('project-1')}>
      <AgentManagementPage vm={vm} loading={false} onRefresh={() => {}} onRegistryChange={() => {}} />
    </RightWorkspaceProvider>,
  ));
  return container;
}

describe('Agent registry help', () => {
  it('opens ACP Registry in the in-app browser from a visible hyperlink after clicking help', async () => {
    expect(agentManagementSource).not.toContain('@tauri-apps/plugin-opener');
    expect(agentManagementSource).toContain('openWebTarget');
    expect(workspaceShellSource).toContain("conversationPageHasDraftWorkspaceScope(props.active)");

    const container = await renderAgents(unhealthyRegistry());
    expect(container.querySelector(`a[href="${ACP_REGISTRY_URL}"]`)).toBeNull();

    const help = container.querySelector<HTMLButtonElement>(`button[aria-label="${i18n.t('agentManagement.diagnosticHelpLabel')}"]`);
    expect(help).not.toBeNull();
    await act(async () => help?.click());

    const link = document.querySelector<HTMLAnchorElement>(`a[href="${ACP_REGISTRY_URL}"]`);
    expect(link).not.toBeNull();
    expect(link?.textContent).toBe('ACP Registry');
    expect(link?.className).toContain('text-link');
    expect(link?.className).toContain('underline');
    expect(link?.className).not.toContain('text-primary');

    await act(async () => link?.click());

    expect(browser.openWebTarget).toHaveBeenCalledWith(
      ACP_REGISTRY_URL,
      expect.objectContaining({
        projectId: 'project-1',
        scopeKey: 'draft:project-1',
      }),
    );
  });

  it('keeps the page banner generic and shows ACP raw reason only after clicking help', async () => {
    const container = await renderAgents(unhealthyRegistry());
    expect(container.textContent).not.toContain('Authentication required');
    expect(container.textContent).not.toContain(i18n.t('errors.acp.session-request-failed'));

    const help = container.querySelector<HTMLButtonElement>(`button[aria-label="${i18n.t('agentManagement.diagnosticHelpLabel')}"]`);
    await act(async () => help?.click());

    expect(document.body.textContent).toContain('Authentication required');
    expect(document.body.textContent).not.toContain(i18n.t('errors.acp.session-request-failed'));
  });
});
