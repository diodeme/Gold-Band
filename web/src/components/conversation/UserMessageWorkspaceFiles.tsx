import { SquareCode } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import type { WorkspaceFileTimelineRef } from '@/lib/composer-context';

export function UserMessageWorkspaceFiles({
  files,
  onOpen,
}: {
  files: readonly WorkspaceFileTimelineRef[];
  onOpen: (file: WorkspaceFileTimelineRef) => void;
}) {
  const { t } = useTranslation();
  if (files.length === 0) return null;
  return (
    <div
      className="flex max-w-full flex-wrap justify-end gap-1.5"
      data-user-workspace-files="true"
    >
      {files.map((file) => (
        <Button
          key={`${file.projectId}:${file.canonicalPath ?? file.relativePath}`}
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 max-w-56 gap-1.5 rounded-lg px-2 text-xs font-medium"
          onClick={() => onOpen(file)}
          aria-label={file.name ?? file.relativePath}
        >
          <SquareCode className="size-3.5 shrink-0" />
          <span className="truncate">{file.name ?? file.relativePath.split('/').at(-1)}</span>
          <span className="sr-only">{t('workspace.filesPanel.openWorkspaceFile')}</span>
        </Button>
      ))}
    </div>
  );
}
