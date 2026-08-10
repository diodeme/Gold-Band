import { useMemo, type KeyboardEvent } from 'react';
import { GitLog } from '@tomplum/react-git-log';
import { GitMerge } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import { cn } from '@/lib/utils';
import type { CommitGraphEntry } from './commit-graph-model';

const COMMIT_GRAPH_PAGE_SIZE = 300;
const COMMIT_GRAPH_ROW_HEIGHT = 40;
const COMMIT_GRAPH_COLOURS = [
  '#d6b65c',
  '#5aa9e6',
  '#63c29d',
  '#c993d4',
  '#e58b6b',
  '#8fa6d8',
];

interface CommitGraphProps {
  entries: CommitGraphEntry[];
  currentBranch?: string | null;
  page?: number;
  pageSize?: number;
  selectedOids: ReadonlySet<string>;
  focusedOid?: string | null;
  runtimeLabel: string;
  selectLabel: (entry: CommitGraphEntry) => string;
  formatTimestamp: (timestamp: string) => string;
  onToggleSelected: (oid: string) => void;
  onOpenCommit: (oid: string) => void;
}

export function CommitGraph({
  entries,
  currentBranch,
  page = 0,
  pageSize = COMMIT_GRAPH_PAGE_SIZE,
  selectedOids,
  focusedOid,
  runtimeLabel,
  selectLabel,
  formatTimestamp,
  onToggleSelected,
  onOpenCommit,
}: CommitGraphProps) {
  const entryByHash = useMemo(
    () => new Map(entries.map((entry) => [entry.hash, entry])),
    [entries],
  );
  const libraryEntries = useMemo(() => entries.map((entry) => ({
    ...entry,
    author: entry.author ? {
      name: entry.author.name,
      email: entry.author.email ?? undefined,
    } : undefined,
  })), [entries]);
  const activeBranch = currentBranch?.trim() || 'HEAD';
  const theme = typeof document !== 'undefined' && document.documentElement.classList.contains('dark')
    ? 'dark'
    : 'light';

  return (
    <div className="min-w-0 overflow-x-auto" data-commit-graph="html-grid">
      <GitLog
        entries={libraryEntries}
        currentBranch={activeBranch}
        theme={theme}
        colours={COMMIT_GRAPH_COLOURS}
        showHeaders={false}
        showGitIndex={false}
        rowSpacing={0}
        defaultGraphWidth={56}
        paging={{ page, size: pageSize }}
        enableSelectedCommitStyling={false}
        enablePreviewedCommitStyling={false}
        onSelectCommit={(commit) => {
          if (commit) onOpenCommit(commit.hash);
        }}
        classes={{
          containerClass: 'min-w-0 [&>div:last-child]:min-w-0',
          containerStyles: { minHeight: 0 },
        }}
      >
        <GitLog.GraphHTMLGrid
          nodeSize={10}
          showCommitNodeHashes={false}
          showCommitNodeTooltips={false}
          highlightedBackgroundHeight={COMMIT_GRAPH_ROW_HEIGHT}
        />
        <GitLog.Table
          className="min-w-0"
          row={({ commit }) => {
            const entry = entryByHash.get(commit.hash);
            if (!entry) return <div className="h-10" />;
            return (
              <CommitGraphRow
                entry={entry}
                selected={selectedOids.has(entry.hash)}
                focused={focusedOid === entry.hash}
                runtimeLabel={runtimeLabel}
                selectLabel={selectLabel(entry)}
                formattedTimestamp={formatTimestamp(entry.committerDate)}
                onToggleSelected={onToggleSelected}
                onOpenCommit={onOpenCommit}
              />
            );
          }}
        />
      </GitLog>
    </div>
  );
}

function CommitGraphRow({
  entry,
  selected,
  focused,
  runtimeLabel,
  selectLabel,
  formattedTimestamp,
  onToggleSelected,
  onOpenCommit,
}: {
  entry: CommitGraphEntry;
  selected: boolean;
  focused: boolean;
  runtimeLabel: string;
  selectLabel: string;
  formattedTimestamp: string;
  onToggleSelected: (oid: string) => void;
  onOpenCommit: (oid: string) => void;
}) {
  const openFromKeyboard = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'Enter' && event.key !== ' ') return;
    event.preventDefault();
    onOpenCommit(entry.hash);
  };
  const visibleRefs = entry.refs.slice(0, 2);
  const hiddenRefCount = entry.refs.length - visibleRefs.length;

  return (
    <div
      role="button"
      tabIndex={0}
      className={cn(
        'group flex h-10 min-w-0 items-center gap-2 border-b border-border/35 px-1.5 text-left outline-none transition-colors hover:bg-muted/40 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/50',
        selected && 'bg-primary/8',
        focused && 'bg-muted/60',
      )}
      data-commit-graph-row={entry.hash}
      data-selected={selected ? 'true' : 'false'}
      onClick={() => onOpenCommit(entry.hash)}
      onKeyDown={openFromKeyboard}
    >
      <Checkbox
        checked={selected}
        aria-label={selectLabel}
        className="size-3.5"
        onClick={(event) => event.stopPropagation()}
        onCheckedChange={() => onToggleSelected(entry.hash)}
      />
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-center gap-1">
          {entry.parents.length > 1 ? <GitMerge className="size-3 shrink-0 text-muted-foreground" /> : null}
          <span className="min-w-0 flex-1 truncate text-xs font-medium">{entry.message}</span>
          {entry.runtimeCheckpoint ? <Badge variant="outline" className="h-4 px-1 text-[9px]">{runtimeLabel}</Badge> : null}
          {visibleRefs.map((ref) => (
            <Badge key={ref.fullName} variant="secondary" className="hidden h-4 max-w-24 px-1 text-[9px] sm:inline-flex">
              <span className="truncate">{ref.shortName}</span>
            </Badge>
          ))}
          {hiddenRefCount > 0 ? <span className="hidden text-[9px] text-muted-foreground sm:inline">+{hiddenRefCount}</span> : null}
        </span>
        <span className="mt-0.5 flex min-w-0 items-center gap-2 text-[10px] text-muted-foreground">
          <span className="max-w-24 truncate">{entry.author?.name}</span>
          <span className="font-mono">{entry.hash.slice(0, 8)}</span>
          <span className="truncate">{formattedTimestamp}</span>
        </span>
      </span>
    </div>
  );
}

export const commitGraphPageSize = COMMIT_GRAPH_PAGE_SIZE;
