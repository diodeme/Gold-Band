import { useEffect, useState } from 'react';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';
import { Loader2, RefreshCw } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { cn } from '@/lib/utils';
import {
  getMulticaSettings,
  listMulticaSkills,
  pullMulticaSkills,
} from '../api';
import { displayAppError } from '../i18n';
import type {
  MulticaPullItemResultVm,
  MulticaPullReportVm,
  MulticaSettingsVm,
  MulticaSkillListItemVm,
} from '../types';

interface MulticaSkillSyncDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /// 同步完成（弹窗关闭）后回调，调用方刷新本地 SKILL 列表。
  onFinished: () => void;
}

/// SKILL 管理页「从 Multica 同步」弹窗：选择工作空间 → 勾选远端 SKILL（新增默认勾选，
/// 已存在默认不勾选并提示将覆盖）→ 同步 → 展示逐项结果报告。
/// 拉取走 PAT REST（list_multica_skills / pull_multica_skills 命令），与心跳推送通道无关。
export function MulticaSkillSyncDialog({
  open,
  onOpenChange,
  onFinished,
}: MulticaSkillSyncDialogProps) {
  const { t } = useTranslation();
  // phase：select = 选择阶段；report = 同步结果报告阶段（不可回退，关闭后由调用方刷新列表）。
  const [phase, setPhase] = useState<'select' | 'report'>('select');
  const [settings, setSettings] = useState<MulticaSettingsVm | null>(null);
  const [workspaceId, setWorkspaceId] = useState('');
  const [skills, setSkills] = useState<MulticaSkillListItemVm[]>([]);
  const [selected, setSelected] = useState<Record<string, boolean>>({});
  const [loadingList, setLoadingList] = useState(false);
  const [pulling, setPulling] = useState(false);
  // 手动「重新加载」用 nonce 触发列表 effect 重跑（工作空间 id 不变时也生效）。
  const [reloadNonce, setReloadNonce] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [report, setReport] = useState<MulticaPullReportVm | null>(null);

  // 每次打开：重置到选择阶段并拉取连接状态 + 工作空间列表。
  useEffect(() => {
    if (!open) return;
    setPhase('select');
    setSettings(null);
    setWorkspaceId('');
    setSkills([]);
    setSelected({});
    setError(null);
    setReport(null);
    let cancelled = false;
    getMulticaSettings()
      .then((next) => { if (!cancelled) setSettings(next); })
      .catch((err) => { if (!cancelled) setError(displayAppError(t, err)); });
    return () => { cancelled = true; };
  }, [open, t]);

  // 连接就绪后选定默认工作空间（活跃空间优先，否则第一个）并加载远端 SKILL 列表。
  useEffect(() => {
    if (!open || !settings) return;
    if (!settings.connected || settings.workspaces.length === 0) return;
    const fallback =
      settings.activeWorkspaceId && settings.workspaces.some((w) => w.id === settings.activeWorkspaceId)
        ? settings.activeWorkspaceId
        : settings.workspaces[0].id;
    setWorkspaceId((current) => current || fallback);
  }, [open, settings]);

  // 工作空间确定/切换/手动重载后加载列表：新增默认勾选，已存在默认不勾选（避免误覆盖）。
  useEffect(() => {
    if (!open || !workspaceId) return;
    let cancelled = false;
    setLoadingList(true);
    setError(null);
    listMulticaSkills(workspaceId)
      .then((list) => {
        if (cancelled) return;
        setSkills(list);
        const next: Record<string, boolean> = {};
        for (const item of list) next[item.id] = item.localState === 'new';
        setSelected(next);
      })
      .catch((err) => {
        if (cancelled) return;
        setSkills([]);
        setSelected({});
        setError(displayAppError(t, err));
      })
      .finally(() => { if (!cancelled) setLoadingList(false); });
    return () => { cancelled = true; };
  }, [open, workspaceId, reloadNonce, t]);

  const selectedIds = skills.filter((item) => selected[item.id]);
  const selectedExistsCount = selectedIds.filter((item) => item.localState === 'exists').length;

  async function handleSync() {
    if (selectedIds.length === 0 || !workspaceId) return;
    setPulling(true);
    setError(null);
    try {
      const nextReport = await pullMulticaSkills(
        workspaceId,
        selectedIds.map((item) => item.id),
      );
      setReport(nextReport);
      setPhase('report');
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setPulling(false);
    }
  }

  function handleClose() {
    onOpenChange(false);
    // 有同步结果（即使部分失败）才刷新列表；纯取消不触发。
    if (report) onFinished();
  }

  function renderSelectPhase() {
    const notConnected = settings !== null && !settings.connected;
    return (
      <>
        <div className="min-h-0 flex-1 space-y-3 p-6">
          {notConnected && (
            <p className="text-xs leading-relaxed text-muted-foreground">
              {t('contextManagement.skills.multicaSync.notConnected')}
            </p>
          )}
          {!notConnected && (
            <>
              <div className="flex items-end gap-2">
                <div className="min-w-0 flex-1 space-y-1">
                  <div className="text-xs font-medium text-muted-foreground">
                    {t('contextManagement.skills.multicaSync.workspace')}
                  </div>
                  <Select
                    value={workspaceId || undefined}
                    onValueChange={setWorkspaceId}
                    disabled={loadingList || pulling || (settings?.workspaces.length ?? 0) < 2}
                  >
                    <SelectTrigger className="h-9 min-w-0 text-xs">
                      <SelectValue placeholder={loadingList ? '…' : t('contextManagement.skills.multicaSync.workspace')} />
                    </SelectTrigger>
                    <SelectContent>
                      {(settings?.workspaces ?? []).map((ws) => (
                        <SelectItem key={ws.id} value={ws.id} className="text-xs">{ws.name}</SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  className="size-9 shrink-0"
                  disabled={!workspaceId || loadingList || pulling}
                  aria-label={t('contextManagement.skills.multicaSync.reload')}
                  onClick={() => setReloadNonce((n) => n + 1)}
                >
                  <RefreshCw className={cn('size-4', loadingList && 'animate-spin')} />
                </Button>
              </div>

              <ScrollArea className="min-h-0 flex-1 rounded-md border border-border/60">
                <div className="p-2">
                  {loadingList && (
                    <div className="flex items-center justify-center gap-2 p-6 text-xs text-muted-foreground">
                      <Loader2 className="size-3.5 animate-spin" />
                    </div>
                  )}
                  {!loadingList && skills.length === 0 && (
                    <p className="p-6 text-center text-xs text-muted-foreground">
                      {t('contextManagement.skills.multicaSync.empty')}
                    </p>
                  )}
                  {!loadingList && skills.map((item) => (
                    <label
                      key={item.id}
                      className="flex cursor-pointer items-start gap-2 rounded-md px-2 py-1.5 hover:bg-muted/50"
                    >
                      <Checkbox
                        className="mt-0.5"
                        checked={!!selected[item.id]}
                        disabled={pulling}
                        onCheckedChange={(checked) =>
                          setSelected((prev) => ({ ...prev, [item.id]: checked === true }))
                        }
                      />
                      <span className="min-w-0 flex-1">
                        <span className="flex items-center gap-2">
                          <span className="truncate text-xs font-medium">{item.name}</span>
                          {item.localState === 'new' ? (
                            <Badge variant="secondary" className="h-4 px-1.5 text-[10px]">
                              {t('contextManagement.skills.multicaSync.newBadge')}
                            </Badge>
                          ) : (
                            <Badge variant="outline" className="h-4 px-1.5 text-[10px] text-muted-foreground">
                              {t('contextManagement.skills.multicaSync.existsBadge')}
                            </Badge>
                          )}
                        </span>
                        {item.description && (
                          <span className="mt-0.5 block truncate text-[11px] text-muted-foreground">
                            {item.description}
                          </span>
                        )}
                      </span>
                    </label>
                  ))}
                </div>
              </ScrollArea>
            </>
          )}
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>

        {!notConnected && (
          <DialogFooter className="shrink-0 border-t border-border/60 p-6 pt-4">
            <Button
              type="button"
              size="sm"
              disabled={pulling || selectedIds.length === 0}
              onClick={() => void handleSync()}
            >
              {pulling ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}
              {selectedExistsCount > 0
                ? t('contextManagement.skills.multicaSync.confirmWithOverwrite', { count: selectedExistsCount })
                : t('contextManagement.skills.multicaSync.confirm')}
            </Button>
          </DialogFooter>
        )}
      </>
    );
  }

  function renderReportPhase() {
    const results = report?.results ?? [];
    const counts = results.reduce(
      (acc, item) => { acc[item.outcome] = (acc[item.outcome] ?? 0) + 1; return acc; },
      {} as Record<string, number>,
    );
    return (
      <>
        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-6">
          <p className="text-xs text-muted-foreground">
            {t('contextManagement.skills.multicaSync.reportSummary', {
              created: counts.created ?? 0,
              overwritten: counts.overwritten ?? 0,
              skipped: counts.skipped ?? 0,
              failed: counts.failed ?? 0,
            })}
          </p>
          <div className="space-y-1">
            {results.map((item) => (
              <ReportRow key={item.id} item={item} t={t} />
            ))}
          </div>
        </div>
        <DialogFooter className="shrink-0 border-t border-border/60 p-6 pt-4">
          <Button type="button" size="sm" onClick={handleClose}>
            {t('common.close')}
          </Button>
        </DialogFooter>
      </>
    );
  }

  return (
    <Dialog open={open} onOpenChange={(next) => { if (!next) handleClose(); else onOpenChange(next); }}>
      <DialogContent className="flex max-h-[85vh] max-w-md flex-col overflow-hidden gap-0 p-0">
        <DialogHeader className="shrink-0 p-6 pb-0">
          <DialogTitle>
            {phase === 'select'
              ? t('contextManagement.skills.multicaSync.title')
              : t('contextManagement.skills.multicaSync.reportTitle')}
          </DialogTitle>
        </DialogHeader>
        {phase === 'select' ? renderSelectPhase() : renderReportPhase()}
      </DialogContent>
    </Dialog>
  );
}

function cap(value: string) {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

/// 报告阶段单行：名称 + 结果徽标 + （跳过/失败时）原因文案。
/// 原因/结果文案用 i18n key 兜底原始值（新增错误码未翻译时不至于显示空白）。
function ReportRow({ item, t }: { item: MulticaPullItemResultVm; t: TFunction }) {
  const reasonText = item.reason
    ? t(`contextManagement.skills.multicaSync.reason.${item.reason}`, {
        defaultValue: item.reason,
      })
    : null;
  return (
    <div className="flex items-center gap-2 rounded-md px-2 py-1.5">
      <span className="min-w-0 flex-1 truncate text-xs">{item.name}</span>
      <Badge variant="outline" className={cn('h-4 shrink-0 px-1.5 text-[10px]', outcomeBadgeClass(item.outcome))}>
        {t(`contextManagement.skills.multicaSync.outcome${cap(item.outcome)}`, {
          defaultValue: item.outcome,
        })}
      </Badge>
      {reasonText && (
        <span className="max-w-[45%] shrink-0 truncate text-[11px] text-muted-foreground" title={reasonText}>
          {reasonText}
        </span>
      )}
    </div>
  );
}

/// 结果徽标配色：created 主色、overwritten 中性、failed 错误色、skipped 弱化。
function outcomeBadgeClass(outcome: string) {
  switch (outcome) {
    case 'created':
      return 'bg-primary/15 text-primary border-transparent';
    case 'overwritten':
      return 'bg-muted text-foreground border-transparent';
    case 'failed':
      return 'bg-destructive/15 text-destructive border-transparent';
    default:
      return 'text-muted-foreground';
  }
}
