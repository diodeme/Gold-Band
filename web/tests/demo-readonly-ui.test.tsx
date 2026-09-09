/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it, vi } from 'vitest';
import { ReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { ConversationSidebar } from '@/components/conversation/ConversationSidebar';
import { demoSidebar } from '../../marketing/demo/fixtures';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@/components/ui/scroll-area', () => ({ ScrollArea: ({ children }: { children: React.ReactNode }) => <div>{children}</div> }));
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const noop = () => {};

describe('readonly experience presentation boundary', () => {
  it('keeps desktop actions by default and removes them only within the readonly provider', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    const onSelect = vi.fn();
    const sidebar = <ConversationSidebar vm={demoSidebar('en')} active={{ kind: 'contexts' }} onSelect={onSelect}
      onNewConversation={noop} onSearch={noop} onPinTask={noop} onUnpinTask={noop}
      onRenameTask={noop} onDeleteTask={noop} onRetryBootstrap={noop} onRequestWorkspaceTasks={noop}
      onRequestPinnedTasks={noop} onRequestTaskRuns={noop} />;
    try {
      await act(async () => root.render(sidebar));
      expect(container.textContent).toContain('conversation.sidebar.newChat');
      expect(container.textContent).toContain('conversation.sidebar.agentManagement');
      expect(container.textContent).toContain('conversation.sidebar.more');
      await act(async () => root.render(<ReadOnlyExperience.Provider value={true}>{sidebar}</ReadOnlyExperience.Provider>));
      expect(container.textContent).toContain('conversation.sidebar.newChat');
      expect(container.textContent).toContain('conversation.sidebar.agentManagement');
      const taskActions = container.querySelectorAll('[data-demo-task-actions] button');
      expect(taskActions.length).toBeGreaterThan(0);
      expect([...taskActions].every((button) => (button as HTMLButtonElement).disabled)).toBe(true);
      expect(container.textContent).toContain('conversation.sidebar.more');
      const more = [...container.querySelectorAll('button')].find((button) => button.textContent === 'conversation.sidebar.more')!;
      await act(async () => more.click());
      for (const [label, kind] of [['conversation.sidebar.multicaTaskManagement', 'multica-tasks'], ['scheduled.management.title', 'scheduled-tasks']]) {
        const entry = [...container.querySelectorAll('button')].find((button) => button.textContent === label)!;
        expect(entry.disabled).toBe(false);
        await act(async () => entry.click());
        expect(onSelect).toHaveBeenCalledWith({ kind });
      }
      const contexts = [...container.querySelectorAll('button')].find((button) => button.textContent === 'conversation.sidebar.contextManagement')!;
      await act(async () => contexts.click());
      expect(onSelect).toHaveBeenCalledWith({ kind: 'contexts' });
      expect(container.textContent).toContain('conversation.sidebar.settings');
    } finally {
      await act(async () => root.unmount());
      container.remove();
    }
  });
});
