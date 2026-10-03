import { useState } from 'react';
import { Loader2, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { revokeRemoteImageHost, useRemoteImageTrust } from '@/lib/remote-image-trust-store';

/** Hosts beyond this count fold behind "show all" so a long list never stretches the settings page. */
export const TRUSTED_HOST_PREVIEW_LIMIT = 20;

/** Hosts are trusted from the image itself (chat or file preview); this list only reviews and removes them. */
export function RemoteImageTrustSettings() {
  const { t } = useTranslation();
  const { trustedHosts } = useRemoteImageTrust();
  const [pendingHost, setPendingHost] = useState<string | null>(null);
  const [failedHost, setFailedHost] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const foldable = trustedHosts.length > TRUSTED_HOST_PREVIEW_LIMIT;
  const visibleHosts = foldable && !expanded ? trustedHosts.slice(0, TRUSTED_HOST_PREVIEW_LIMIT) : trustedHosts;
  const showAllLabel = t('settings.remoteImages.showAll', { count: trustedHosts.length });

  const remove = async (host: string) => {
    setPendingHost(host);
    setFailedHost(null);
    try {
      await revokeRemoteImageHost(host);
    } catch {
      setFailedHost(host);
    } finally {
      setPendingHost(null);
    }
  };

  if (trustedHosts.length === 0) {
    return <p className="text-sm text-muted-foreground">{t('settings.remoteImages.empty')}</p>;
  }
  return (
    <div className="flex max-w-3xl flex-col gap-2">
      {/* Trust grows one host per click, so wrapped chips stay a few rows and need no inner scroll. */}
      <ul className="flex flex-wrap gap-2" data-remote-image-trusted-hosts="true">
        {visibleHosts.map((host) => {
          const label = t('settings.remoteImages.remove', { host });
          return (
            <li key={host} className="contents">
              <Badge
                variant="secondary"
                aria-invalid={failedHost === host || undefined}
                className="h-7 max-w-full gap-0.5 py-0 pr-0.5 pl-2.5 font-mono font-normal"
              >
                <span className="truncate">{host}</span>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="size-6 shrink-0 rounded-full"
                      aria-label={label}
                      disabled={pendingHost !== null}
                      onClick={() => void remove(host)}
                    >
                      {pendingHost === host ? <Loader2 className="size-3.5 animate-spin" /> : <X className="size-3.5" />}
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="top">{label}</TooltipContent>
                </Tooltip>
              </Badge>
            </li>
          );
        })}
        {foldable ? (
          <li className="contents">
            {expanded ? (
              <Button
                size="sm"
                variant="secondary"
                className="h-7 rounded-full px-2.5 text-xs font-normal text-muted-foreground"
                aria-expanded
                onClick={() => setExpanded(false)}
              >
                {t('settings.remoteImages.showLess')}
              </Button>
            ) : (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    size="sm"
                    variant="secondary"
                    className="h-7 rounded-full px-2.5 font-mono text-xs font-normal text-muted-foreground"
                    aria-expanded={false}
                    aria-label={showAllLabel}
                    onClick={() => setExpanded(true)}
                  >
                    {t('settings.remoteImages.hiddenCount', { count: trustedHosts.length - TRUSTED_HOST_PREVIEW_LIMIT })}
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="top">{showAllLabel}</TooltipContent>
              </Tooltip>
            )}
          </li>
        ) : null}
      </ul>
      {failedHost ? <p className="text-xs text-destructive">{t('settings.remoteImages.removeFailed')}</p> : null}
    </div>
  );
}
