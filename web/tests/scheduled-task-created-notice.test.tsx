/** @vitest-environment jsdom */

import React, { act, useState } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ScheduledTaskCreatedNotice } from '@/components/conversation/ConversationComposer';
import { SCHEDULED_TASK_CREATED_NOTICE_DURATION_MS, useScheduledTaskCreatedNotice } from '@/lib/scheduled-task-created-notice';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

function NoticeLifecycleProbe() {
  const notice = useScheduledTaskCreatedNotice();
  const [route, setRoute] = useState<'scheduled-task-create' | 'conversation-home' | 'scheduled-tasks'>('scheduled-task-create');
  return (
    <div data-route={route}>
      {route === 'scheduled-task-create' ? (
        <button type="button" onClick={() => { notice.show(); setRoute('conversation-home'); }}>create</button>
      ) : null}
      {notice.visible ? (
        <ScheduledTaskCreatedNotice onOpenScheduledTasks={() => { notice.dismiss(); setRoute('scheduled-tasks'); }} />
      ) : null}
    </div>
  );
}

beforeEach(() => {
  vi.useFakeTimers();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
  vi.useRealTimers();
});

describe('scheduled task created notice lifecycle', () => {
  it('survives the creation-route exit and opens scheduled task management', async () => {
    await act(async () => root.render(<NoticeLifecycleProbe />));
    await act(async () => host.querySelector<HTMLButtonElement>('button')?.click());

    expect(host.firstElementChild?.getAttribute('data-route')).toBe('conversation-home');
    expect(host.querySelector('[role="status"]')).not.toBeNull();

    await act(async () => host.querySelector<HTMLAnchorElement>('a')?.click());

    expect(host.firstElementChild?.getAttribute('data-route')).toBe('scheduled-tasks');
    expect(host.querySelector('[role="status"]')).toBeNull();
  });

  it('automatically clears the root-owned notice after five seconds', async () => {
    await act(async () => root.render(<NoticeLifecycleProbe />));
    await act(async () => host.querySelector<HTMLButtonElement>('button')?.click());
    await act(async () => vi.advanceTimersByTime(SCHEDULED_TASK_CREATED_NOTICE_DURATION_MS - 1));
    expect(host.querySelector('[role="status"]')).not.toBeNull();

    await act(async () => vi.advanceTimersByTime(1));
    expect(host.querySelector('[role="status"]')).toBeNull();
  });
});
