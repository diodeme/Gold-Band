import { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, ExternalLink, Loader2, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import {
  connectMultica,
  disconnectMultica,
  getMulticaSettings,
  openExternalUrl,
  removeMulticaWorkspace,
  saveMulticaSettings,
  setActiveMulticaWorkspace,
  subscribeMulticaSettingsUpdates,
} from '../../api';
import { displayAppError } from '../../i18n';
import type { MulticaSettingsVm } from '../../types';

/// multica 接入 provider 选项（绑定后不可变；与 agent registry agentType 对齐）。
const MULTICA_PROVIDER_OPTIONS = [
  { value: 'claude-acp', label: 'Claude' },
  { value: 'codex-acp', label: 'Codex' },
] as const;

/// multica 设置区块：自管理状态（照搬 metrics 的 barrel 直连 + 本地 error 回显模式）。
/// 仅渲染区块内字段，外层 `SettingsSection` 由 SettingsPage 包裹（与 metrics 一致）。
/// 工作空间「添加」入口已收敛到会话侧栏远程任务列表的弹窗（见 MulticaAddWorkspaceDialog）；
/// 本区块只保留配置 + 已绑定工作空间的管理（激活/删除）。
export function MulticaSettingsBlock() {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<MulticaSettingsVm | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [baseUrl, setBaseUrl] = useState('');
  const [appUrl, setAppUrl] = useState('');
  const [defaultProvider, setDefaultProvider] = useState<string>(MULTICA_PROVIDER_OPTIONS[0].value);
  const [saving, setSaving] = useState(false);
  const [connecting, setConnecting] = useState(false);
  const [disconnecting, setDisconnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [workspaceBusy, setWorkspaceBusy] = useState<string | null>(null);

  const refresh = useCallback(() => {
    getMulticaSettings()
      .then((vm) => {
        setSettings(vm);
        setEnabled(vm.enabled);
        setBaseUrl(vm.multicaBaseUrl ?? '');
        setAppUrl(vm.multicaAppUrl ?? '');
        setDefaultProvider(vm.defaultProvider || MULTICA_PROVIDER_OPTIONS[0].value);
      })
      .catch((err) => setError(displayAppError(t, err)));
  }, [t]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 工作空间绑定/连接态可能在别处变更（会话侧栏远程任务列表的添加工作空间弹窗），
  // 订阅 multica-settings-updated 即时刷新本区块，杜绝显示旧数据。
  useEffect(() => {
    let unsub = () => {};
    subscribeMulticaSettingsUpdates(() => refresh()).then((fn) => { unsub = fn; });
    return () => { unsub(); };
  }, [refresh]);

  function applyVm(vm: MulticaSettingsVm) {
    setSettings(vm);
    setEnabled(vm.enabled);
    setBaseUrl(vm.multicaBaseUrl ?? '');
    setAppUrl(vm.multicaAppUrl ?? '');
    setDefaultProvider(vm.defaultProvider || MULTICA_PROVIDER_OPTIONS[0].value);
  }

  function catchError(err: unknown) {
    setError(displayAppError(t, err));
  }

  async function handleSave() {
    setSaving(true);
    setError(null);
    try {
      const vm = await saveMulticaSettings(
        enabled,
        baseUrl || null,
        appUrl || null,
        defaultProvider || null,
        settings?.activeWorkspaceId ?? null,
      );
      applyVm(vm);
    } catch (err) {
      catchError(err);
    } finally {
      setSaving(false);
    }
  }

  async function handleConnect() {
    setConnecting(true);
    setError(null);
    try {
      const vm = await connectMultica();
      applyVm(vm);
    } catch (err) {
      catchError(err);
    } finally {
      setConnecting(false);
    }
  }

  async function handleDisconnect() {
    setDisconnecting(true);
    setError(null);
    try {
      const vm = await disconnectMultica();
      applyVm(vm);
    } catch (err) {
      catchError(err);
    } finally {
      setDisconnecting(false);
    }
  }

  // 切换账号逃生口：码灵把认证委托给浏览器，浏览器 cookie 不受控——若连到了非预期账号，
  // 此处打开 multica Web（在浏览器内登出当前账号 / 登录目标账号），再回此点「切换账号」。
  // 根因（webank 见 cookie 即签 JWT）需在 multica-webank 侧加授权确认屏，见设计文档 M5-l。
  async function handleSwitchAccount() {
    if (!appUrl) return;
    await openExternalUrl(appUrl);
  }

  async function handleRemove(id: string) {
    setWorkspaceBusy(`remove:${id}`);
    setError(null);
    try {
      const vm = await removeMulticaWorkspace(id);
      applyVm(vm);
    } catch (err) {
      catchError(err);
    } finally {
      setWorkspaceBusy(null);
    }
  }

  async function handleSetActive(id: string) {
    setWorkspaceBusy(`active:${id}`);
    setError(null);
    try {
      const vm = await setActiveMulticaWorkspace(id);
      applyVm(vm);
    } catch (err) {
      catchError(err);
    } finally {
      setWorkspaceBusy(null);
    }
  }

  const toggleLocked = settings?.toggleLocked ?? false;
  const patSet = settings?.patSet ?? false;
  const daemonIdSet = settings?.daemonIdSet ?? false;
  const connected = settings?.connected ?? false;
  const connectedAccount = settings?.connectedAccount ?? null;
  const workspaces = settings?.workspaces ?? [];

  return (
    <div className="max-w-4xl space-y-3">
      <div className="flex items-center gap-3">
        <p className="text-sm font-medium text-muted-foreground">{t('settings.multica.enable')}</p>
        <Switch
          checked={enabled}
          disabled={toggleLocked}
          onCheckedChange={setEnabled}
        />
      </div>
      <p className="text-xs leading-5 text-muted-foreground">{t('settings.multica.enableDescription')}</p>

      {enabled && (
        <>
          <div className="space-y-1">
            <div className="text-xs font-medium text-muted-foreground">{t('settings.multica.baseUrl')}</div>
            <Input
              value={baseUrl}
              placeholder="https://multica.example.com"
              disabled={toggleLocked}
              className="h-9 min-w-0 font-mono text-xs"
              onChange={(event) => setBaseUrl(event.target.value)}
            />
          </div>
          <div className="space-y-1">
            <div className="text-xs font-medium text-muted-foreground">{t('settings.multica.appUrl')}</div>
            <Input
              value={appUrl}
              placeholder="https://multica.example.com/app"
              disabled={toggleLocked}
              className="h-9 min-w-0 font-mono text-xs"
              onChange={(event) => setAppUrl(event.target.value)}
            />
          </div>
          <div className="space-y-1">
            <div className="text-xs font-medium text-muted-foreground">{t('settings.multica.defaultProvider')}</div>
            <Select value={defaultProvider} onValueChange={setDefaultProvider} disabled={toggleLocked}>
              <SelectTrigger className="h-9 min-w-0 font-mono text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {MULTICA_PROVIDER_OPTIONS.map((opt) => (
                  <SelectItem key={opt.value} value={opt.value} className="font-mono text-xs">
                    {opt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" variant="outline" disabled={connecting || toggleLocked} onClick={() => void handleConnect()}>
              {connecting ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}
              {connected ? t('settings.multica.reconnect') : t('settings.multica.connect')}
            </Button>
            {connected && (
              <Button
                size="sm"
                variant="ghost"
                className="text-muted-foreground hover:text-destructive"
                disabled={disconnecting || toggleLocked}
                onClick={() => void handleDisconnect()}
              >
                {disconnecting ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}
                {t('settings.multica.disconnect')}
              </Button>
            )}
            <span className={cn('text-xs', patSet ? 'text-gold-success' : 'text-muted-foreground')}>
              {patSet ? t('settings.multica.patSet') : t('settings.multica.patNotSet')}
            </span>
            <span className="text-muted-foreground/50">·</span>
            <span className={cn('text-xs', daemonIdSet ? 'text-gold-success' : 'text-muted-foreground')}>
              {daemonIdSet ? t('settings.multica.daemonIdSet') : t('settings.multica.daemonIdNotSet')}
            </span>
          </div>

          {/* 已连接账号身份（核对浏览器是否静默连到非预期账号）+ 切换账号逃生口 */}
          {connected && connectedAccount?.email && (
            <div className="flex items-center gap-2">
              <span className="text-xs text-muted-foreground">
                {t('settings.multica.connectedAccount')}：
                {connectedAccount.name ? `${connectedAccount.name} ` : ''}
                <span className="font-mono">{connectedAccount.email}</span>
              </span>
              <Tooltip>
                <TooltipTrigger asChild>
                  <span>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="size-7"
                      disabled={!appUrl || toggleLocked}
                      onClick={() => void handleSwitchAccount()}
                    >
                      <ExternalLink className="size-3.5" />
                    </Button>
                  </span>
                </TooltipTrigger>
                <TooltipContent side="top" className="text-xs">{t('settings.multica.switchAccountHint')}</TooltipContent>
              </Tooltip>
            </div>
          )}

          <div className="flex justify-end">
            <Button size="sm" onClick={() => void handleSave()} disabled={saving || toggleLocked}>
              {saving ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}
              {t('common.save')}
            </Button>
          </div>

          {/* 已绑定工作空间管理（激活/删除）；添加入口在会话侧栏远程任务列表弹窗 */}
          <div className="space-y-2 border-t border-border/45 pt-3">
            <div className="text-xs font-medium text-muted-foreground">{t('settings.multica.workspaceList')}</div>

            {workspaces.length === 0 ? (
              <p className="text-xs text-muted-foreground">{t('settings.multica.noWorkspaces')}</p>
            ) : (
              <ul className="space-y-1">
                {workspaces.map((ws) => {
                  const isActive = settings?.activeWorkspaceId === ws.id;
                  return (
                    <li key={ws.id} className="flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-muted/40">
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          <span className="truncate text-sm text-foreground">{ws.name}</span>
                          {isActive && (
                            <span className="inline-flex items-center gap-0.5 text-xs text-gold-success">
                              <Check className="size-3" />
                              {t('settings.multica.active')}
                            </span>
                          )}
                        </div>
                        <div className="truncate font-mono text-[11px] text-muted-foreground">
                          {ws.provider}
                        </div>
                      </div>
                      <div className="flex shrink-0 items-center gap-0.5">
                        {!isActive && (
                          <Tooltip>
                            <TooltipTrigger asChild>
                              <span>
                                <Button
                                  size="icon"
                                  variant="ghost"
                                  className="size-7"
                                  disabled={workspaceBusy !== null}
                                  onClick={() => void handleSetActive(ws.id)}
                                >
                                  <Check className="size-3.5" />
                                </Button>
                              </span>
                            </TooltipTrigger>
                            <TooltipContent side="top" className="text-xs">{t('settings.multica.setActive')}</TooltipContent>
                          </Tooltip>
                        )}
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span>
                              <Button
                                size="icon"
                                variant="ghost"
                                className="size-7 text-muted-foreground hover:text-destructive"
                                disabled={workspaceBusy !== null}
                                onClick={() => void handleRemove(ws.id)}
                              >
                                <Trash2 className="size-3.5" />
                              </Button>
                            </span>
                          </TooltipTrigger>
                          <TooltipContent side="top" className="text-xs">{t('settings.multica.remove')}</TooltipContent>
                        </Tooltip>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        </>
      )}

      {error && <p className="text-xs text-destructive">{error}</p>}
    </div>
  );
}
