/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, saveConversationPreference: vi.fn().mockResolvedValue(undefined) };
});

vi.mock('@/components/ui/scroll-area', () => ({
  ScrollArea: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
}));

import { ConversationSidebar, RESIDENT_PIN_GRADIENT_STOPS } from '@/components/conversation/ConversationSidebar';
import type { ConversationSidebarVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const callbacks = {
  onSelect: () => {},
  onNewConversation: () => {},
  onSearch: () => {},
  onPinTask: () => {},
  onUnpinTask: () => {},
  onRenameTask: () => {},
  onDeleteTask: () => {},
  onRetryBootstrap: () => {},
  onRequestWorkspaceTasks: () => {},
  onRequestPinnedTasks: () => {},
  onRequestTaskRuns: () => {},
};

function sidebarVm(): ConversationSidebarVm {
  return {
    loadStatus: 'ready',
    workspaces: [
      { projectId: 'workspace-a', workspacePath: 'D:\\workspace-a', name: 'Workspace A' },
      { projectId: 'workspace-b', workspacePath: 'D:\\workspace-b', name: 'Workspace B' },
    ],
    pinRefs: [],
    pinnedTasks: [],
    pinnedTaskPage: { status: 'ready', nextCursor: null },
    tasksByWorkspace: { 'workspace-a': [], 'workspace-b': [] },
    workspaceTaskPages: {
      'workspace-a': { status: 'ready', nextCursor: null },
      'workspace-b': { status: 'ready', nextCursor: null },
    },
    lastActiveWorkspaceId: 'workspace-a',
  };
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('Direct residency context menu', () => {
  it.each(['direct', 'workflow', 'auto'] as const)('offers residency only for %s when appropriate', async (runMode) => {
    const container = document.createElement('div'); document.body.append(container);
    const root = createRoot(container); const onSetResident = vi.fn().mockResolvedValue(undefined);
    const vm = sidebarVm();
    vm.pinnedTasks = [{ projectId:'workspace-a', taskId:'resident-task', taskUuid:'uuid', title:'Resident test', autoTitle:false, runMode, latestRun:null, runs:[], runHistoryStatus:'ready', runsNextCursor:null, pinned:true, resident:false }];
    try {
      await act(async () => root.render(<ConversationSidebar {...callbacks} onSetResident={onSetResident} vm={vm} active={{kind:'conversation-home'}} />));
      const trigger = container.querySelector('[data-slot="context-menu-trigger"]');
      expect(Boolean(trigger)).toBe(runMode === 'direct');
      if (!trigger) return;
      await act(async () => trigger.dispatchEvent(new MouseEvent('contextmenu', {bubbles:true,button:2,clientX:20,clientY:20})));
      const item = document.querySelector<HTMLElement>('[role="menuitem"]')!;
      expect(item.textContent).toContain('conversation.sidebar.keepResident');
      await act(async () => item.click());
      expect(onSetResident).toHaveBeenCalledWith('workspace-a','resident-task',true);
      const updated = {...vm,pinnedTasks:[{...vm.pinnedTasks[0],resident:true}]};
      await act(async () => root.render(<ConversationSidebar {...callbacks} onSetResident={onSetResident} vm={updated} active={{kind:'conversation-home'}} />));
      expect(container.querySelector('[aria-label="conversation.sidebar.resident"]')).not.toBeNull();
      const pin = container.querySelector('[aria-label="conversation.sidebar.resident"] svg[data-resident-pin]')!;
      const gradient = pin.querySelector('linearGradient')!;
      expect(pin.querySelector('path')?.getAttribute('fill')).toBe(`url(#${gradient.id})`);
      // 包围盒单位会让零宽线段无法着色，必须使用图标坐标系。
      expect(gradient.getAttribute('gradientUnits')).toBe('userSpaceOnUse');
      expect(Array.from(gradient.querySelectorAll('stop'), (stop) => stop.style.stopColor)).toEqual(RESIDENT_PIN_GRADIENT_STOPS.map(({ color }) => color));
      // 色相必须来自主题 token，不能写死成与主题无关的固定色板。
      for (const token of ['--gold-emphasis', '--gold-running', '--accent-foreground']) {
        expect(RESIDENT_PIN_GRADIENT_STOPS.some(({ color }) => color.includes(`var(${token})`))).toBe(true);
      }
      expect(container.querySelector('[aria-label="conversation.sidebar.unpin"]')).not.toBeNull();
      await act(async () => container.querySelector('[data-slot="context-menu-trigger"]')!.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,button:2,clientX:20,clientY:20})));
      const release = document.querySelector<HTMLElement>('[role="menuitem"]')!;
      expect(release.textContent).toContain('conversation.sidebar.releaseResident');
      await act(async () => release.click());
      expect(onSetResident).toHaveBeenLastCalledWith('workspace-a','resident-task',false);
    } finally { await act(async () => root.unmount()); }
  });
});
