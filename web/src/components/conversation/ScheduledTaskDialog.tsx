import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlarmClock, CalendarClock, Info } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { TimezoneCombobox } from '@/components/scheduled-tasks/TimezoneCombobox';
import { ScheduledTimePicker } from '@/components/scheduled-tasks/ScheduledTimePicker';
import {
  analyzeScheduledLocalTime,
  buildScheduledScheduleInput,
  validateScheduledCron,
  validateScheduledEvery,
  validateScheduledWeeklyDays,
  type ScheduledAuthoringTab,
  type ScheduledRepeatFrequency,
} from '@/lib/scheduled-task-authoring';
import { getScheduledTimezones } from '@/lib/scheduled-task-timezones';
import { getPreferredScheduledTimezone, rememberScheduledTimezone } from '@/lib/scheduled-task-timezone-preference';
import type {
  ScheduledAtDisambiguation,
  ScheduledEveryUnit,
  ScheduledOverlapPolicy,
  ScheduledScheduleInput,
  ScheduledScheduleSpec,
  ScheduledSessionPolicy,
} from '@/types';

export type ScheduledTaskConfig = {
  schedule: ScheduledScheduleInput;
  overlapPolicy: ScheduledOverlapPolicy;
  sessionPolicy: ScheduledSessionPolicy;
};

export type ScheduledTaskInitialConfig = {
  schedule: ScheduledScheduleSpec;
  overlapPolicy: ScheduledOverlapPolicy;
  sessionPolicy: ScheduledSessionPolicy;
};

type ScheduledTaskDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSave: (config: ScheduledTaskConfig, content?: string) => Promise<void>;
  allowContinuous: boolean;
  initialConfig?: ScheduledTaskInitialConfig | null;
  draftConfig?: ScheduledTaskConfig | null;
  initialContent?: string;
  showContent?: boolean;
  presentation?: 'dialog' | 'workspace';
};

type ValidationField = 'atDate' | 'atTime' | 'at' | 'repeatTime' | 'weekdays' | 'every' | 'cron' | 'timezone';
type ValidationReason =
  | 'invalidDate'
  | 'invalidTime'
  | 'invalidTimezone'
  | 'nonexistentLocalTime'
  | 'emptyWeekdays'
  | 'invalidEvery'
  | 'invalidCron';

type ValidationIssue = {
  field: ValidationField;
  reason: ValidationReason;
};

const weekdays = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'] as const;
const defaultSelectedWeekdays = ['Mon', 'Wed', 'Fri'];

function localDateValue() {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
}

function localFieldsForInstant(instant: string, timezone: string) {
  const parts = new Intl.DateTimeFormat('en-CA', {
    timeZone: timezone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(new Date(instant));
  const values = Object.fromEntries(parts.map((part) => [part.type, part.value]));
  return {
    localDate: `${values.year}-${values.month}-${values.day}`,
    localTime: `${values.hour}:${values.minute}`,
  };
}

export function ScheduledTaskDialog({
  open,
  onOpenChange,
  onSave,
  allowContinuous,
  initialConfig,
  draftConfig,
  initialContent,
  showContent = false,
  presentation = 'dialog',
}: ScheduledTaskDialogProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<ScheduledAuthoringTab>('repeat');
  const [atDate, setAtDate] = useState(localDateValue);
  const [atTime, setAtTime] = useState('09:00');
  const [atDisambiguation, setAtDisambiguation] = useState<ScheduledAtDisambiguation>('earlier');
  const [frequency, setFrequency] = useState<ScheduledRepeatFrequency>('daily');
  const [selectedWeekdays, setSelectedWeekdays] = useState<string[]>(defaultSelectedWeekdays);
  const [repeatTime, setRepeatTime] = useState('09:00');
  const [everyValue, setEveryValue] = useState('6');
  const [everyUnit, setEveryUnit] = useState<ScheduledEveryUnit>('hours');
  const [cron, setCron] = useState('0 0 9 * * *');
  const [timezone, setTimezone] = useState(getPreferredScheduledTimezone);
  const [queueProtection, setQueueProtection] = useState(true);
  const [sessionPolicy, setSessionPolicy] = useState<ScheduledSessionPolicy>('new');
  const [saving, setSaving] = useState(false);
  const [content, setContent] = useState('');

  const applyAuthoringSchedule = (schedule: ScheduledScheduleInput) => {
    setTimezone(schedule.timezone);
    if (schedule.kind === 'At') {
      setTab('at');
      setAtDate(schedule.localDate);
      setAtTime(schedule.localTime);
      setAtDisambiguation(schedule.disambiguation);
    } else if (schedule.kind === 'Every') {
      setTab('repeat');
      setFrequency('every');
      setEveryValue(String(schedule.every.value));
      setEveryUnit(schedule.every.unit);
    } else if (schedule.kind === 'Cron') {
      setTab('cron');
      setCron(schedule.expression);
    } else {
      setTab('repeat');
      setRepeatTime(`${String(schedule.hour).padStart(2, '0')}:${String(schedule.minute).padStart(2, '0')}`);
      if (schedule.preset === 'Hourly') setFrequency('hourly');
      else if (schedule.preset === 'Daily') setFrequency('daily');
      else if (schedule.preset === 'Weekdays') setFrequency('weekdays');
      else {
        setFrequency('weekly');
        setSelectedWeekdays(schedule.preset.Weekly.weekdays);
      }
    }
  };

  const applyPersistedSchedule = (schedule: ScheduledScheduleSpec) => {
    setTimezone(schedule.timezone);
    if (schedule.kind === 'At') {
      const fields = localFieldsForInstant(schedule.at, schedule.timezone);
      const analysis = analyzeScheduledLocalTime(fields.localDate, fields.localTime, schedule.timezone);
      setTab('at');
      setAtDate(fields.localDate);
      setAtTime(fields.localTime);
      setAtDisambiguation(
        analysis.kind === 'ambiguous' && Date.parse(analysis.laterInstant) === Date.parse(schedule.at)
          ? 'later'
          : 'earlier',
      );
    } else if (schedule.kind === 'Every') {
      setTab('repeat');
      setFrequency('every');
      setEveryValue(String(schedule.every.value));
      setEveryUnit(schedule.every.unit);
    } else if (schedule.kind === 'Cron') {
      setTab('cron');
      setCron(schedule.expression);
    } else {
      setTab('repeat');
      setRepeatTime(`${String(schedule.hour).padStart(2, '0')}:${String(schedule.minute).padStart(2, '0')}`);
      if (schedule.preset === 'Hourly') setFrequency('hourly');
      else if (schedule.preset === 'Daily') setFrequency('daily');
      else if (schedule.preset === 'Weekdays') setFrequency('weekdays');
      else {
        setFrequency('weekly');
        setSelectedWeekdays(schedule.preset.Weekly.weekdays);
      }
    }
  };

  const resetNewTask = () => {
    setTab('repeat');
    setAtDate(localDateValue());
    setAtTime('09:00');
    setAtDisambiguation('earlier');
    setFrequency('daily');
    setSelectedWeekdays(defaultSelectedWeekdays);
    setRepeatTime('09:00');
    setEveryValue('6');
    setEveryUnit('hours');
    setCron('0 0 9 * * *');
    setTimezone(getPreferredScheduledTimezone());
    setQueueProtection(true);
    setSessionPolicy('new');
  };

  useEffect(() => {
    if (!open) return;
    setContent(initialContent ?? '');
    const config = initialConfig ?? draftConfig;
    if (!config) {
      resetNewTask();
      return;
    }
    setQueueProtection(config.overlapPolicy === 'skip_when_running');
    setSessionPolicy(config.sessionPolicy);
    if (initialConfig) applyPersistedSchedule(initialConfig.schedule);
    else if (draftConfig) applyAuthoringSchedule(draftConfig.schedule);
  }, [draftConfig, initialConfig, initialContent, open]);

  const timezoneValid = useMemo(() => getScheduledTimezones().includes(timezone), [timezone]);
  const atDateValid = useMemo(
    () => analyzeScheduledLocalTime(atDate, '00:00', 'UTC').kind !== 'invalid',
    [atDate],
  );
  const atTimeValid = useMemo(
    () => analyzeScheduledLocalTime('2000-01-01', atTime, 'UTC').kind !== 'invalid',
    [atTime],
  );
  const repeatTimeValid = useMemo(
    () => analyzeScheduledLocalTime('2000-01-01', repeatTime, 'UTC').kind !== 'invalid',
    [repeatTime],
  );
  const atAnalysis = useMemo(
    () => analyzeScheduledLocalTime(atDate, atTime, timezone),
    [atDate, atTime, timezone],
  );

  const validationIssue = useMemo<ValidationIssue | null>(() => {
    if (!timezoneValid) return { field: 'timezone', reason: 'invalidTimezone' };
    if (tab === 'at') {
      if (!atDateValid) return { field: 'atDate', reason: 'invalidDate' };
      if (!atTimeValid) return { field: 'atTime', reason: 'invalidTime' };
      if (atAnalysis.kind === 'nonexistent') {
        return { field: 'at', reason: 'nonexistentLocalTime' };
      }
      if (atAnalysis.kind === 'invalid') return { field: 'atTime', reason: 'invalidTime' };
      return null;
    }
    if (tab === 'cron') {
      return validateScheduledCron(cron)
        ? { field: 'cron', reason: 'invalidCron' }
        : null;
    }
    if (frequency === 'weekly' && validateScheduledWeeklyDays(selectedWeekdays)) {
      return { field: 'weekdays', reason: 'emptyWeekdays' };
    }
    if (frequency === 'every' && validateScheduledEvery(everyValue)) {
      return { field: 'every', reason: 'invalidEvery' };
    }
    if (frequency !== 'every' && frequency !== 'hourly' && !repeatTimeValid) {
      return { field: 'repeatTime', reason: 'invalidTime' };
    }
    return null;
  }, [
    atAnalysis.kind,
    atDateValid,
    atTimeValid,
    cron,
    everyValue,
    frequency,
    repeatTimeValid,
    selectedWeekdays,
    tab,
    timezoneValid,
  ]);

  const repeatDescription = useMemo(() => {
    if (frequency === 'hourly') return t('scheduled.dialog.hourlyDescription');
    if (frequency === 'daily') return t('scheduled.schedule.daily', { time: repeatTime });
    if (frequency === 'weekdays') return t('scheduled.schedule.weekdays', { time: repeatTime });
    if (frequency === 'weekly') return t('scheduled.schedule.weekly', {
      weekdays: selectedWeekdays.map((day) => t(`scheduled.weekdays.${day}`)).join(t('scheduled.schedule.weekdaySeparator')),
      time: repeatTime,
    });
    return t('scheduled.schedule.every', { count: everyValue, unit: t(`scheduled.units.${everyUnit}`) });
  }, [everyUnit, everyValue, frequency, repeatTime, selectedWeekdays, t]);

  const canSave = validationIssue === null;
  const validationMessage = (field: ValidationField) => (
    validationIssue?.field === field
      ? t(`scheduled.dialog.validation.${validationIssue.reason}`)
      : null
  );

  const save = async () => {
    if (!canSave) return;
    const schedule = buildScheduledScheduleInput({
      tab,
      atDate,
      atTime,
      disambiguation: atDisambiguation,
      frequency,
      selectedWeekdays,
      repeatTime,
      everyValue,
      everyUnit,
      cron,
      timezone,
      anchorAt: new Date().toISOString(),
    });
    setSaving(true);
    try {
      await onSave({
        schedule,
        overlapPolicy: queueProtection ? 'skip_when_running' : 'retry_when_busy',
        sessionPolicy: allowContinuous ? sessionPolicy : 'new',
      }, showContent ? content : undefined);
      onOpenChange(false);
    } finally {
      setSaving(false);
    }
  };

  const editor = (
    <>
        {presentation === 'dialog' ? (
          <DialogHeader className="border-b border-border/60 px-5 py-4 text-left">
            <DialogTitle className="flex items-center gap-2 text-base">
              <AlarmClock className="size-4 text-foreground" />
              {t('scheduled.dialog.title')}
            </DialogTitle>
          </DialogHeader>
        ) : (
          <header className="border-b border-border/60 px-5 py-4">
            <h2 className="flex items-center gap-2 text-sm font-semibold">
              <AlarmClock className="size-4 text-foreground" />
              {t('scheduled.dialog.title')}
            </h2>
          </header>
        )}
        <div className="space-y-5 overflow-y-auto p-5">
          {showContent ? (
            <label className="block space-y-2 text-xs text-muted-foreground">
              {t('scheduled.dialog.content')}
              <Textarea value={content} onChange={(event) => setContent(event.target.value)} className="min-h-28 resize-y text-sm" />
            </label>
          ) : null}
          <Tabs value={tab} onValueChange={(value) => setTab(value as ScheduledAuthoringTab)}>
            <TabsList className="grid w-full grid-cols-3">
              <TabsTrigger value="at">{t('scheduled.dialog.tabs.at')}</TabsTrigger>
              <TabsTrigger value="repeat">{t('scheduled.dialog.tabs.repeat')}</TabsTrigger>
              <TabsTrigger value="cron">{t('scheduled.dialog.tabs.cron')}</TabsTrigger>
            </TabsList>
          </Tabs>

          {tab === 'at' ? (
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label className="space-y-2 text-xs text-muted-foreground">
                {t('scheduled.dialog.date')}
                <Input type="date" value={atDate} aria-invalid={validationIssue?.field === 'atDate'} onChange={(event) => setAtDate(event.target.value)} />
                {validationMessage('atDate') ? <span className="block text-destructive">{validationMessage('atDate')}</span> : null}
              </label>
              <label className="space-y-2 text-xs text-muted-foreground">
                {t('scheduled.dialog.time')}
                <ScheduledTimePicker value={atTime} invalid={validationIssue?.field === 'atTime' || validationIssue?.field === 'at'} onValueChange={setAtTime} />
                {validationMessage('atTime') ? <span className="block text-destructive">{validationMessage('atTime')}</span> : null}
              </label>
              {validationMessage('at') ? <p className="text-xs text-destructive sm:col-span-2">{validationMessage('at')}</p> : null}
              {atAnalysis.kind === 'ambiguous' ? (
                <div className="space-y-2 sm:col-span-2">
                  <span className="text-xs text-muted-foreground">{t('scheduled.dialog.disambiguation.label')}</span>
                  <div className="grid grid-cols-2 rounded-md bg-secondary p-1" role="group" aria-label={t('scheduled.dialog.disambiguation.label')}>
                    <Button type="button" variant={atDisambiguation === 'earlier' ? 'default' : 'ghost'} size="sm" className="h-8 text-xs" onClick={() => setAtDisambiguation('earlier')}>
                      {t('scheduled.dialog.disambiguation.earlier', { offset: atAnalysis.earlierOffset })}
                    </Button>
                    <Button type="button" variant={atDisambiguation === 'later' ? 'default' : 'ghost'} size="sm" className="h-8 text-xs" onClick={() => setAtDisambiguation('later')}>
                      {t('scheduled.dialog.disambiguation.later', { offset: atAnalysis.laterOffset })}
                    </Button>
                  </div>
                </div>
              ) : null}
            </div>
          ) : null}

          {tab === 'repeat' ? (
            <div className="space-y-4">
              <label className="block space-y-2 text-xs text-muted-foreground">
                {t('scheduled.dialog.frequency')}
                <Select value={frequency} onValueChange={(value) => setFrequency(value as ScheduledRepeatFrequency)}>
                  <SelectTrigger><SelectValue /></SelectTrigger>
                  <SelectContent>
                    {(['hourly', 'daily', 'weekdays', 'weekly', 'every'] as const).map((value) => (
                      <SelectItem key={value} value={value}>{t(`scheduled.dialog.frequencyOptions.${value}`)}</SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </label>
              {frequency === 'weekly' ? (
                <div className="space-y-2">
                  <span className="text-xs text-muted-foreground">{t('scheduled.dialog.weekdaySelect')}</span>
                  <div className="grid grid-cols-7 gap-1.5">
                    {weekdays.map((day) => (
                      <Button
                        key={day}
                        type="button"
                        variant={selectedWeekdays.includes(day) ? 'secondary' : 'outline'}
                        className="h-9 px-0 text-xs"
                        aria-pressed={selectedWeekdays.includes(day)}
                        onClick={() => setSelectedWeekdays((current) => current.includes(day) ? current.filter((item) => item !== day) : [...current, day])}
                      >
                        {t(`scheduled.weekdays.${day}`)}
                      </Button>
                    ))}
                  </div>
                  {validationMessage('weekdays') ? <p className="text-xs text-destructive">{validationMessage('weekdays')}</p> : null}
                </div>
              ) : null}
              {frequency !== 'every' && frequency !== 'hourly' ? (
                <label className="block space-y-2 text-xs text-muted-foreground">
                  {t('scheduled.dialog.executionTime')}
                  <ScheduledTimePicker value={repeatTime} invalid={validationIssue?.field === 'repeatTime'} onValueChange={setRepeatTime} />
                  {validationMessage('repeatTime') ? <span className="block text-destructive">{validationMessage('repeatTime')}</span> : null}
                </label>
              ) : null}
              {frequency === 'every' ? (
                <label className="block space-y-2 text-xs text-muted-foreground">
                  {t('scheduled.dialog.interval')}
                  <div className="grid grid-cols-2 gap-2">
                    <Input type="number" min="1" step="1" value={everyValue} aria-invalid={validationIssue?.field === 'every'} onChange={(event) => setEveryValue(event.target.value)} />
                    <Select value={everyUnit} onValueChange={(value) => setEveryUnit(value as ScheduledEveryUnit)}>
                      <SelectTrigger><SelectValue /></SelectTrigger>
                      <SelectContent>
                        <SelectItem value="minutes">{t('scheduled.units.minutes')}</SelectItem>
                        <SelectItem value="hours">{t('scheduled.units.hours')}</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  {validationMessage('every') ? <span className="block text-destructive">{validationMessage('every')}</span> : null}
                </label>
              ) : null}
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <CalendarClock className="size-3.5 text-foreground" />
                {t('scheduled.dialog.nextRun', { description: repeatDescription, timezone })}
              </div>
              <div className="flex items-start gap-2 text-xs text-muted-foreground">
                <Info className="mt-0.5 size-3.5 shrink-0" />
                <span>{t('scheduled.dialog.repeatHint', { frequency: t(`scheduled.dialog.frequencyOptions.${frequency}`) })}</span>
              </div>
            </div>
          ) : null}

          {tab === 'cron' ? (
            <div className="space-y-3">
              <label className="block space-y-2 text-xs text-muted-foreground">
                {t('scheduled.dialog.cronExpression')}
                <Input className="font-mono" value={cron} aria-invalid={validationIssue?.field === 'cron'} onChange={(event) => setCron(event.target.value)} />
                {validationMessage('cron') ? <span className="block text-destructive">{validationMessage('cron')}</span> : null}
              </label>
              <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <CalendarClock className="size-3.5 text-foreground" />
                {t('scheduled.dialog.cronHint')}
              </div>
            </div>
          ) : null}

          <label className="block space-y-2 text-xs text-muted-foreground">
            {t('scheduled.dialog.timezone')}
            <TimezoneCombobox value={timezone} onValueChange={(value) => { setTimezone(value); rememberScheduledTimezone(value); }} />
            {validationMessage('timezone') ? <span className="block text-destructive">{validationMessage('timezone')}</span> : null}
          </label>

          <div className="flex items-center justify-between gap-5 border-t border-border/60 pt-4">
            <div>
              <strong className="block text-xs font-medium text-foreground">{t('scheduled.dialog.queueProtection')}</strong>
              <span className="text-ui-caption text-muted-foreground">{t('scheduled.dialog.queueProtectionDescription')}</span>
            </div>
            <Switch checked={queueProtection} onCheckedChange={setQueueProtection} aria-label={t('scheduled.dialog.queueProtection')} />
          </div>
          {allowContinuous ? (
            <div className="flex items-center justify-between gap-5 border-t border-border/60 pt-4">
              <div>
                <strong className="block text-xs font-medium text-foreground">{t('scheduled.dialog.sessionPolicy')}</strong>
                <span className="text-ui-caption text-muted-foreground">{t('scheduled.dialog.directMode')}</span>
              </div>
              <div className="flex rounded-md bg-secondary p-1">
                <Button type="button" variant={sessionPolicy === 'new' ? 'default' : 'ghost'} size="sm" className="h-7 px-3 text-xs" onClick={() => setSessionPolicy('new')}>{t('scheduled.session.new')}</Button>
                <Button type="button" variant={sessionPolicy === 'continuous' ? 'default' : 'ghost'} size="sm" className="h-7 px-3 text-xs" onClick={() => setSessionPolicy('continuous')}>{t('scheduled.session.continuous')}</Button>
              </div>
            </div>
          ) : null}
        </div>
        <div className="flex justify-end gap-2 border-t border-border/60 px-5 py-4">
          <Button variant="outline" onClick={() => onOpenChange(false)}>{t('scheduled.dialog.cancel')}</Button>
          <Button disabled={!canSave || saving} onClick={() => void save()}>{t('scheduled.dialog.done')}</Button>
        </div>
    </>
  );

  if (presentation === 'workspace') {
    return (
      <section className="flex min-h-0 flex-1 flex-col overflow-hidden bg-background" data-scheduled-task-config-panel="true">
        {editor}
      </section>
    );
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        aria-describedby={undefined}
        className="max-h-[min(760px,calc(100vh-2rem))] max-w-[590px] gap-0 overflow-hidden p-0"
      >
        {editor}
      </DialogContent>
    </Dialog>
  );
}
