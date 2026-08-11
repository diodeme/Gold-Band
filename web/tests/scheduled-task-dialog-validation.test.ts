import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

import { buildScheduledScheduleInput } from '../src/lib/scheduled-task-authoring';

const baseInput = {
  tab: 'at' as const,
  atDate: '2026-11-01',
  atTime: '01:30',
  disambiguation: 'earlier' as const,
  frequency: 'daily' as const,
  selectedWeekdays: ['Mon'],
  repeatTime: '09:00',
  everyValue: '6',
  everyUnit: 'hours' as const,
  cron: '0 0 9 * * *',
  timezone: 'America/New_York',
  anchorAt: '2026-08-10T00:00:00Z',
};

describe('scheduled task dialog authoring contract', () => {
  it('submits one-time wall-clock input with explicit disambiguation', () => {
    expect(buildScheduledScheduleInput(baseInput)).toEqual({
      kind: 'At',
      localDate: '2026-11-01',
      localTime: '01:30',
      timezone: 'America/New_York',
      disambiguation: 'earlier',
    });
    expect(buildScheduledScheduleInput({ ...baseInput, disambiguation: 'later' })).toEqual({
      kind: 'At',
      localDate: '2026-11-01',
      localTime: '01:30',
      timezone: 'America/New_York',
      disambiguation: 'later',
    });
  });

  it('builds Cron and weekly authoring payloads without changing input', () => {
    expect(buildScheduledScheduleInput({ ...baseInput, tab: 'cron' })).toEqual({
      kind: 'Cron',
      expression: '0 0 9 * * *',
      timezone: 'America/New_York',
    });
    expect(buildScheduledScheduleInput({
      ...baseInput,
      tab: 'repeat',
      frequency: 'weekly',
      selectedWeekdays: ['Tue', 'Thu'],
    })).toEqual({
      kind: 'Repeat',
      preset: { Weekly: { weekdays: ['Tue', 'Thu'] } },
      hour: 9,
      minute: 0,
      timezone: 'America/New_York',
    });
  });

  it('builds Every from the original value without coercing invalid input to one', () => {
    expect(buildScheduledScheduleInput({
      ...baseInput,
      tab: 'repeat',
      frequency: 'every',
      everyValue: '15',
      everyUnit: 'minutes',
    })).toEqual({
      kind: 'Every',
      every: { value: 15, unit: 'minutes' },
      anchorAt: '2026-08-10T00:00:00Z',
      timezone: 'America/New_York',
    });
    expect(buildScheduledScheduleInput({
      ...baseInput,
      tab: 'repeat',
      frequency: 'every',
      everyValue: '0',
    })).toMatchObject({ every: { value: 0 } });
  });

  it('renders inline validation and DST overlap controls in the dialog', () => {
    const source = readFileSync(
      fileURLToPath(new URL('../src/components/conversation/ScheduledTaskDialog.tsx', import.meta.url)),
      'utf8',
    );

    expect(source).toContain("validationIssue === null");
    expect(source).toContain("atAnalysis.kind === 'ambiguous'");
    expect(source).toContain("scheduled.dialog.disambiguation.earlier");
    expect(source).toContain("scheduled.dialog.disambiguation.later");
    expect(source).toContain('aria-describedby={undefined}');
    expect(source).toContain('ScheduledTimePicker');
    expect(source).not.toContain('type="time"');
    expect(source).toContain('rememberScheduledTimezone(value)');
  });
});
