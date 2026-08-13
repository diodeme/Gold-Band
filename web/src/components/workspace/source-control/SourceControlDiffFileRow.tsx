import type { ReactNode } from 'react';
import { cn } from '@/lib/utils';
import type { GitFileChangeKindVm } from '@/types';

export function SourceControlDiffFileRow({
  path,
  oldPath,
  kind,
  addedLines,
  deletedLines,
  onClick,
  pathDetail,
  trailing,
  className,
}: {
  path: string;
  oldPath?: string | null;
  kind: GitFileChangeKindVm;
  addedLines?: number | null;
  deletedLines?: number | null;
  onClick: () => void;
  pathDetail?: ReactNode;
  trailing?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn('group flex min-w-0 max-w-full items-center overflow-hidden px-1.5 hover:bg-muted/45', className)} data-source-control-diff-file-row="true">
      <button type="button" className="flex h-9 min-w-0 flex-1 items-center gap-2 overflow-hidden rounded-md px-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50" onClick={onClick}>
        <DiffTypeIcon kind={kind} />
        <span className="flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden text-xs">
          <span className="min-w-0 truncate">{oldPath ? `${oldPath} → ${path}` : path}</span>
          {pathDetail}
        </span>
        <DiffSummary addedLines={addedLines} deletedLines={deletedLines} />
      </button>
      {trailing}
    </div>
  );
}

export function DiffTypeIcon({ kind }: { kind: GitFileChangeKindVm }) {
  const presentation = diffTypePresentation(kind);
  return (
    <span className={cn('flex size-4 shrink-0 items-center justify-center rounded text-[10px] font-semibold', presentation.className)} data-source-control-diff-type={presentation.label}>
      {presentation.label}
    </span>
  );
}

export function DiffSummary({ addedLines, deletedLines }: { addedLines?: number | null; deletedLines?: number | null }) {
  if (addedLines == null && deletedLines == null) return null;
  return (
    <span className="flex shrink-0 items-center gap-1.5 text-[10px] tabular-nums" data-source-control-diff-summary="true">
      <span className="text-emerald-600 dark:text-emerald-400">+{addedLines ?? 0}</span>
      <span className="text-destructive">-{deletedLines ?? 0}</span>
    </span>
  );
}

export function diffTypePresentation(kind: GitFileChangeKindVm) {
  if (kind === 'added' || kind === 'untracked') {
    return { label: 'A', className: 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400' } as const;
  }
  if (kind === 'deleted') {
    return { label: 'D', className: 'bg-destructive/15 text-destructive' } as const;
  }
  return { label: 'M', className: 'bg-blue-500/15 text-blue-600 dark:text-blue-400' } as const;
}
