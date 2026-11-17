import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';
import type { ConversationTurnIndexEntryVm } from '@/types';

// Semantic tokens that stay visible on both light and dark schemes. The
// accent token is a hover-surface color and runs nearly invisible on dark
// canvases, so compaction marks ride the link token instead.
const TURN_KIND_DASH_CLASS: Record<ConversationTurnIndexEntryVm['kind'], string> = {
  prompt: 'bg-primary',
  tool: 'bg-muted-foreground/60',
  compaction: 'bg-link',
  permission: 'bg-gold-warning',
};

export interface ConversationTurnRailLocator {
  projectId?: string | null;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  branchId?: string | null;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
}

interface ConversationTurnRailProps {
  locator: ConversationTurnRailLocator;
  /**
   * Canonical timeline progress watermark (e.g. the session's newest
   * seq/covered revision). Bumping it means new committed turns may exist
   * beyond what the rail already shows, so the rail fetches only entries
   * after its newest known seq and merges them — never a full re-read.
   */
  refreshSignal: number;
  /** Jumps the conversation timeline to the turn starting at `seq`. */
  onJumpToSeq: (seq: number) => void;
}

function mergeTurnEntries(
  current: ConversationTurnIndexEntryVm[],
  incremental: ConversationTurnIndexEntryVm[],
): ConversationTurnIndexEntryVm[] {
  if (incremental.length === 0) return current;
  if (current.length === 0) return incremental;
  const bySeq = new Map(current.map((entry) => [entry.seq, entry]));
  for (const entry of incremental) bySeq.set(entry.seq, entry);
  return [...bySeq.values()].sort((a, b) => a.seq - b.seq);
}

/**
 * Left-edge session navigation rail: one tick per committed turn, colored by
 * the turn's dominant event kind, hover preview, click to jump. All ticks live
 * in one scroll list that starts pinned to the newest turn; fading caps at
 * both ends mark that older or newer ticks continue beyond the visible range.
 * Mounted by the conversation viewport outside the scroll container so it
 * stays fixed while the timeline scrolls.
 */
export function ConversationTurnRail({ locator, refreshSignal, onJumpToSeq }: ConversationTurnRailProps) {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<ConversationTurnIndexEntryVm[] | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const newestSeqRef = useRef<number | null>(null);

  const locatorKey = JSON.stringify(locator);
  useEffect(() => {
    let cancelled = false;
    const current = JSON.parse(locatorKey) as ConversationTurnRailLocator;
    newestSeqRef.current = null;
    setEntries(null);
    import('@/api/client').then(({ getRuntimeApi }) =>
      getRuntimeApi().getConversationTurnIndex(
        current.projectId ?? null,
        current.taskId,
        current.runId,
        current.roundId,
        current.nodeId,
        current.attemptId,
        current.branchId ?? null,
        null,
        current.outerNodeId ?? null,
        current.outerAttemptId ?? null,
      ),
    ).then((loaded) => {
      if (!cancelled) {
        newestSeqRef.current = loaded.length > 0
          ? loaded[loaded.length - 1].seq
          : null;
        setEntries(loaded);
      }
    }).catch(() => {
      if (!cancelled) setEntries([]);
    });
    return () => {
      cancelled = true;
    };
  }, [locatorKey]);

  // Incremental refresh: when the canonical session watermark advances, ask
  // the backend only for turns newer than the newest one already shown and
  // merge them by seq. This keeps the read proportional to new turns instead
  // of rescanning the whole projection.
  useEffect(() => {
    const newestSeq = newestSeqRef.current;
    if (newestSeq === null) return;
    let cancelled = false;
    const current = JSON.parse(locatorKey) as ConversationTurnRailLocator;
    import('@/api/client').then(({ getRuntimeApi }) =>
      getRuntimeApi().getConversationTurnIndex(
        current.projectId ?? null,
        current.taskId,
        current.runId,
        current.roundId,
        current.nodeId,
        current.attemptId,
        current.branchId ?? null,
        newestSeq,
        current.outerNodeId ?? null,
        current.outerAttemptId ?? null,
      ),
    ).then((incremental) => {
      if (cancelled || incremental.length === 0) return;
      newestSeqRef.current = incremental[incremental.length - 1].seq;
      setEntries((previous) => mergeTurnEntries(previous ?? [], incremental));
    }).catch(() => {
      // The rail keeps its current projection; the next watermark bump retries.
    });
    return () => {
      cancelled = true;
    };
    // refreshSignal is the only trigger: the locator effect above handles
    // identity resets (newestSeqRef.current is null until the initial load
    // finishes, so this never races the first fetch).
  }, [locatorKey, refreshSignal]);

  // The list opens at the newest turns; older ones scroll in from above.
  useEffect(() => {
    const scroller = scrollRef.current;
    if (scroller && entries !== null) scroller.scrollTop = scroller.scrollHeight;
  }, [entries]);

  if (entries === null || entries.length === 0) return null;

  return (
    <TooltipProvider delayDuration={200}>
    <nav
      aria-label={t('acp.turnRail.label')}
      data-acp-conversation-rail="turn-rail"
      // Always visible; vertically centered and capping at 70% of the viewport
      // with internal scrolling beyond that.
      className="absolute inset-y-0 left-0 z-10 flex w-6 items-center pl-1"
    >
      <div
        ref={scrollRef}
        // Fade caps replace chapter lines and expand controls: content fading
        // out at either end means "keep scrolling, there is more".
        className="flex max-h-[70%] w-4 flex-none flex-col items-center gap-2 overflow-y-auto py-3 [mask-image:linear-gradient(to_bottom,transparent,black_12px,black_calc(100%-12px),transparent)] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
      {entries.map((turn) => (
        <Tooltip key={turn.seq}>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="group flex h-5 w-4 flex-none items-center justify-center rounded outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label={t('acp.turnRail.jump', { preview: turn.promptPreview ?? turn.timestamp.slice(0, 10) })}
              data-turn-seq={turn.seq}
              onClick={() => onJumpToSeq(turn.seq)}
            >
              {/* A vertical tick reads as a ruler mark; taller and brighter on
                  hover, never morphing shape. */}
              <span
                className={cn(
                  'h-2.5 w-0.5 rounded-full transition-all group-hover:h-4 group-hover:w-1',
                  TURN_KIND_DASH_CLASS[turn.kind],
                )}
              />
            </button>
          </TooltipTrigger>
          <TooltipContent side="right" className="max-w-72">
            <span className="block text-xs text-muted-foreground">
              {t('acp.turnRail.kind.' + turn.kind)} · {new Date(turn.timestamp).toLocaleString()}
            </span>
            {turn.promptPreview ? <span className="block line-clamp-3 text-xs">{turn.promptPreview}</span> : null}
          </TooltipContent>
        </Tooltip>
      ))}
      </div>
    </nav>
    </TooltipProvider>
  );
}
