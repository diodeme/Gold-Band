import { useEffect, useMemo, useState } from 'react';
import { AlarmClock, CalendarClock, Info } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import type { ScheduledScheduleSpec } from '@/types';

export type ScheduledTaskConfig = {
  schedule: ScheduledScheduleSpec;
  overlapPolicy: 'skip_when_running' | 'retry_when_busy';
  sessionPolicy: 'new' | 'continuous';
};

type ScheduledTaskDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSave: (config: ScheduledTaskConfig, content?: string) => Promise<void>;
  allowContinuous: boolean;
  initialConfig?: ScheduledTaskConfig | null;
  initialContent?: string;
  showContent?: boolean;
};

type ScheduleTab = 'at' | 'repeat' | 'cron';
type RepeatFrequency = 'hourly' | 'daily' | 'weekdays' | 'weekly' | 'every';

const weekdays = [
  ['Mon', '一'], ['Tue', '二'], ['Wed', '三'], ['Thu', '四'],
  ['Fri', '五'], ['Sat', '六'], ['Sun', '日'],
] as const;

const timezoneOptions = [
  { value: 'Asia/Shanghai', label: '中国（上海）' },
  { value: 'Asia/Tokyo', label: '日本（东京）' },
  { value: 'Europe/London', label: '英国（伦敦）' },
  { value: 'America/New_York', label: '美国（纽约）' },
] as const;

function localDateValue() {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
}

function zonedDateTimeToUtcIso(date: string, time: string, timezone: string) {
  const [year, month, day] = date.split('-').map(Number);
  const [hour, minute] = time.split(':').map(Number);
  const guess = Date.UTC(year, month - 1, day, hour, minute, 0);
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: timezone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(new Date(guess));
  const values = Object.fromEntries(parts.map((part) => [part.type, part.value]));
  const represented = Date.UTC(
    Number(values.year),
    Number(values.month) - 1,
    Number(values.day),
    Number(values.hour),
    Number(values.minute),
    Number(values.second),
  );
  return new Date(guess - (represented - guess)).toISOString();
}

export function ScheduledTaskDialog({
  open,
  onOpenChange,
  onSave,
  allowContinuous,
  initialConfig,
  initialContent,
  showContent = false,
}: ScheduledTaskDialogProps) {
  const [tab, setTab] = useState<ScheduleTab>('repeat');
  const [atDate, setAtDate] = useState(localDateValue);
  const [atTime, setAtTime] = useState('09:00');
  const [frequency, setFrequency] = useState<RepeatFrequency>('daily');
  const [selectedWeekdays, setSelectedWeekdays] = useState<string[]>(['Mon', 'Wed', 'Fri']);
  const [repeatTime, setRepeatTime] = useState('09:00');
  const [everyValue, setEveryValue] = useState('6');
  const [everyUnit, setEveryUnit] = useState<'minutes' | 'hours'>('hours');
  const [cron, setCron] = useState('0 0 9 * * *');
  const [timezone, setTimezone] = useState('Asia/Shanghai');
  const [queueProtection, setQueueProtection] = useState(true);
  const [sessionPolicy, setSessionPolicy] = useState<'new' | 'continuous'>('new');
  const [saving, setSaving] = useState(false);
  const [content, setContent] = useState('');

  const syncInitialConfig = (config: ScheduledTaskConfig | null | undefined, initialContent: string | undefined) => {
    if (!config) {
      setContent(initialContent ?? '');
      return;
    }
    setQueueProtection(config.overlapPolicy === 'skip_when_running');
    setSessionPolicy(config.sessionPolicy);
    setContent(initialContent ?? '');
    const schedule = config.schedule;
    if (schedule.kind === 'At') {
      setTab('at');
      const parts = new Intl.DateTimeFormat('en-CA', {
        timeZone: schedule.timezone,
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        hourCycle: 'h23',
      }).formatToParts(new Date(schedule.at));
      const values = Object.fromEntries(parts.map((part) => [part.type, part.value]));
      setAtDate(`${values.year}-${values.month}-${values.day}`);
      setAtTime(`${values.hour}:${values.minute}`);
      setTimezone(schedule.timezone);
    } else if (schedule.kind === 'Every') {
      setTab('repeat');
      setFrequency('every');
      setEveryValue(String(schedule.every.value));
      setEveryUnit(schedule.every.unit);
      setTimezone(schedule.timezone);
    } else if (schedule.kind === 'Cron') {
      setTab('cron');
      setCron(schedule.expression);
      setTimezone(schedule.timezone);
    } else {
      setTab('repeat');
      setTimezone(schedule.timezone);
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

  useEffect(() => {
    if (open) syncInitialConfig(initialConfig, initialContent);
  }, [initialConfig, initialContent, open]);

  const repeatDescription = useMemo(() => {
    if (frequency === 'hourly') return '整点执行';
    if (frequency === 'daily') return `每天 ${repeatTime}`;
    if (frequency === 'weekdays') return `工作日 ${repeatTime}`;
    if (frequency === 'weekly') return `每周 ${selectedWeekdays.join('、')} ${repeatTime}`;
    return `每隔 ${everyValue || '1'} ${everyUnit === 'minutes' ? '分钟' : '小时'}`;
  }, [everyUnit, everyValue, frequency, repeatTime, selectedWeekdays]);

  const canSave = tab !== 'at' || Boolean(atDate && atTime);

  const save = async () => {
    if (!canSave) return;
    const schedule: ScheduledScheduleSpec = tab === 'at'
      ? { kind: 'At', at: zonedDateTimeToUtcIso(atDate, atTime, timezone), timezone }
      : tab === 'cron'
        ? { kind: 'Cron', expression: cron, timezone }
        : frequency === 'every'
          ? { kind: 'Every', every: { value: Math.max(1, Number(everyValue) || 1), unit: everyUnit }, anchorAt: new Date().toISOString(), timezone }
          : { kind: 'Repeat', preset: frequency === 'weekly' ? { Weekly: { weekdays: selectedWeekdays } } : frequency === 'hourly' ? 'Hourly' : frequency === 'weekdays' ? 'Weekdays' : 'Daily', hour: Number(repeatTime.slice(0, 2)) || 0, minute: Number(repeatTime.slice(3, 5)) || 0, timezone };
    setSaving(true);
    try {
      await onSave({ schedule, overlapPolicy: queueProtection ? 'skip_when_running' : 'retry_when_busy', sessionPolicy: allowContinuous ? sessionPolicy : 'new' }, showContent ? content : undefined);
      onOpenChange(false);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-[590px] gap-0 overflow-hidden p-0">
        <DialogHeader className="border-b border-border/60 px-5 py-4 text-left">
          <DialogTitle className="flex items-center gap-2 text-base"><AlarmClock className="size-4 text-primary" />定时任务设置</DialogTitle>
        </DialogHeader>
        <div className="space-y-5 p-5">
          {showContent ? <label className="block space-y-2 text-xs text-muted-foreground">任务内容<Textarea value={content} onChange={(event) => setContent(event.target.value)} className="min-h-28 resize-y text-sm" /></label> : null}
          <Tabs value={tab} onValueChange={(value) => setTab(value as ScheduleTab)}>
            <TabsList className="grid w-full grid-cols-3">
              <TabsTrigger value="at">单次</TabsTrigger>
              <TabsTrigger value="repeat">重复</TabsTrigger>
              <TabsTrigger value="cron">Cron</TabsTrigger>
            </TabsList>
          </Tabs>

          {tab === 'at' ? <div className="grid grid-cols-2 gap-3"><label className="space-y-2 text-xs text-muted-foreground">日期<Input type="date" value={atDate} onChange={(event) => setAtDate(event.target.value)} /></label><label className="space-y-2 text-xs text-muted-foreground">时间<Input type="time" value={atTime} onChange={(event) => setAtTime(event.target.value)} /></label></div> : null}

          {tab === 'repeat' ? <div className="space-y-4">
            <label className="block space-y-2 text-xs text-muted-foreground">频率<Select value={frequency} onValueChange={(value) => setFrequency(value as RepeatFrequency)}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="hourly">每小时</SelectItem><SelectItem value="daily">每天</SelectItem><SelectItem value="weekdays">工作日</SelectItem><SelectItem value="weekly">每周</SelectItem><SelectItem value="every">每隔</SelectItem></SelectContent></Select></label>
            {frequency === 'weekly' ? <div className="space-y-2"><span className="text-xs text-muted-foreground">星期（可多选）</span><div className="grid grid-cols-7 gap-1.5">{weekdays.map(([day, label]) => <Button key={day} type="button" variant={selectedWeekdays.includes(day) ? 'secondary' : 'outline'} className="h-9 px-0 text-xs" aria-pressed={selectedWeekdays.includes(day)} onClick={() => setSelectedWeekdays((current) => current.includes(day) ? current.filter((item) => item !== day) : [...current, day])}>{label}</Button>)}</div></div> : null}
            {frequency !== 'every' && frequency !== 'hourly' ? <label className="block space-y-2 text-xs text-muted-foreground">执行时间<Input type="time" value={repeatTime} onChange={(event) => setRepeatTime(event.target.value)} /></label> : null}
            {frequency === 'every' ? <label className="block space-y-2 text-xs text-muted-foreground">间隔<div className="grid grid-cols-2 gap-2"><Input type="number" min="1" value={everyValue} onChange={(event) => setEveryValue(event.target.value)} /><Select value={everyUnit} onValueChange={(value) => setEveryUnit(value as typeof everyUnit)}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent><SelectItem value="minutes">分钟</SelectItem><SelectItem value="hours">小时</SelectItem></SelectContent></Select></div></label> : null}
            <div className="flex items-center gap-2 text-xs text-muted-foreground"><CalendarClock className="size-3.5 text-primary" />下次执行：{repeatDescription} · {timezoneOptions.find((option) => option.value === timezone)?.label}</div>
            <div className="flex items-start gap-2 text-xs text-muted-foreground"><Info className="mt-0.5 size-3.5 shrink-0" /><span><strong className="font-medium text-primary">{frequency === 'every' ? '每隔' : frequency === 'weekly' ? '每周' : frequency === 'weekdays' ? '工作日' : frequency === 'hourly' ? '每小时' : '每天'}</strong> 将按当前配置计算下一次执行时间。</span></div>
          </div> : null}

          {tab === 'cron' ? <div className="space-y-3"><label className="block space-y-2 text-xs text-muted-foreground">Cron 表达式<Input className="font-mono" value={cron} onChange={(event) => setCron(event.target.value)} /></label><div className="flex items-center gap-2 text-xs text-muted-foreground"><CalendarClock className="size-3.5 text-primary" />保存后显示下一次执行时间。</div></div> : null}

          <label className="block space-y-2 text-xs text-muted-foreground">时区<Select value={timezone} onValueChange={setTimezone}><SelectTrigger><SelectValue /></SelectTrigger><SelectContent>{timezoneOptions.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectContent></Select></label>

          <div className="flex items-center justify-between gap-5 border-t border-border/60 pt-4"><div><strong className="block text-xs font-medium text-foreground">队列保护</strong><span className="text-[11px] text-muted-foreground">已有执行未结束时跳过本次</span></div><Switch checked={queueProtection} onCheckedChange={setQueueProtection} aria-label="队列保护" /></div>
          {allowContinuous ? <div className="flex items-center justify-between gap-5 border-t border-border/60 pt-4"><div><strong className="block text-xs font-medium text-foreground">会话方式</strong><span className="text-[11px] text-muted-foreground">Direct 模式</span></div><div className="flex rounded-md bg-secondary p-1"><Button type="button" variant={sessionPolicy === 'new' ? 'default' : 'ghost'} size="sm" className="h-7 px-3 text-xs" onClick={() => setSessionPolicy('new')}>新会话</Button><Button type="button" variant={sessionPolicy === 'continuous' ? 'default' : 'ghost'} size="sm" className="h-7 px-3 text-xs" onClick={() => setSessionPolicy('continuous')}>持续会话</Button></div></div> : null}
        </div>
        <DialogFooter className="border-t border-border/60 px-5 py-4"><Button variant="outline" onClick={() => onOpenChange(false)}>取消</Button><Button disabled={!canSave || saving} onClick={() => void save()}>完成</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
