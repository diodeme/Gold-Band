import { Temporal } from '@js-temporal/polyfill';
import { CronExpressionParser } from 'cron-parser';

import type {
  ScheduledAtDisambiguation,
  ScheduledEveryUnit,
  ScheduledScheduleInput,
  ScheduledScheduleSpec,
} from '@/types';

export type ScheduledAuthoringValidationIssue =
  | 'empty-weekdays'
  | 'invalid-cron'
  | 'invalid-every-value';

export type ScheduledLocalTimeAnalysis =
  | { kind: 'invalid' }
  | { kind: 'nonexistent' }
  | { kind: 'valid'; earlierInstant: string; earlierOffset: string }
  | {
      kind: 'ambiguous';
      earlierInstant: string;
      laterInstant: string;
      earlierOffset: string;
      laterOffset: string;
    };

export type ScheduledAuthoringTab = 'at' | 'repeat' | 'cron';
export type ScheduledRepeatFrequency = 'hourly' | 'daily' | 'weekdays' | 'weekly' | 'every';

export interface BuildScheduledScheduleInputOptions {
  tab: ScheduledAuthoringTab;
  atDate: string;
  atTime: string;
  disambiguation: ScheduledAtDisambiguation;
  frequency: ScheduledRepeatFrequency;
  selectedWeekdays: string[];
  repeatTime: string;
  everyValue: string;
  everyUnit: ScheduledEveryUnit;
  cron: string;
  timezone: string;
  anchorAt: string;
}

export function buildScheduledScheduleInput(
  options: BuildScheduledScheduleInputOptions,
): ScheduledScheduleInput {
  if (options.tab === 'at') {
    return {
      kind: 'At',
      localDate: options.atDate,
      localTime: options.atTime,
      timezone: options.timezone,
      disambiguation: options.disambiguation,
    };
  }
  if (options.tab === 'cron') {
    return {
      kind: 'Cron',
      expression: options.cron,
      timezone: options.timezone,
    };
  }
  if (options.frequency === 'every') {
    return {
      kind: 'Every',
      every: { value: Number(options.everyValue), unit: options.everyUnit },
      anchorAt: options.anchorAt,
      timezone: options.timezone,
    };
  }

  const [hour, minute] = options.repeatTime.split(':').map(Number);
  return {
    kind: 'Repeat',
    preset: options.frequency === 'weekly'
      ? { Weekly: { weekdays: options.selectedWeekdays } }
      : options.frequency === 'hourly'
        ? 'Hourly'
        : options.frequency === 'weekdays'
          ? 'Weekdays'
          : 'Daily',
    hour: options.frequency === 'hourly' ? 0 : hour,
    minute: options.frequency === 'hourly' ? 0 : minute,
    timezone: options.timezone,
  };
}

export function analyzeScheduledLocalTime(
  localDate: string,
  localTime: string,
  timezone: string,
): ScheduledLocalTimeAnalysis {
  try {
    const local = Temporal.PlainDateTime.from(`${localDate}T${localTime}`);
    const fields = {
      timeZone: timezone,
      year: local.year,
      month: local.month,
      day: local.day,
      hour: local.hour,
      minute: local.minute,
      second: local.second,
      millisecond: local.millisecond,
      microsecond: local.microsecond,
      nanosecond: local.nanosecond,
    };
    const earlier = Temporal.ZonedDateTime.from(fields, { disambiguation: 'earlier' });
    const later = Temporal.ZonedDateTime.from(fields, { disambiguation: 'later' });

    if (!earlier.toPlainDateTime().equals(local) || !later.toPlainDateTime().equals(local)) {
      return { kind: 'nonexistent' };
    }

    const earlierInstant = earlier.toInstant().toString();
    const laterInstant = later.toInstant().toString();
    if (earlierInstant === laterInstant) {
      return {
        kind: 'valid',
        earlierInstant,
        earlierOffset: earlier.offset,
      };
    }

    return {
      kind: 'ambiguous',
      earlierInstant,
      laterInstant,
      earlierOffset: earlier.offset,
      laterOffset: later.offset,
    };
  } catch {
    return { kind: 'invalid' };
  }
}

export function scheduledScheduleSpecFromInput(
  schedule: ScheduledScheduleInput,
): ScheduledScheduleSpec {
  if (schedule.kind === 'At') {
    const analysis = analyzeScheduledLocalTime(
      schedule.localDate,
      schedule.localTime,
      schedule.timezone,
    );
    if (analysis.kind === 'invalid' || analysis.kind === 'nonexistent') {
      throw new Error('invalid-scheduled-local-time');
    }
    const at = analysis.kind === 'ambiguous' && schedule.disambiguation === 'later'
      ? analysis.laterInstant
      : analysis.earlierInstant;
    return { kind: 'At', at, timezone: schedule.timezone };
  }
  if (schedule.kind === 'Every') {
    return {
      kind: 'Every',
      every: { ...schedule.every },
      anchorAt: schedule.anchorAt,
      timezone: schedule.timezone,
    };
  }
  if (schedule.kind === 'Cron') {
    return { ...schedule };
  }
  return {
    kind: 'Repeat',
    preset: typeof schedule.preset === 'string'
      ? schedule.preset
      : { Weekly: { weekdays: [...schedule.preset.Weekly.weekdays] } },
    hour: schedule.hour,
    minute: schedule.minute,
    timezone: schedule.timezone,
  };
}

export function validateScheduledCron(expression: string): ScheduledAuthoringValidationIssue | null {
  try {
    CronExpressionParser.parse(expression, { strict: true });
    return null;
  } catch {
    return 'invalid-cron';
  }
}

export function validateScheduledWeeklyDays(weekdays: readonly string[]): ScheduledAuthoringValidationIssue | null {
  return weekdays.length > 0 ? null : 'empty-weekdays';
}

export function validateScheduledEvery(value: string): ScheduledAuthoringValidationIssue | null {
  if (!/^\d+$/.test(value)) return 'invalid-every-value';
  const parsed = Number(value);
  return Number.isSafeInteger(parsed) && parsed > 0 ? null : 'invalid-every-value';
}
