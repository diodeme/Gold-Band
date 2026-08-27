/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  deleteHistory: vi.fn(),
  getDiagnostics: vi.fn(),
  listHistory: vi.fn(),
  listTasks: vi.fn(),
  occurrenceListener: null as null | ((event: { projectId: string; scheduledTaskId: string }) => void),
}));
const translation = vi.hoisted(() => ({ t: (key: string) => key }));

vi.mock('react-i18next', async (importOriginal) => ({
  ...(await importOriginal<typeof import('react-i18next')>()),
  useTranslation: () => translation,
}));

vi.mock('@/api', () => ({
  deleteScheduledExecutionHistory: api.deleteHistory,
  deleteScheduledTask: vi.fn(),
  getScheduledTask: vi.fn(),
  getScheduledTaskDiagnostics: api.getDiagnostics,
  listScheduledExecutionHistory: api.listHistory,
  listScheduledTasks: api.listTasks,
  runScheduledTaskNow: vi.fn(),
  setScheduledTaskEnabled: vi.fn(),
  subscribeScheduledOccurrenceUpdates: vi.fn(async (listener) => {
    api.occurrenceListener = listener;
    return () => undefined;
  }),
  subscribeScheduledTaskUpdates: vi.fn(async () => () => undefined),
  updateScheduledTask: vi.fn(),
}));

vi.mock('@/components/conversation/ScheduledTaskDialog', () => ({
  ScheduledTaskDialog: () => null,
}));

import { TooltipProvider } from '@/components/ui/tooltip';
import { ScheduledTaskDetailPage } from '@/pages/ScheduledTaskDetailPage';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const historyItem = {
  projectId: 'project-1',
  scheduledTaskId: 'scheduled-1',
  taskId: 'task-1',
  runId: 'run-1',
  firstAcceptedAt: '2026-08-27T00:00:00Z',
  lastAcceptedAt: '2026-08-27T00:00:00Z',
  occurrenceCount: 1,
  latestOccurrenceId: 'occurrence-1',
  latestSummary: 'immutable summary',
  latestContentFingerprint: 'fingerprint-1',
  availability: 'available' as const,
  run: { runId: 'run-1', status: 'completed', outcome: 'succeeded', startedAt: '2026-08-27T00:00:00Z', updatedAt: '2026-08-27T00:00:01Z', resumable: false },
  error: null,
};

const task = {
  id: 'scheduled-1', projectId: 'project-1', workspaceName: 'Project 1', title: 'Task', mode: 'direct', sessionPolicy: 'new',
  enabled: true, status: 'enabled', schedule: { kind: 'At', at: '2099-01-01T00:00:00Z', timezone: 'UTC' }, nextAt: null,
  lastTriggerStatus: null, updatedAt: '2026-08-27T00:00:00Z',
};

let host: HTMLDivElement;
let root: Root;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => { resolve = next; });
  return { promise, resolve };
}

async function renderDetail(props: Partial<React.ComponentProps<typeof ScheduledTaskDetailPage>> = {}) {
  await act(async () => root.render(
    <TooltipProvider>
      <ScheduledTaskDetailPage projectId="project-1" scheduledTaskId="scheduled-1" onBack={() => undefined} {...props} />
    </TooltipProvider>,
  ));
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  api.deleteHistory.mockReset();
  api.getDiagnostics.mockReset().mockResolvedValue(null);
  api.listHistory.mockReset();
  api.listTasks.mockReset();
  api.occurrenceListener = null;
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
  vi.clearAllMocks();
});

describe('ScheduledTaskDetailPage history lifecycle', () => {
  it('shows completed history without waiting for the definition request', async () => {
    const definition = deferred<typeof task[]>();
    api.listTasks.mockReturnValue(definition.promise);
    api.listHistory
      .mockResolvedValueOnce({ items: [historyItem], nextCursor: 'cursor-2' })
      .mockResolvedValueOnce({ items: [], nextCursor: null });

    await renderDetail();
    await act(async () => undefined);

    expect(host.textContent).toContain('immutable summary');
    expect(host.textContent).toContain('scheduled.detail.loading');
    const nextButton = Array.from(host.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent?.includes('scheduled.detail.nextPage'))!;
    await act(async () => nextButton.click());
    definition.resolve([task]);
    await act(async () => undefined);
    expect(host.querySelector('h1')?.textContent).toBe('Task');
  });

  it('keeps deleted-definition history read-only', async () => {
    api.listTasks.mockResolvedValue([]);
    api.listHistory.mockResolvedValue({ items: [historyItem], nextCursor: null });

    await renderDetail();
    await act(async () => undefined);

    expect(host.textContent).toContain('scheduled.detail.deleted');
    expect(host.querySelector('[role="checkbox"]')).toBeNull();
    expect(host.textContent).not.toContain('scheduled.detail.deleteSelected');
  });

  it('does not refresh an anchored Run when a newer occurrence event arrives', async () => {
    api.listTasks.mockResolvedValue([task]);
    api.listHistory.mockResolvedValue({ items: [historyItem], nextCursor: null });

    await renderDetail({ taskId: 'task-1', runId: 'run-1', occurrenceId: 'occurrence-1' });
    await act(async () => undefined);
    expect(api.listHistory).toHaveBeenCalledTimes(1);

    await act(async () => api.occurrenceListener?.({ projectId: 'project-1', scheduledTaskId: 'scheduled-1' }));
    expect(api.listHistory).toHaveBeenCalledTimes(1);
  });

  it('clears selection and locks deletion while a page request is in flight', async () => {
    const nextPage = deferred<{ items: Array<typeof historyItem>; nextCursor: null }>();
    api.listTasks.mockResolvedValue([task]);
    api.listHistory
      .mockResolvedValueOnce({ items: [historyItem], nextCursor: 'cursor-2' })
      .mockReturnValueOnce(nextPage.promise);

    await renderDetail();
    await act(async () => undefined);
    const runCheckbox = host.querySelector<HTMLButtonElement>('[role="checkbox"][aria-label="scheduled.detail.selectRun"]');
    await act(async () => runCheckbox?.click());
    const deleteButton = Array.from(host.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent?.includes('scheduled.detail.deleteSelected'))!;
    expect(deleteButton.disabled).toBe(false);

    const nextButton = Array.from(host.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent?.includes('scheduled.detail.nextPage'))!;
    await act(async () => nextButton.click());
    expect(deleteButton.disabled).toBe(true);
    expect(runCheckbox?.getAttribute('data-state')).toBe('unchecked');
    expect(runCheckbox?.disabled).toBe(true);

    nextPage.resolve({ items: [], nextCursor: null });
    await act(async () => undefined);
  });

  it('coalesces duplicate history deletion clicks into one request', async () => {
    const deletion = deferred<Array<{ projectId: string; scheduledTaskId: string; taskId: string; runId: string; operationId: null; status: 'completed'; code: null; params: {} }>>();
    api.listTasks.mockResolvedValue([task]);
    api.listHistory.mockResolvedValue({ items: [historyItem], nextCursor: null });
    api.deleteHistory.mockReturnValue(deletion.promise);

    await renderDetail();
    await act(async () => undefined);
    await act(async () => host.querySelector<HTMLButtonElement>('[role="checkbox"][aria-label="scheduled.detail.selectRun"]')?.click());
    const deleteButton = Array.from(host.querySelectorAll<HTMLButtonElement>('button')).find((button) => button.textContent?.includes('scheduled.detail.deleteSelected'))!;
    await act(async () => { deleteButton.click(); deleteButton.click(); });
    expect(api.deleteHistory).toHaveBeenCalledTimes(1);

    deletion.resolve([{ projectId: 'project-1', scheduledTaskId: 'scheduled-1', taskId: 'task-1', runId: 'run-1', operationId: null, status: 'completed', code: null, params: {} }]);
    await act(async () => undefined);
  });
});
