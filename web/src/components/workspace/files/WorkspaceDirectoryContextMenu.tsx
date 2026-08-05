import { useTranslation } from 'react-i18next';
import { ContextMenuItem } from '@/components/ui/context-menu';

interface WorkspaceDirectoryContextMenuProps {
  canonicalPath: string;
  relativePath: string;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
}

async function copyPath(value: string) {
  if (!navigator.clipboard) throw new Error('clipboard-unavailable');
  await navigator.clipboard.writeText(value);
}

export function copyableAbsolutePath(path: string) {
  if (path.startsWith('\\\\?\\UNC\\')) return `\\\\${path.slice(8)}`;
  return path.startsWith('\\\\?\\') ? path.slice(4) : path;
}

export function copyableRelativePath(path: string) {
  return path.replaceAll('\\', '/');
}

export function WorkspaceDirectoryContextMenu({ canonicalPath, relativePath, onCopyFailed, onOpenInFileManager }: WorkspaceDirectoryContextMenuProps) {
  const { t } = useTranslation();
  const copyEntryPath = (event: Event, value: string) => {
    event.stopPropagation();
    void copyPath(value).catch(onCopyFailed);
  };
  return <>
    <ContextMenuItem className="h-8 px-2 py-1 text-xs" onSelect={(event) => copyEntryPath(event, copyableAbsolutePath(canonicalPath))}>{t('workspace.filesPanel.copyAbsolutePath')}</ContextMenuItem>
    <ContextMenuItem className="h-8 px-2 py-1 text-xs" onSelect={(event) => copyEntryPath(event, copyableRelativePath(relativePath))}>{t('workspace.filesPanel.copyRelativePath')}</ContextMenuItem>
    <ContextMenuItem className="h-8 px-2 py-1 text-xs" onSelect={(event) => { event.stopPropagation(); onOpenInFileManager(relativePath); }}>{t('workspace.filesPanel.openInFileManager')}</ContextMenuItem>
  </>;
}
