import { useState } from 'react';
import { ExternalLink } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { openFileWithSystemApp } from '@/api';
import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { FileWorkspaceResource } from '../right-workspace-context';
import { fileContentStore } from './file-content-store';

const SYSTEM_OPEN_FAILED = 'workspace-file.system-open-failed';

export function OpenWithSystemAppButton({
  resource,
  variant,
}: {
  resource: FileWorkspaceResource;
  variant: 'icon' | 'button';
}) {
  const readOnly = useReadOnlyExperience();
  const { t } = useTranslation();
  const [pending, setPending] = useState(false);
  const [errorCode, setErrorCode] = useState<string | null>(null);
  if (readOnly) return null;
  const label = t('workspace.filesPanel.openWithSystem');
  const open = async () => {
    setPending(true);
    setErrorCode(null);
    try {
      await openFileWithSystemApp({
        projectId: resource.projectId,
        canonicalPath: resource.locator.canonicalPath,
        externalAccessToken: fileContentStore.externalAccessToken(resource.key),
      });
    } catch (reason) {
      const code = (reason as { code?: unknown } | null)?.code;
      setErrorCode(typeof code === 'string' ? code : SYSTEM_OPEN_FAILED);
    } finally {
      setPending(false);
    }
  };
  const error = errorCode ? (
    <span role="alert" className="text-xs text-destructive" data-open-with-system-app-error={errorCode}>
      {t(`workspace.filesPanel.errors.${errorCode}`, { defaultValue: t(`workspace.filesPanel.errors.${SYSTEM_OPEN_FAILED}`) })}
    </span>
  ) : null;
  if (variant === 'button') {
    return (
      <>
        <Button size="sm" variant="outline" disabled={pending} onClick={() => void open()} data-open-with-system-app="true">
          <ExternalLink className="size-3.5" />{label}
        </Button>
        {error ? <p className="mt-2">{error}</p> : null}
      </>
    );
  }
  return (
    <>
      {error ? <span className="min-w-0 truncate">{error}</span> : null}
      <Tooltip>
        <TooltipTrigger asChild>
          <Button size="icon" variant="ghost" className="size-7" disabled={pending} onClick={() => void open()} aria-label={label} data-open-with-system-app="true">
            <ExternalLink className="size-3.5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>{label}</TooltipContent>
      </Tooltip>
    </>
  );
}
