import { FolderX } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import type { WorkspaceTabRoot } from '@/lib/workspace-root';

/** Shown by a workspace tab while the session's worktree cannot be served. */
export function WorkspaceRootUnavailable({ root, onBrowseMain }: {
  root: Extract<WorkspaceTabRoot, { kind: 'unavailable' }>;
  onBrowseMain: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex h-full min-h-56 flex-1 items-center justify-center px-6 text-center" data-workspace-root-unavailable={root.reason}>
      <div className="max-w-80 text-muted-foreground">
        <FolderX className="mx-auto mb-3 size-8 stroke-[1.4]" />
        <p className="text-sm font-medium text-foreground">{t(`workspace.rootUnavailable.${root.reason}`)}</p>
        {root.workspacePath ? <p className="mt-1 break-all font-mono text-xs leading-5">{root.workspacePath}</p> : null}
        <Button size="sm" variant="outline" className="mt-4" onClick={onBrowseMain}>{t('workspace.rootUnavailable.browseMain')}</Button>
      </div>
    </div>
  );
}
