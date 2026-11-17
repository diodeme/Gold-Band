import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent } from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

export interface TerminalOtpDialogState {
  sessionId: string;
  message: string;
  /** 连接名（用于标题展示），缺失时回退到通用标题。 */
  label?: string | null;
}

/**
 * OTP 代输弹窗（连接→对话）：agent 驱动的远程会话需要动态口令时的**权威输入口**。
 * 与终端面板挂载状态完全解耦——authPrompt 事件在 App 壳层触发，无论终端面板是否
 * 打开、是否错过事件，弹窗都会出现。提交走既有 provide_terminal_auth 通道；关闭
 * （取消）表示放弃本次输入，会话将在 120s 后以 auth-required 失败，agent 可自动
 * 重试 open_connection 触发新弹窗。
 */
export function TerminalOtpDialog({
  otp,
  onSubmit,
  onClose,
}: {
  otp: TerminalOtpDialogState;
  onSubmit: (sessionId: string, answer: string) => Promise<void>;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [answer, setAnswer] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // sessionId 变化（新弹窗）时重置输入。
  useEffect(() => {
    setAnswer('');
    setError(null);
  }, [otp.sessionId, otp.message]);

  const submit = async () => {
    if (!answer.trim() || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(otp.sessionId, answer);
      onClose();
    } catch {
      setError(t('terminal.otpSubmitFailed'));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent className="max-w-md" data-terminal-otp-dialog="true">
        <div className="flex flex-col gap-3">
          <div className="text-sm font-medium text-foreground">
            {otp.label
              ? t('terminal.otpDialogTitleNamed', { name: otp.label })
              : t('terminal.otpDialogTitle')}
          </div>
          <p className="text-xs text-muted-foreground">{otp.message}</p>
          <Input
            type="password"
            value={answer}
            onChange={(event) => setAnswer(event.target.value)}
            placeholder={t('terminal.authPromptPlaceholder')}
            autoFocus
            disabled={submitting}
            onKeyDown={(event) => { if (event.key === 'Enter') void submit(); }}
          />
          {error ? (
            <p className="text-xs text-destructive" role="alert">{error}</p>
          ) : null}
          <div className="flex items-center justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={onClose} disabled={submitting}>
              {t('common.cancel')}
            </Button>
            <Button size="sm" onClick={() => void submit()} disabled={!answer.trim() || submitting}>
              {t('terminal.authPromptSubmit')}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
