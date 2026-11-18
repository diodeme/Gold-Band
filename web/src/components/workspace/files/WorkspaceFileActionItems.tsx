import { useTranslation } from 'react-i18next';
import { FilePlus2 } from 'lucide-react';
import { ContextMenuItem } from '@/components/ui/context-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

export function copyableAbsolutePath(path: string) {
  if (path.startsWith('\\\\?\\UNC\\')) return `\\\\${path.slice(8)}`;
  return path.startsWith('\\\\?\\') ? path.slice(4) : path;
}

export function copyableRelativePath(path: string) { return path.replaceAll('\\', '/'); }

/** Shared read-only actions; resource owners decide capabilities and authorization. */
export function WorkspaceFileActionItems({ absolutePath, relativePath, relativeBaseLabel, reveal, reference, onCopyFailed }: {
  absolutePath: string | null;
  relativePath: string | null;
  relativeBaseLabel?: string;
  reveal?: { disabled: boolean; run: () => void };
  reference?: { disabled: boolean; run: () => void };
  onCopyFailed: () => void;
}) {
  const { t } = useTranslation();
  const copy = (event: Event, value: string) => {
    event.stopPropagation();
    void (async () => {
      if (!navigator.clipboard) throw new Error('clipboard-unavailable');
      await navigator.clipboard.writeText(value);
    })().catch(onCopyFailed);
  };
  const relativeItem = <ContextMenuItem className="h-8 px-2 py-1 text-xs" disabled={relativePath === null}
    onSelect={e => relativePath !== null && copy(e, copyableRelativePath(relativePath))}>
    {t('workspace.filesPanel.copyRelativePath')}
  </ContextMenuItem>;
  return <>
    <ContextMenuItem className="h-8 px-2 py-1 text-xs" disabled={absolutePath === null}
      onSelect={e => absolutePath !== null && copy(e, copyableAbsolutePath(absolutePath))}>
      {t('workspace.filesPanel.copyAbsolutePath')}
    </ContextMenuItem>
    {relativeBaseLabel ? <Tooltip><TooltipTrigger asChild>{relativeItem}</TooltipTrigger>
      <TooltipContent side="right">{relativeBaseLabel}</TooltipContent></Tooltip> : relativeItem}
    {reveal && <ContextMenuItem className="h-8 px-2 py-1 text-xs" disabled={reveal.disabled}
      onSelect={e => { e.stopPropagation(); reveal.run(); }}>{t('workspace.filesPanel.openInFileManager')}</ContextMenuItem>}
    {reference && <ContextMenuItem className="h-8 gap-2 px-2 py-1 text-xs" disabled={reference.disabled}
      onSelect={e => { e.stopPropagation(); reference.run(); }}>
      <FilePlus2 className="size-3.5" />{t('workspace.filesPanel.referenceToConversation')}
    </ContextMenuItem>}
  </>;
}
