import { TURN_RAIL_PAGE_SIZE, TURN_RAIL_WINDOW_SIZE } from '@/lib/conversation-turn-index';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ChevronUp, ChevronDown, RotateCw } from 'lucide-react';
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

type LoadDirection = 'initial' | 'older' | 'newer' | 'refresh';

export function ConversationTurnRail(props: ConversationTurnRailProps) {
  return <ScopedTurnRail key={JSON.stringify(props.locator)} {...props} />;
}

function ScopedTurnRail({ locator, refreshSignal, onJumpToSeq }: ConversationTurnRailProps) {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<ConversationTurnIndexEntryVm[]>([]);
  const [status, setStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [pages, setPages] = useState({ older: false, newer: false });
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const anchorRef = useRef<{ seq: string; top: number } | 'bottom' | null>(null);
  const loadRef = useRef<(direction: LoadDirection) => void>(() => {});
  const previousSignal = useRef(refreshSignal);
  const locatorKey = JSON.stringify(locator);

  useEffect(() => {
    let cancelled = false;
    let loading = false;
    let refreshQueued = false;
    let loaded = false;
    let current: ConversationTurnIndexEntryVm[] = [];
    let older = false;
    let newer = false;
    const scope = JSON.parse(locatorKey) as ConversationTurnRailLocator;
    const load = async (direction: LoadDirection) => {
      if (cancelled) return;
      if (loading) { if (direction === 'refresh') refreshQueued = true; return; }
      if (direction === 'refresh' && newer) return;
      loading = true;
      setStatus('loading');
      const initial = !loaded || direction === 'initial';
      const before = direction === 'older' ? current[0]?.seq ?? null : null;
      // Re-read the newest turn too: a tool or permission can upgrade its kind.
      const after = !initial && before === null ? Math.max(0, (current.at(-1)?.seq ?? 1) - 1) : null;
      try {
        const { getRuntimeApi } = await import('@/api/client');
        const page = await getRuntimeApi().getConversationTurnIndex(scope.projectId ?? null, scope.taskId, scope.runId, scope.roundId, scope.nodeId, scope.attemptId, scope.branchId ?? null, after, scope.outerNodeId ?? null, scope.outerAttemptId ?? null, before);
        if (cancelled) return;
        const scroller = scrollRef.current;
        if (initial) anchorRef.current = 'bottom';
        else if (scroller) {
          const top = scroller.getBoundingClientRect().top;
          const anchor = [...scroller.querySelectorAll<HTMLElement>('[data-turn-seq]')].find(n => n.getBoundingClientRect().bottom >= top);
          anchorRef.current = direction !== 'older' && scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop < 24 ? 'bottom' : anchor ? { seq: anchor.dataset.turnSeq!, top: anchor.getBoundingClientRect().top } : null;
        }
        let merged = initial ? page : mergeTurnEntries(current, page);
        if (initial) { older = page.length === TURN_RAIL_PAGE_SIZE; newer = false; }
        else if (direction === 'older') { older = page.length === TURN_RAIL_PAGE_SIZE; }
        else { newer = page.length === TURN_RAIL_PAGE_SIZE; }
        if (merged.length > TURN_RAIL_WINDOW_SIZE) {
          if (direction === 'older') { merged = merged.slice(0, TURN_RAIL_WINDOW_SIZE); newer = true; }
          else { merged = merged.slice(-TURN_RAIL_WINDOW_SIZE); older = true; }
        }
        current = merged;
        loaded = true;
        setEntries(merged);
        setPages({ older, newer });
        setStatus('ready');
      } catch {
        if (!cancelled) setStatus('error');
      } finally {
        loading = false;
        if (!cancelled && refreshQueued) { refreshQueued = false; void load('refresh'); }
      }
    };
    loadRef.current = direction => { void load(direction); };
    void load('initial');
    return () => { cancelled = true; };
  }, [locatorKey]);

  useEffect(() => {
    if (previousSignal.current === refreshSignal) return;
    previousSignal.current = refreshSignal;
    loadRef.current('refresh');
  }, [refreshSignal]);

  useLayoutEffect(() => {
    const scroller = scrollRef.current;
    const anchor = anchorRef.current;
    if (!scroller || !anchor) return;
    if (anchor === 'bottom') scroller.scrollTop = scroller.scrollHeight;
    else {
      const item = scroller.querySelector<HTMLElement>('[data-turn-seq="' + anchor.seq + '"]');
      if (item) scroller.scrollTop += item.getBoundingClientRect().top - anchor.top;
    }
    anchorRef.current = null;
  }, [entries]);

  if (entries.length === 0 && status !== 'error') return null;

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
        onScroll={(event) => {
          if (status !== 'ready') return;
          const el = event.currentTarget;
          if (pages.older && el.scrollTop <= 16) loadRef.current('older');
          else if (pages.newer && el.scrollHeight - el.clientHeight - el.scrollTop <= 16) loadRef.current('newer');
        }}
        // Fade caps replace chapter lines and expand controls: content fading
        // out at either end means "keep scrolling, there is more".
        className="flex max-h-[70%] w-4 flex-none flex-col items-center gap-2 overflow-y-auto py-3 [mask-image:linear-gradient(to_bottom,transparent,black_12px,black_calc(100%-12px),transparent)] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
      {pages.older && <button type="button" aria-label={t('acp.turnRail.older')} disabled={status === 'loading'} onClick={() => loadRef.current('older')}><ChevronUp className="size-3" /></button>}
      {status === 'error' && <button type="button" aria-label={t('common.retry')} onClick={() => loadRef.current('initial')}><RotateCw className="size-3" /></button>}
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
      {pages.newer && <button type="button" aria-label={t('acp.turnRail.newer')} disabled={status === 'loading'} onClick={() => loadRef.current('newer')}><ChevronDown className="size-3" /></button>}
      </div>
    </nav>
    </TooltipProvider>
  );
}
