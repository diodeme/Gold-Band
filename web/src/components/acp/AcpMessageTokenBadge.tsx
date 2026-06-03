import { useTranslation } from 'react-i18next';

export interface AcpMessageTokenBadgeProps {
  /** Token count read from event.raw._goldBand.tokens */
  tokens?: number | null;
}

export function AcpMessageTokenBadge({ tokens }: AcpMessageTokenBadgeProps) {
  const { t } = useTranslation();
  if (tokens == null || tokens <= 0) return null;
  return (
    <span className="text-[11px] text-muted-foreground/50 select-none mt-1 inline-block">
      {t('acp.messageTokens', { tokens: tokens.toLocaleString() })}
    </span>
  );
}
