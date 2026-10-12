import { useTranslation } from 'react-i18next';
import { FilePlus, FilePlus2, FolderPlus, PencilLine, Trash2 } from 'lucide-react';
import { ContextMenuItem, ContextMenuSeparator } from '@/components/ui/context-menu';
import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';
import type { AddWorkspaceFileRefResult } from '../workspace-file-reference-bridge';
import type { WorkspaceDirectoryEntryVm } from '@/types';

interface WorkspaceDirectoryContextMenuProps {
  canonicalPath: string;
  relativePath: string;
  entry?: WorkspaceDirectoryEntryVm;
  canReferenceToConversation?: boolean;
  onCopyFailed: () => void;
  onOpenInFileManager: (relativePath: string) => void;
  onReferenceToConversation?: (entry: WorkspaceDirectoryEntryVm) => AddWorkspaceFileRefResult;
  /** Present only where the entry can be edited in place (the workspace tree). */
  entryActions?: WorkspaceEntryMenuActions;
}

export interface WorkspaceEntryMenuActions {
  onCreate: (entry: WorkspaceDirectoryEntryVm, kind: 'file' | 'directory') => void;
  onRename: (entry: WorkspaceDirectoryEntryVm) => void;
  onDelete: (entry: WorkspaceDirectoryEntryVm) => void;
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
  canReferenceToConversation = false,
  onCopyFailed,
  onOpenInFileManager,
  onReferenceToConversation,
  entryActions,
}: WorkspaceDirectoryContextMenuProps) {
  const readOnly = useReadOnlyExperience();
  const { t } = useTranslation();
  const copyEntryPath = (event: Event, value: string) => {
    event.stopPropagation();
    void copyPath(value).catch(onCopyFailed);
  };
  const referenceAction = entry && onReferenceToConversation
    && canReferenceWorkspaceFileToConversation(entry.kind, canReferenceToConversation, readOnly)
    ? { entry, onReferenceToConversation }
    : null;
  const editableEntry = entry && entryActions && !readOnly ? { entry, actions: entryActions } : null;
  return <>
    {editableEntry ? <>
      <ContextMenuItem className="h-8 gap-2 px-2 py-1 text-xs" onSelect={() => editableEntry.actions.onCreate(editableEntry.entry, 'file')}>
        <FilePlus className="size-3.5" />
        {t('workspace.filesPanel.newFile')}
      </ContextMenuItem>
      <ContextMenuItem className="h-8 gap-2 px-2 py-1 text-xs" onSelect={() => editableEntry.actions.onCreate(editableEntry.entry, 'directory')}>
        <FolderPlus className="size-3.5" />
        {t('workspace.filesPanel.newFolder')}
      </ContextMenuItem>
      <ContextMenuSeparator />
    </> : null}
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
    {editableEntry ? <>
      <ContextMenuSeparator />
      <ContextMenuItem className="h-8 gap-2 px-2 py-1 text-xs" onSelect={() => editableEntry.actions.onRename(editableEntry.entry)}>
        <PencilLine className="size-3.5" />
        {t('workspace.filesPanel.rename')}
      </ContextMenuItem>
      <ContextMenuItem variant="destructive" className="h-8 gap-2 px-2 py-1 text-xs" onSelect={() => editableEntry.actions.onDelete(editableEntry.entry)}>
        <Trash2 className="size-3.5" />
        {t('workspace.filesPanel.delete')}
      </ContextMenuItem>
    </> : null}
  </>;
}

export function canReferenceWorkspaceFileToConversation(
  kind: string | undefined,
  hasComposerCommand: boolean,
  readOnly: boolean,
) {
  return !readOnly && kind === 'file' && hasComposerCommand;
}
