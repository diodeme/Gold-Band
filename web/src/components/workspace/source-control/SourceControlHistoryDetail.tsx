import { ArrowLeft, ChevronRight, GitCompareArrows, LoaderCircle } from 'lucide-react';
import type { TFunction } from 'i18next';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ScrollArea } from '@/components/ui/scroll-area';
import type {
  GitCommitDetailVm,
  GitCommitFileChangeVm,
  GitCommitRelationsVm,
} from '@/types';

interface HistoryDetailProps {
  kind: 'commit' | 'relations';
  detail?: GitCommitDetailVm | null;
  relations?: GitCommitRelationsVm | null;
  loading: boolean;
  t: TFunction;
  onBack: () => void;
  onOpenFile: (change: GitCommitFileChangeVm, beforeOid: string | null, afterOid: string) => void;
}

export function SourceControlHistoryDetail({
  kind,
  detail,
  relations,
  loading,
  t,
  onBack,
  onOpenFile,
}: HistoryDetailProps) {
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-source-control-history-detail="true">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border/50 px-2">
        <Button size="xs" variant="ghost" onClick={onBack}>
          <ArrowLeft className="size-3" />{t('common.back')}
        </Button>
        <span className="min-w-0 flex-1 truncate text-xs font-medium">
          {kind === 'relations' ? t('sourceControl.relationAnalysis') : t('sourceControl.commitDetail')}
        </span>
      </div>
      {loading ? (
        <div className="flex min-h-0 flex-1 items-center justify-center gap-2 text-xs text-muted-foreground">
          <LoaderCircle className="size-3.5 animate-spin" />{t('sourceControl.loadingCommitData')}
        </div>
      ) : null}
      {!loading && detail ? (
        <CommitDetail detail={detail} t={t} onOpenFile={onOpenFile} />
      ) : null}
      {!loading && relations ? (
        <CommitRelationsDetail relations={relations} t={t} onOpenFile={onOpenFile} />
      ) : null}
    </div>
  );
}

function CommitDetail({
  detail,
  t,
  onOpenFile,
}: {
  detail: GitCommitDetailVm;
  t: TFunction;
  onOpenFile: HistoryDetailProps['onOpenFile'];
}) {
  const { commit } = detail;
  const beforeOid = commit.parentOids[0] ?? null;
  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="space-y-3 px-3 py-3">
        <div>
          <h3 className="text-sm font-semibold leading-snug">{commit.subject}</h3>
          <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] text-muted-foreground">
            <span>{commit.author.name}</span>
            <span className="font-mono">{commit.oid}</span>
            <span>{formatCommitTime(commit.committer.timestamp)}</span>
          </div>
          {commit.refs.length > 0 ? (
            <div className="mt-2 flex flex-wrap gap-1">
              {commit.refs.map((ref) => <Badge key={ref.fullName} variant="secondary" className="h-5 px-1.5 text-[9px]">{ref.shortName}</Badge>)}
            </div>
          ) : null}
        </div>
        {commit.body.trim() ? <pre className="whitespace-pre-wrap border-t border-border/45 pt-3 font-sans text-xs leading-relaxed text-foreground/90">{commit.body.trim()}</pre> : null}
        <div className="border-t border-border/45 pt-2">
          <div className="mb-1 text-[11px] font-medium text-muted-foreground">{t('sourceControl.changedFiles', { count: detail.files.length })}</div>
          <CommitFileList
            files={detail.files}
            onOpen={(change) => onOpenFile(change, beforeOid, commit.oid)}
          />
        </div>
      </div>
    </ScrollArea>
  );
}

function CommitRelationsDetail({
  relations,
  t,
  onOpenFile,
}: {
  relations: GitCommitRelationsVm;
  t: TFunction;
  onOpenFile: HistoryDetailProps['onOpenFile'];
}) {
  const beforeOid = relations.selectedOids[0] ?? '';
  const afterOid = relations.selectedOids[1] ?? '';
  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="space-y-3 px-3 py-3 text-xs">
        <div className="flex min-w-0 items-center gap-2">
          <span className="text-muted-foreground">{t('sourceControl.targetRef')}</span>
          <Badge variant="secondary" className="max-w-44 truncate">{relations.targetRef}</Badge>
          <span className="min-w-0 truncate font-mono text-[10px] text-muted-foreground">{relations.targetOid.slice(0, 8)}</span>
        </div>
        <section>
          <div className="mb-1 text-[11px] font-medium text-muted-foreground">{t('sourceControl.commonMergeBase')}</div>
          <div className="flex flex-wrap gap-1">
            {relations.commonMergeBases.length > 0
              ? relations.commonMergeBases.map((oid) => <Badge key={oid} variant="outline" className="font-mono text-[9px]">{oid.slice(0, 8)}</Badge>)
              : <span className="text-[11px] text-muted-foreground">{t('sourceControl.noCommonMergeBase')}</span>}
          </div>
        </section>
        <section className="border-t border-border/45 pt-2">
          <div className="mb-1 text-[11px] font-medium text-muted-foreground">{t('sourceControl.pairwiseRelations')}</div>
          <div className="divide-y divide-border/35">
            {relations.pairwise.map((pair) => (
              <div key={`${pair.leftOid}:${pair.rightOid}`} className="py-2">
                <div className="flex items-center gap-1 font-mono text-[10px]">
                  <span>{pair.leftOid.slice(0, 8)}</span>
                  <span className="font-sans text-muted-foreground">{t(`sourceControl.relationKinds.${pair.relation}`)}</span>
                  <span>{pair.rightOid.slice(0, 8)}</span>
                </div>
                <div className="mt-0.5 text-[10px] text-muted-foreground">
                  {t('sourceControl.leftRightCounts', { left: pair.leftOnlyCount, right: pair.rightOnlyCount })}
                </div>
              </div>
            ))}
          </div>
        </section>
        <section className="border-t border-border/45 pt-2">
          <div className="mb-1 text-[11px] font-medium text-muted-foreground">{t('sourceControl.targetContainment')}</div>
          <div className="divide-y divide-border/35">
            {relations.mergeEntries.map((entry) => (
              <div key={entry.oid} className="flex min-w-0 items-center gap-2 py-1.5 text-[10px]">
                <span className="font-mono">{entry.oid.slice(0, 8)}</span>
                <span className="min-w-0 flex-1 text-muted-foreground">{t(`sourceControl.mergeEntryStatus.${entry.status}`)}</span>
                {entry.firstMergeOid ? <span className="font-mono text-muted-foreground">{entry.firstMergeOid.slice(0, 8)}</span> : null}
              </div>
            ))}
          </div>
        </section>
        {relations.selectedOids.length === 2 ? (
          <section className="border-t border-border/45 pt-2">
            <div className="mb-1 flex items-center gap-1 text-[11px] font-medium text-muted-foreground">
              <GitCompareArrows className="size-3" />{t('sourceControl.twoPointDiff', { count: relations.comparisonFiles.length })}
            </div>
            <CommitFileList
              files={relations.comparisonFiles}
              onOpen={(change) => onOpenFile(change, beforeOid, afterOid)}
            />
          </section>
        ) : null}
      </div>
    </ScrollArea>
  );
}

function CommitFileList({ files, onOpen }: { files: GitCommitFileChangeVm[]; onOpen: (change: GitCommitFileChangeVm) => void }) {
  return (
    <div className="divide-y divide-border/35">
      {files.map((change) => (
        <button key={`${change.oldPath ?? ''}:${change.path}`} type="button" className="flex h-8 w-full min-w-0 items-center gap-2 text-left hover:bg-muted/35" onClick={() => onOpen(change)}>
          <span className="flex size-4 shrink-0 items-center justify-center rounded bg-muted text-[9px] font-semibold text-muted-foreground">{changeStatusLabel(change.kind)}</span>
          <span className="min-w-0 flex-1 truncate text-[11px]">{change.path}</span>
          {change.addedLines != null ? <span className="text-[9px] tabular-nums text-emerald-600">+{change.addedLines}</span> : null}
          {change.deletedLines != null ? <span className="text-[9px] tabular-nums text-destructive">-{change.deletedLines}</span> : null}
          <ChevronRight className="size-3 shrink-0 text-muted-foreground" />
        </button>
      ))}
    </div>
  );
}

function changeStatusLabel(kind: GitCommitFileChangeVm['kind']) {
  if (kind === 'added') return 'A';
  if (kind === 'deleted') return 'D';
  if (kind === 'renamed') return 'R';
  if (kind === 'copied') return 'C';
  if (kind === 'type-changed') return 'T';
  return 'M';
}

function formatCommitTime(timestamp: string) {
  const date = new Date(timestamp);
  return Number.isNaN(date.getTime()) ? timestamp : date.toLocaleString();
}
