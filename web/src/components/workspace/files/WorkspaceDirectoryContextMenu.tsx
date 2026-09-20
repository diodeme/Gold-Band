import { useTranslation } from 'react-i18next';
import { FilePlus2 } from 'lucide-react';
import { ContextMenuItem } from '@/components/ui/context-menu';
import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';
import type { WorkspaceDirectoryEntryVm } from '@/types';

interface WorkspaceDirectoryContextMenuProps {
  canonicalPath: string;
  relativePath: string;
  entry?: WorkspaceDirectoryEntryVm;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
  onReferenceToConversation?: (entry: WorkspaceDirectoryEntryVm) => boolean;
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

export function WorkspaceDirectoryContextMenu({
  canonicalPath,
  relativePath,
  entry,
  onCopyFailed,
  onOpenInFileManager,
  onReferenceToConversation,
}: WorkspaceDirectoryContextMenuProps) {
  const readOnly = useReadOnlyExperience();
  const { t } = useTranslation();
  const copyEntryPath = (event: Event, value: string) => {
    event.stopPropagation();
    void copyPath(value).catch(onCopyFailed);
  };
  const referenceAction = entry && onReferenceToConversation
    && canReferenceWorkspaceFileToConversation(entry.kind, true, readOnly)
    ? { entry, onReferenceToConversation }
    : null;
  return <>
    <ContextMenuItem className="h-8 px-2 py-1 text-xs" onSelect={(event) => copyEntryPath(event, copyableAbsolutePath(canonicalPath))}>{t('workspace.filesPanel.copyAbsolutePath')}</ContextMenuItem>
    <ContextMenuItem className="h-8 px-2 py-1 text-xs" onSelect={(event) => copyEntryPath(event, copyableRelativePath(relativePath))}>{t('workspace.filesPanel.copyRelativePath')}</ContextMenuItem>
    {!readOnly && <ContextMenuItem className="h-8 px-2 py-1 text-xs" onSelect={(event) => { event.stopPropagation(); onOpenInFileManager(relativePath); }}>{t('workspace.filesPanel.openInFileManager')}</ContextMenuItem>}
    {referenceAction ? (
      <ContextMenuItem
        className="h-8 gap-2 px-2 py-1 text-xs"
        onSelect={(event) => {
          event.stopPropagation();
          referenceAction.onReferenceToConversation(referenceAction.entry);
        }}
      >
        <FilePlus2 className="size-3.5" />
        {t('workspace.filesPanel.referenceToConversation')}
      </ContextMenuItem>
    ) : null}
  </>;
}

export function canReferenceWorkspaceFileToConversation(
  kind: string | undefined,
  hasComposerCommand: boolean,
  readOnly: boolean,
) {
  return !readOnly && kind === 'file' && hasComposerCommand;
}
