import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import type { EnvironmentCheck } from './types';

export function EnvironmentCheckResult({ tool, check, waiting = false }: { tool: EnvironmentCheck['tool']; check?: EnvironmentCheck; waiting?: boolean }) {
  const { t } = useTranslation();
  const reason = check?.error?.params.reason;
  const explanation = reason ? t(`onboarding.environmentReasons.${String(reason)}`, { ...check?.error?.params, defaultValue: t('onboarding.environmentReasons.probe-unavailable') }) : check && check.status !== 'ready' ? t(`onboarding.environmentHelp.${check.status}`) : '';
  const statusLabel = t(`onboarding.status.${check?.status ?? 'pending'}`);
  const blocking = tool === 'git';
  const statusClass = check?.status === 'ready' ? 'text-gold-success' : !check ? 'text-muted-foreground' : blocking ? 'text-destructive' : 'text-gold-warning';
  return <div className="py-4">
    <div className="flex flex-wrap items-center justify-between gap-2 text-sm">
      <span className="font-medium">{tool === 'node' ? 'Node.js' : tool === 'git' ? 'Git' : tool} {check?.version}</span>
      {waiting ? <span role="status" className="flex items-center gap-2 text-muted-foreground"><Loader2 className="size-4 animate-spin" />{t('onboarding.checking')}</span> : <span className={statusClass}>{statusLabel}</span>}
    </div>
    {check?.executablePath && <p className="mt-2 break-all text-xs text-muted-foreground">{check.executablePath}</p>}
    {check?.nodeExecutablePath && tool !== 'node' && <p className="mt-1 break-all text-xs text-muted-foreground">{t('onboarding.actualNode', { version: check.nodeVersion ?? '', path: check.nodeExecutablePath })}</p>}
    {!waiting && explanation && <p className="mt-2 break-words text-sm text-muted-foreground [overflow-wrap:anywhere]">{explanation}</p>}
  </div>;
}
