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

// lucide 图标桩成空组件，避免引入真实图标逻辑（弹窗不再用 FolderInput：本地目录已移出）。
vi.mock('lucide-react', () => ({
  Loader2: () => null,
}));

// 将 shadcn ui 桩成最小 DOM：Dialog 按 open 渲染、Button 落成真 <button>。
// Select 桩捕获 onValueChange，使 SelectItem 点击能驱动组件的 setWorkspaceChange（不引入真实 Radix）。
const selectHolder = vi.hoisted(() => ({ onValueChange: (_value: string) => {} }));
vi.mock('@/components/ui/dialog', () => ({
  Dialog: ({ open, children }: { open: boolean; children: ReactNode }) => (open ? children : null),
  DialogContent: ({ children }: { children: ReactNode }) => children,
  DialogHeader: ({ children }: { children: ReactNode }) => children,
  DialogTitle: ({ children }: { children: ReactNode }) => <h2>{children}</h2>,
  DialogFooter: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));
vi.mock('@/components/ui/select', () => ({
  Select: ({ children, onValueChange }: { children: ReactNode; onValueChange?: (v: string) => void }) => {
    selectHolder.onValueChange = onValueChange ?? (() => {});
    return <div>{children}</div>;
  },
  SelectTrigger: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SelectValue: ({ placeholder }: { placeholder?: string }) => <span>{placeholder ?? ''}</span>,
  SelectContent: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  SelectItem: ({ value, children }: { value: string; children: ReactNode }) => {
    // 捕获渲染时刻的 onValueChange（而非点击时刻）：弹窗有两个 Select（工作区 + provider），
    // provider Select 后渲染会覆盖 holder；深度优先渲染保证工作区 SelectItem 先拿到 setWorkspaceId。
    const onChange = selectHolder.onValueChange;
    return (
      <div
        data-value={value}
        role="option"
        onClick={() => onChange(value)}
      >
        {children}
      </div>
    );
  },
}));
vi.mock('@/components/ui/button', () => ({
  Button: (props: Record<string, unknown> & { children?: ReactNode }) => (
    <button {...(props as object)}>{props.children}</button>
  ),
}));

const mocks = vi.hoisted(() => ({
  addMulticaWorkspace: vi.fn(),
  listServerMulticaWorkspaces: vi.fn(),
}));

vi.mock('@/api', () => ({
  addMulticaWorkspace: mocks.addMulticaWorkspace,
  listServerMulticaWorkspaces: mocks.listServerMulticaWorkspaces,
}));

const { addMulticaWorkspace, listServerMulticaWorkspaces } = mocks;

import { MulticaAddWorkspaceDialog } from '@/components/conversation/MulticaAddWorkspaceDialog';

const DIALOG_KEY = 'conversation.sidebar.multica.dialog';
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
  it('fetches server workspaces on open and disables add until a workspace is chosen', async () => {
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
    // 绑定模型下沉后弹窗只收「远程工作空间 + provider」：未选工作空间 → 禁用（不再要求本地目录）。
    expect(addButton?.disabled).toBe(true);
  });

  it('calls addMulticaWorkspace with (id, name, provider) — no local path — once a workspace is chosen', async () => {
    // 绑定模型已下沉到任务级：添加工作空间只绑 provider，本地目录推迟到执行时 composer 下拉选。
    const onAdded = vi.fn();
    const onOpenChange = vi.fn();
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <MulticaAddWorkspaceDialog
          open
          onOpenChange={onOpenChange}
          boundWorkspaceIds={[]}
          onAdded={onAdded}
        />,
      );
    });
    await act(async () => { await Promise.resolve(); });

    // 通过 Select 桩点击 Alpha（ws-1）选中远程工作空间。
    const alphaItem = container.querySelector('[data-value="ws-1"]') as HTMLElement;
    expect(alphaItem).toBeTruthy();
    await act(async () => { alphaItem.click(); });

    const addButton = findButton(container, ADD_KEY);
    expect(addButton).toBeTruthy();
    expect(addButton?.disabled).toBe(false);

    await act(async () => { addButton!.click(); });
    await act(async () => { await Promise.resolve(); });

    // addMulticaWorkspace 三参签名（id, name, provider），无 localPath；默认 provider = claude-acp。
    expect(addMulticaWorkspace).toHaveBeenCalledTimes(1);
    expect(addMulticaWorkspace).toHaveBeenCalledWith('ws-1', 'Alpha', 'claude-acp');
    expect(onAdded).toHaveBeenCalledTimes(1);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
