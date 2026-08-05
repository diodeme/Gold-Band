/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// `t` 必须跨渲染稳定：被测组件的 useEffect 依赖 [open, t]，若每次渲染返回新的 t，
// 会触发 effect 反复重跑（setLoading 真假翻转）→ 无限渲染循环、用例超时。
const stableMocks = vi.hoisted(() => ({ t: (key: string) => key }));
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: stableMocks.t }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}));

vi.mock('@/i18n', () => ({
  displayAppError: () => 'mock-error',
}));

// lucide 图标桩成空组件，避免引入真实图标逻辑
vi.mock('lucide-react', () => ({
  FolderInput: () => null,
  Loader2: () => null,
}));

// 将 shadcn ui 桩成最小 DOM：Dialog 按 open 渲染、Select 透传、Button 落成真 <button>
vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children: ReactNode }) => (open ? children : null),
  DialogContent: ({ children }: { children: ReactNode }) => children,
  DialogHeader: ({ children }: { children: ReactNode }) => children,
  DialogTitle: ({ children }: { children: ReactNode }) => <h2>{children}</h2>,
  DialogFooter: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));
vi.mock('@/components/ui/select', () => ({
  Select: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SelectTrigger: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SelectValue: ({ placeholder }: { placeholder?: string }) => <span>{placeholder ?? ''}</span>,
  SelectContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SelectItem: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));
vi.mock('@/components/ui/button', () => ({
  Button: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));

const mocks = vi.hoisted(() => ({
  addMulticaWorkspace: vi.fn(),
  listServerMulticaWorkspaces: vi.fn(),
  pickLocalDirectory: vi.fn(),
}));

vi.mock('@/api', () => ({
  addMulticaWorkspace: mocks.addMulticaWorkspace,
  listServerMulticaWorkspaces: mocks.listServerMulticaWorkspaces,
  pickLocalDirectory: mocks.pickLocalDirectory,
}));

const { addMulticaWorkspace, listServerMulticaWorkspaces, pickLocalDirectory } = mocks;

import { MulticaAddWorkspaceDialog } from '@/components/conversation/MulticaAddWorkspaceDialog';

const DIALOG_KEY = 'conversation.sidebar.multica.dialog';
const BIND_KEY = `${DIALOG_KEY}.bindDirectory`;
const ADD_KEY = `${DIALOG_KEY}.add`;

function findButton(container: HTMLElement, textKey: string): HTMLButtonElement | undefined {
  return [...container.querySelectorAll('button')].find((button) => button.textContent === textKey);
}

beforeEach(() => {
  vi.clearAllMocks();
  listServerMulticaWorkspaces.mockResolvedValue([
    { id: 'ws-1', name: 'Alpha', slug: 'alpha' },
    { id: 'ws-2', name: 'Beta', slug: 'beta' },
  ]);
  addMulticaWorkspace.mockResolvedValue({ workspaces: [] });
});

afterEach(() => {
  document.body.innerHTML = '';
});

describe('multica add workspace dialog', () => {
  it('fetches server workspaces on open and disables add until workspace + directory chosen', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <MulticaAddWorkspaceDialog
          open
          onOpenChange={() => {}}
          boundWorkspaceIds={[]}
          onAdded={() => {}}
        />,
      );
    });
    await act(async () => { await Promise.resolve(); });

    expect(listServerMulticaWorkspaces).toHaveBeenCalledTimes(1);
    expect(container.textContent).toContain(`${DIALOG_KEY}.title`);

    const addButton = findButton(container, ADD_KEY);
    expect(addButton).toBeTruthy();
    // 初始：未选工作空间 + 未选目录 → 禁用
    expect(addButton?.disabled).toBe(true);
  });

  it('binds a local directory via pickLocalDirectory and still requires a workspace', async () => {
    pickLocalDirectory.mockResolvedValue('D:/projects/demo');

    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <MulticaAddWorkspaceDialog
          open
          onOpenChange={() => {}}
          boundWorkspaceIds={[]}
          onAdded={() => {}}
        />,
      );
    });
    await act(async () => { await Promise.resolve(); });

    const bindButton = findButton(container, BIND_KEY);
    expect(bindButton).toBeTruthy();

    await act(async () => {
      bindButton?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    });
    await act(async () => { await Promise.resolve(); });

    expect(pickLocalDirectory).toHaveBeenCalledTimes(1);
    // 选定目录后回显路径
    expect(container.textContent).toContain('D:/projects/demo');
    // 目录按钮文案切换为“更改目录”
    expect(findButton(container, `${DIALOG_KEY}.changeDirectory`)).toBeTruthy();
    // 仍未选工作空间 → 添加仍禁用
    expect(findButton(container, ADD_KEY)?.disabled).toBe(true);
  });
});
