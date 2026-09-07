import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { Input } from '@/components/ui/input';
import { isValidHttpUrl } from '@/lib/multica-address';
import { cancelMulticaConnect, connectMultica, saveMulticaConnectionAddress } from '../../api';
import { displayAppError } from '../../i18n';
import type { MulticaSettingsVm } from '../../types';

/// 后端结构化错误形如 { code, params }（AppErrorVm）；取消判定只看 code，不依赖文案。
function isConnectCancelled(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    (error as { code?: unknown }).code === 'multica.connect-cancelled'
  );
}

interface MulticaConnectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  settingsVm: MulticaSettingsVm | null;
  /// 连接成功后的回调（调用方刷新任务列表 + 连接态）。
  onConnected?: () => void;
}

/**
 * Multica 连接确认弹窗（M5-ay）：确认 + 可改地址。
 *
 * 预填当前生效地址、可直接编辑（校验 http(s) URL，非法禁用连接）。确认时**地址有变才保存**
 * （API 与登录页同址 `(v, v)`）；未变不写——防「原样确认」把分端口渠道默认（API 8080 /
 * 登录页 3000）误覆盖成同址。连接中（浏览器登录等待段）主按钮禁用转「连接中…」，取消按钮
 * 变「取消连接」→ `cancelMulticaConnect` 触发后端取消槽，连接命令以
 * `multica.connect-cancelled` 收尾——非失败语义，弹窗静默关闭、不作错误展示。
 */
export function MulticaConnectDialog({
  open,
  onOpenChange,
  settingsVm,
  onConnected,
}: MulticaConnectDialogProps) {
  const { t } = useTranslation();
  const [address, setAddress] = useState('');
  const [connecting, setConnecting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // 每次打开：从最新 settingsVm 预填。ref 读最新值、仅以 open 为触发——弹窗开着时
  // settings-updated 事件会刷新页面 settingsVm，不应覆写用户正在编辑的输入。
  const settingsRef = useRef(settingsVm);
  settingsRef.current = settingsVm;
  useEffect(() => {
    if (open) {
      setAddress(settingsRef.current?.multicaBaseUrl ?? '');
      setConnecting(false);
      setError(null);
    }
  }, [open]);

  const trimmed = address.trim();
  const addressInvalid = trimmed !== '' && !isValidHttpUrl(trimmed);
  const canConnect = trimmed !== '' && !addressInvalid;

  async function handleConnect() {
    if (!canConnect || connecting) return;
    setConnecting(true);
    setError(null);
    try {
      // 地址有变才保存（API 与登录页同址）；未变不写——防分端口默认被同址覆盖。
      if (trimmed !== (settingsRef.current?.multicaBaseUrl ?? '')) {
        await saveMulticaConnectionAddress(trimmed, trimmed);
      }
      await connectMultica();
      onConnected?.();
      onOpenChange(false);
    } catch (err) {
      if (isConnectCancelled(err)) {
        // 用户主动取消：静默关窗（非失败，不进错误展示）。
        onOpenChange(false);
        return;
      }
      setError(displayAppError(t, err));
    } finally {
      setConnecting(false);
    }
  }

  async function handleCancelConnect() {
    try {
      await cancelMulticaConnect();
    } catch {
      // 取消 best-effort：失败时弹窗保持打开，连接命令照常完成或超时自终。
    }
  }

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent className="max-w-md gap-0 p-0">
        <AlertDialogHeader className="p-6 pb-0">
          <AlertDialogTitle>{t('multica.connect.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('multica.connect.body')}</AlertDialogDescription>
        </AlertDialogHeader>

        <div className="space-y-2 p-6">
          <Input
            value={address}
            onChange={(e) => setAddress(e.target.value)}
            disabled={connecting}
            placeholder="http://localhost:8080"
            className="h-9 font-mono text-xs"
            spellCheck={false}
            aria-label={t('multica.connection.addressLabel')}
          />
          {addressInvalid && (
            <p className="text-[11px] text-destructive">{t('multica.connection.invalidUrl')}</p>
          )}
          {error && <p className="text-xs text-destructive">{error}</p>}
        </div>

        <AlertDialogFooter className="border-t border-border/60 p-6 pt-4">
          <AlertDialogCancel
            onClick={(e) => {
              if (!connecting) return;
              // 连接中的「取消连接」只发取消信号、不直接关窗——关窗统一由连接命令的
              // cancelled 收尾路径触发（单一关闭事实源，避免取消竞态下状态悬空）。
              e.preventDefault();
              void handleCancelConnect();
            }}
          >
            {connecting ? t('multica.connect.cancelConnect') : t('common.cancel')}
          </AlertDialogCancel>
          <AlertDialogAction
            disabled={!canConnect || connecting}
            onClick={(e) => {
              // 阻止 Radix 默认关窗：连接中/失败时弹窗保持打开，关闭只走显式完成路径。
              e.preventDefault();
              void handleConnect();
            }}
          >
            {connecting ? (
              <>
                <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                {t('multica.connect.connecting')}
              </>
            ) : (
              t('multica.connect.confirm')
            )}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
