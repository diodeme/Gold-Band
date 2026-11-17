import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { KeyRound } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import type { TerminalOtpRequest } from '@/lib/terminal-otp-bus';

/**
 * 会话内口令输入卡片（连接→对话）：agent 通过连接 MCP 工具打开的远程会话进入
 * 口令等待时，输入框直接出现在发起它的对话里（AskUserQuestion 式，非阻塞），
 * 与终端面板解耦。提交走 provide_terminal_auth，口令不经过模型。
 */
export function ConversationOtpCard({
  request,
  onSubmit,
  onDismiss,
}: {
  request: TerminalOtpRequest;
  onSubmit: (sessionId: string, answer: string) => Promise<void>;
  onDismiss: (sessionId: string) => void;
}) {
  const { t } = useTranslation();
  const [answer, setAnswer] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    if (!answer.trim() || submitting) return;
    setSubmitting(true);
    setError(null);
    try {
      await onSubmit(request.sessionId, answer);
    } catch {
      // 提交被拒绝（认证失败 / 会话已结束）：保留输入让用户重试或放弃。
      setSubmitting(false);
      setError(t('terminal.otpSubmitFailed'));
    }
  };

  return (
    <div
      data-conversation-otp-card="true"
      className="mx-2 mb-2 flex items-center gap-2 rounded-lg border border-border/60 bg-transparent px-3 py-2"
    >
      <KeyRound aria-hidden className="size-4 shrink-0 text-foreground" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="truncate text-xs font-medium text-foreground">
          {request.label
            ? t('terminal.otpDialogTitleNamed', { name: request.label })
            : t('terminal.otpDialogTitle')}
        </div>
        {request.message ? (
          <p className="truncate text-xs text-muted-foreground">{request.message}</p>
        ) : null}
        {error ? (
          <p className="text-xs text-destructive" role="alert">{error}</p>
        ) : null}
      </div>
      <Input
        type="password"
        value={answer}
        onChange={(event) => setAnswer(event.target.value)}
        placeholder={t('terminal.authPromptPlaceholder')}
        aria-label={t('terminal.authPromptPlaceholder')}
        autoFocus
        disabled={submitting}
        className="h-8 w-44 shrink-0"
        onKeyDown={(event) => { if (event.key === 'Enter') void submit(); }}
      />
      <Button size="sm" className="h-8 shrink-0" disabled={!answer.trim() || submitting} onClick={() => void submit()}>
        {t('terminal.authPromptSubmit')}
      </Button>
      <Button
        size="sm"
        variant="ghost"
        className="h-8 shrink-0"
        disabled={submitting}
        onClick={() => onDismiss(request.sessionId)}
      >
        {t('common.cancel')}
      </Button>
    </div>
  );
}
