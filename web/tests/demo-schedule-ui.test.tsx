/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { ScheduledTaskDialog } from '@/components/conversation/ScheduledTaskDialog';
import { ReadOnlyExperience } from '@/components/ReadOnlyExperience';
import type { ScheduledTaskConfig } from '@/types';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

it('allows local schedule configuration but prevents saving an existing demo task', async () => {
  const container = document.createElement('div');
  document.body.append(container);
  const root = createRoot(container);
  const onSave = vi.fn(async (_config: ScheduledTaskConfig, _content?: string) => {});
  const render = (saveDisabled: boolean) => <ReadOnlyExperience.Provider value={true}>
    <ScheduledTaskDialog open presentation="workspace" allowContinuous saveDisabled={saveDisabled} onOpenChange={() => {}} onSave={onSave}
      initialConfig={{ schedule: { kind: 'Repeat', preset: 'Daily', hour: 9, minute: 0, timezone: 'UTC' }, overlapPolicy: 'skip_when_running', sessionPolicy: 'new' }} />
  </ReadOnlyExperience.Provider>;
  const done = () => [...container.querySelectorAll('button')].find((button) => button.textContent === 'scheduled.dialog.done')!;
  try {
    await act(async () => root.render(render(true)));
    expect(done().disabled).toBe(true);
    await act(async () => done().click());
    expect(onSave).not.toHaveBeenCalled();
    await act(async () => root.render(render(false)));
    expect(done().disabled).toBe(false);
    await act(async () => done().click());
    expect(onSave).toHaveBeenCalledOnce();
    expect(onSave.mock.calls[0][0]).toMatchObject({ schedule: { kind: 'Repeat', timezone: 'UTC' } });
  } finally {
    await act(async () => root.unmount());
    container.remove();
  }
});
