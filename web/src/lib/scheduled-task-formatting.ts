import type { TFunction } from 'i18next';
import type { ScheduledScheduleInput, ScheduledScheduleSpec } from '@/types';

const twoDigits = (value: number) => String(value).padStart(2, '0');

export function scheduledScheduleTimezone(schedule: ScheduledScheduleSpec) {
  return schedule.timezone;
}

export function formatScheduledSchedule(t: TFunction, schedule: ScheduledScheduleSpec) {
  switch (schedule.kind) {
    case 'At':
      return t('scheduled.schedule.at', {
        time: new Intl.DateTimeFormat(undefined, {
          dateStyle: 'short',
          timeStyle: 'short',
          timeZone: schedule.timezone,
        }).format(new Date(schedule.at)),
      });
    case 'Every':
      return t('scheduled.schedule.every', {
        count: schedule.every.value,
        unit: t(`scheduled.units.${schedule.every.unit}`),
      });
    case 'Cron':
      return t('scheduled.schedule.cron', { expression: schedule.expression });
    case 'Repeat': {
      const time = `${twoDigits(schedule.hour)}:${twoDigits(schedule.minute)}`;
      if (schedule.preset === 'Hourly') return t('scheduled.schedule.hourly');
      if (schedule.preset === 'Daily') return t('scheduled.schedule.daily', { time });
      if (schedule.preset === 'Weekdays') return t('scheduled.schedule.weekdays', { time });
      return t('scheduled.schedule.weekly', {
        weekdays: schedule.preset.Weekly.weekdays
          .map((day) => t(`scheduled.weekdays.${day}`))
          .join(t('scheduled.schedule.weekdaySeparator')),
        time,
      });
    }
  }
}

export function formatScheduledScheduleInput(t: TFunction, schedule: ScheduledScheduleInput) {
  switch (schedule.kind) {
    case 'At':
      return t('scheduled.schedule.at', {
        time: `${schedule.localDate} ${schedule.localTime}`,
      });
    case 'Every':
      return t('scheduled.schedule.every', {
        count: schedule.every.value,
        unit: t(`scheduled.units.${schedule.every.unit}`),
      });
    case 'Cron':
      return t('scheduled.schedule.cron', { expression: schedule.expression });
    case 'Repeat': {
      const time = `${twoDigits(schedule.hour)}:${twoDigits(schedule.minute)}`;
      if (schedule.preset === 'Hourly') return t('scheduled.schedule.hourly');
      if (schedule.preset === 'Daily') return t('scheduled.schedule.daily', { time });
      if (schedule.preset === 'Weekdays') return t('scheduled.schedule.weekdays', { time });
      return t('scheduled.schedule.weekly', {
        weekdays: schedule.preset.Weekly.weekdays
          .map((day) => t(`scheduled.weekdays.${day}`))
          .join(t('scheduled.schedule.weekdaySeparator')),
        time,
      });
    }
  }
}
