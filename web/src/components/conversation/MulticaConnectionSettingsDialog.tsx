import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { saveMulticaConnectionAddress } from '../../api';
import { displayAppError } from '../../i18n';
import { isValidHttpUrl } from '@/lib/multica-address';
import type { MulticaSettingsVm } from '../../types';

interface MulticaConnectionSettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  settingsVm: MulticaSettingsVm | null;
}

/**
 * Multica 连接地址设置弹窗（M5-ay：连接按钮旁设置 icon 入口）。
 *
 * 单一「连接地址」输入，保存 = API 与登录页同址（`saveMulticaConnectionAddress(v, v)`）；
 * 分端口形态（内网默认 API 8080 / 登录页 3000）只由渠道默认产生，弹窗不提供预设选择。
 * **地址与当前生效值相同时保存禁用**——手动保存会把两值写同址，若打开弹窗直接点保存，
 * 会把分端口默认误覆盖成同址（8080/8080）。「恢复默认地址」双 null 清除覆盖回落渠道
 * 编译期默认，用返回 VM 刷新弹窗字段（留窗可继续编辑）。
 */
export function MulticaConnectionSettingsDialog({
  open,
  onOpenChange,
  settingsVm,
}: MulticaConnectionSettingsDialogProps) {
  const { t } = useTranslation();
  const [vm, setVm] = useState<MulticaSettingsVm | null>(null);
  const [address, setAddress] = useState('');
  const [saving, setSaving] = useState(false);
  const [restoring, setRestoring] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 每次打开：从最新 settingsVm 预填。ref 读最新值、仅以 open 为触发——弹窗开着时
  // settings-updated 事件会刷新页面 settingsVm，不应覆写用户正在编辑的输入。
  const settingsRef = useRef(settingsVm);
  settingsRef.current = settingsVm;
  useEffect(() => {
    if (!open) return;
    const latest = settingsRef.current;
    setVm(latest);
    setAddress(latest?.multicaBaseUrl ?? '');
    setError(null);
  }, [open]);

  const trimmed = address.trim();
  const addressInvalid = trimmed !== '' && !isValidHttpUrl(trimmed);
  // 未变更禁用保存：防「打开看看就保存」把分端口默认（8080/3000）误写成同址覆盖（8080/8080）。
  const unchanged = trimmed === (vm?.multicaBaseUrl ?? '');
  const canSave = trimmed !== '' && !addressInvalid && !unchanged;

  async function handleSave() {
    if (!canSave || saving || restoring) return;
    setSaving(true);
    setError(null);
    try {
      // 手动保存 = API 与登录页同址（分端口只经渠道默认产生，本弹窗无预设选择）。
      await saveMulticaConnectionAddress(trimmed, trimmed);
      onOpenChange(false);
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setSaving(false);
    }
  }

  async function handleRestoreDefault() {
    setRestoring(true);
    setError(null);
    try {
      // 双 null = 清除覆盖，回落渠道编译期默认；用返回 VM 的生效地址刷新弹窗字段（留窗）。
      const next = await saveMulticaConnectionAddress(null, null);
      setVm(next);
      setAddress(next.multicaBaseUrl ?? '');
    } catch (err) {
      setError(displayAppError(t, err));
    } finally {
      setRestoring(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[85vh] max-w-md flex-col overflow-hidden gap-0 p-0">
        <DialogHeader className="shrink-0 p-6 pb-0">
          <DialogTitle>{t('multica.connection.title')}</DialogTitle>
        </DialogHeader>

        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-6">
          <div className="space-y-1">
            <div className="text-xs font-medium text-muted-foreground">
              {t('multica.connection.addressLabel')}
            </div>
            <Input
              value={address}
              onChange={(e) => setAddress(e.target.value)}
              disabled={saving || restoring}
              placeholder="http://localhost:8080"
              className="h-9 font-mono text-xs"
              spellCheck={false}
              aria-label={t('multica.connection.addressLabel')}
            />
            {addressInvalid && (
              <p className="text-[11px] text-destructive">{t('multica.connection.invalidUrl')}</p>
            )}
          </div>

          {vm?.addressOverrideSet && (
            <Button
              type="button"
              variant="link"
              size="sm"
              className="h-auto p-0 text-xs"
              disabled={restoring || saving}
              onClick={() => void handleRestoreDefault()}
            >
              {restoring ? <Loader2 className="mr-1.5 size-3 animate-spin" /> : null}
              {restoring
                ? t('multica.connection.restoring')
                : t('multica.connection.restoreDefault')}
            </Button>
          )}

          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>

        <DialogFooter className="shrink-0 border-t border-border/60 p-6 pt-4">
          <Button
            type="button"
            size="sm"
            disabled={!canSave || saving || restoring}
            onClick={() => void handleSave()}
          >
            {saving ? <Loader2 className="mr-1.5 size-3.5 animate-spin" /> : null}
            {t('multica.connection.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
