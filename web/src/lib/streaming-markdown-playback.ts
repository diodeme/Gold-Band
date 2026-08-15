import { recordAcpStreamingDiagnostic } from '@/lib/acp-streaming-diagnostics';

export const STREAMING_MARKDOWN_MIN_CHARS_PER_SECOND = 42;
export const STREAMING_MARKDOWN_MAX_CHARS_PER_SECOND = 180;
export const STREAMING_MARKDOWN_TARGET_CATCH_UP_MS = 320;
export const STREAMING_MARKDOWN_MAX_FRAME_MS = 64;
export const STREAMING_MARKDOWN_LONG_FRAME_MS = 50;
export const STREAMING_MARKDOWN_DIAGNOSTIC_SAMPLE_MS = 500;

const STREAMDOWN_TOKEN_SELECTOR = '[data-sd-animate]';
const STREAMDOWN_BLOCK_SELECTOR = '[data-gb-stream-block]';
const TOKEN_STATE_ATTRIBUTE = 'data-gb-stream-state';
const BLOCK_STATE_ATTRIBUTE = 'data-gb-stream-block-state';
const ITEM_VISIBLE_ATTRIBUTE = 'data-gb-stream-item-visible';
let nextPlaybackId = 1;

type PlaybackState = 'pending' | 'revealed' | 'settled';

type PlaybackBlock = {
  element: HTMLElement;
  units: HTMLElement[];
  start: number;
  end: number;
};

type PlaybackIndexUpdate = {
  blockCount: number;
  rebuiltBlockCount: number;
  reusedBlockCount: number;
  recalibratedBlocks: PlaybackBlock[];
  scannedUnitCount: number;
};

export type StreamingMarkdownPlayback = {
  dispose: () => void;
  setCanonical: (canonical: string) => void;
  setStreaming: (streaming: boolean) => void;
};

export function streamingMarkdownCharactersPerSecond(backlog: number) {
  return Math.min(
    STREAMING_MARKDOWN_MAX_CHARS_PER_SECOND,
    Math.max(
      STREAMING_MARKDOWN_MIN_CHARS_PER_SECOND,
      (Math.max(0, backlog) * 1000) / STREAMING_MARKDOWN_TARGET_CATCH_UP_MS,
    ),
  );
}

export function createStreamingMarkdownPlayback(
  root: HTMLElement,
  options: {
    canonical: string;
    streaming: boolean;
    reducedMotion?: boolean;
  },
): StreamingMarkdownPlayback {
  let canonical = options.canonical;
  let streaming = options.streaming;
  let rewriteBaseline = false;
  let disposed = false;
  let frameId = 0;
  let lastFrameAt = 0;
  let carry = 0;
  let revealedUnitCount = 0;
  let unitCount = 0;
  let blocks: PlaybackBlock[] = [];
  let playbackBlockIndex = 0;
  let lastDiagnosticSampleAt = 0;
  let framesSinceDiagnosticSample = 0;
  let revealedSinceDiagnosticSample = 0;
  let longestFrameSinceDiagnosticSample = 0;
  let tickDurationSinceDiagnosticSample = 0;
  let longestTickSinceDiagnosticSample = 0;
  const playbackId = nextPlaybackId++;
  const reducedMotion = options.reducedMotion ?? prefersReducedMotion();

  const diagnosticDetails = (extra: Record<string, unknown> = {}) => ({
    playbackId,
    canonicalLength: canonical.length,
    streaming,
    reducedMotion,
    unitCount,
    revealedUnitCount,
    backlog: Math.max(0, unitCount - revealedUnitCount),
    ...extra,
  });

  const setBlockState = (block: HTMLElement, state: PlaybackState) => {
    const blockState = state === 'pending' ? 'pending' : 'visible';
    if (block.getAttribute(BLOCK_STATE_ATTRIBUTE) !== blockState) {
      block.setAttribute(BLOCK_STATE_ATTRIBUTE, blockState);
    }
  };

  const setUnitState = (
    unit: HTMLElement,
    block: HTMLElement,
    state: PlaybackState,
  ) => {
    if (unit.matches(STREAMDOWN_TOKEN_SELECTOR)) {
      if (unit.getAttribute(TOKEN_STATE_ATTRIBUTE) !== state) {
        unit.setAttribute(TOKEN_STATE_ATTRIBUTE, state);
      }
      if (state !== 'pending') {
        const item = unit.closest('li');
        if (item?.getAttribute(ITEM_VISIBLE_ATTRIBUTE) !== 'true') {
          item?.setAttribute(ITEM_VISIBLE_ATTRIBUTE, 'true');
        }
      }
    }
    setBlockState(block, state);
  };

  const collectBlockUnits = (block: HTMLElement) => {
    const tokens = [...block.querySelectorAll<HTMLElement>(STREAMDOWN_TOKEN_SELECTOR)];
    if (tokens.length > 0) return tokens;
    return block.textContent?.length || block.querySelector('*') ? [block] : [];
  };

  const currentBlockElements = () => (
    [...root.querySelectorAll<HTMLElement>(STREAMDOWN_BLOCK_SELECTOR)]
  );

  const updatePlaybackIndex = (
    changedBlocks: ReadonlySet<HTMLElement>,
    forceRebuild = false,
  ): PlaybackIndexUpdate => {
    const previousByElement = new Map(
      blocks.map((block) => [block.element, block] as const),
    );
    const nextBlocks: PlaybackBlock[] = [];
    const recalibratedBlocks: PlaybackBlock[] = [];
    let nextUnitCount = 0;
    let rebuiltBlockCount = 0;
    let reusedBlockCount = 0;
    let scannedUnitCount = 0;

    for (const element of currentBlockElements()) {
      const previous = previousByElement.get(element);
      const rebuild = forceRebuild || !previous || changedBlocks.has(element);
      const units = rebuild ? collectBlockUnits(element) : previous.units;
      const block: PlaybackBlock = {
        element,
        units,
        start: nextUnitCount,
        end: nextUnitCount + units.length,
      };
      nextBlocks.push(block);
      nextUnitCount = block.end;

      if (rebuild) {
        rebuiltBlockCount += 1;
        scannedUnitCount += units.length;
        recalibratedBlocks.push(block);
      } else {
        reusedBlockCount += 1;
        if (previous.start !== block.start || previous.end !== block.end) {
          recalibratedBlocks.push(block);
        }
      }
    }

    blocks = nextBlocks;
    unitCount = nextUnitCount;
    return {
      blockCount: blocks.length,
      rebuiltBlockCount,
      reusedBlockCount,
      recalibratedBlocks,
      scannedUnitCount,
    };
  };

  const calibrateBlock = (block: PlaybackBlock) => {
    for (const item of block.element.querySelectorAll<HTMLElement>(
      `li[${ITEM_VISIBLE_ATTRIBUTE}]`,
    )) {
      item.removeAttribute(ITEM_VISIBLE_ATTRIBUTE);
    }

    let visible = false;
    for (let offset = 0; offset < block.units.length; offset += 1) {
      const state: PlaybackState = block.start + offset < revealedUnitCount
        ? 'settled'
        : 'pending';
      setUnitState(block.units[offset], block.element, state);
      if (state !== 'pending') visible = true;
    }
    setBlockState(block.element, visible ? 'settled' : 'pending');
  };

  const syncPlaybackCursor = () => {
    playbackBlockIndex = Math.min(playbackBlockIndex, blocks.length);
    while (
      playbackBlockIndex > 0
      && blocks[playbackBlockIndex - 1]?.end > revealedUnitCount
    ) playbackBlockIndex -= 1;
    while (
      playbackBlockIndex < blocks.length
      && blocks[playbackBlockIndex].end <= revealedUnitCount
    ) playbackBlockIndex += 1;
  };

  const resetIdlePlaybackTiming = () => {
    lastFrameAt = 0;
    carry = 0;
    lastDiagnosticSampleAt = 0;
    framesSinceDiagnosticSample = 0;
    revealedSinceDiagnosticSample = 0;
    longestFrameSinceDiagnosticSample = 0;
    tickDurationSinceDiagnosticSample = 0;
    longestTickSinceDiagnosticSample = 0;
  };

  const reconcile = (
    changedBlocks: ReadonlySet<HTMLElement> = new Set(),
    forceRebuild = false,
  ) => {
    if (disposed) return;
    const startedAt = performanceNow();
    const previousUnitCount = unitCount;
    const indexUpdate = updatePlaybackIndex(changedBlocks, forceRebuild);
    if (!streaming || reducedMotion || rewriteBaseline) {
      revealedUnitCount = unitCount;
    } else {
      revealedUnitCount = Math.min(revealedUnitCount, unitCount);
    }
    syncPlaybackCursor();

    for (const block of indexUpdate.recalibratedBlocks) calibrateBlock(block);

    recordAcpStreamingDiagnostic('markdown-playback-reconcile', () => diagnosticDetails({
      previousUnitCount,
      blockCount: indexUpdate.blockCount,
      rebuiltBlockCount: indexUpdate.rebuiltBlockCount,
      reusedBlockCount: indexUpdate.reusedBlockCount,
      recalibratedBlockCount: indexUpdate.recalibratedBlocks.length,
      scannedUnitCount: indexUpdate.scannedUnitCount,
      durationMs: roundDuration(performanceNow() - startedAt),
    }));
    schedule();
  };

  const settleAll = (reason: string, forceRebuild = false) => {
    const backlogBeforeSettle = Math.max(0, unitCount - revealedUnitCount);
    const indexUpdate = updatePlaybackIndex(new Set(), forceRebuild);
    revealedUnitCount = unitCount;
    playbackBlockIndex = blocks.length;
    const blocksToSettle = forceRebuild ? blocks : indexUpdate.recalibratedBlocks;
    for (const block of blocksToSettle) calibrateBlock(block);
    for (const block of blocks) {
      setBlockState(block.element, 'settled');
    }
    for (const item of root.querySelectorAll<HTMLElement>('li')) {
      if (item.getAttribute(ITEM_VISIBLE_ATTRIBUTE) !== 'true') {
        item.setAttribute(ITEM_VISIBLE_ATTRIBUTE, 'true');
      }
    }
    resetIdlePlaybackTiming();
    if (frameId !== 0) cancelAnimationFrame(frameId);
    frameId = 0;
    recordAcpStreamingDiagnostic('markdown-playback-settle', () => diagnosticDetails({
      reason,
      backlogBeforeSettle,
    }));
  };

  const nextPendingUnit = () => {
    syncPlaybackCursor();
    const block = blocks[playbackBlockIndex];
    if (!block) return null;
    const unit = block.units[revealedUnitCount - block.start];
    return unit ? { block, unit } : null;
  };

  const tick = (now: number) => {
    const tickStartedAt = performanceNow();
    frameId = 0;
    const backlog = unitCount - revealedUnitCount;
    if (disposed || !streaming || reducedMotion || backlog <= 0) {
      resetIdlePlaybackTiming();
      return;
    }

    const elapsed = lastFrameAt === 0
      ? 1000 / 60
      : Math.max(0, now - lastFrameAt);
    lastFrameAt = now;
    const playbackElapsed = Math.min(elapsed, STREAMING_MARKDOWN_MAX_FRAME_MS);
    const charactersPerSecond = streamingMarkdownCharactersPerSecond(backlog);
    carry += (charactersPerSecond * playbackElapsed) / 1000;
    let budget = Math.floor(carry);
    carry -= budget;
    let revealedThisFrame = 0;

    while (budget > 0 && revealedUnitCount < unitCount) {
      const pending = nextPendingUnit();
      if (!pending) break;
      revealedUnitCount += 1;
      setUnitState(pending.unit, pending.block.element, 'revealed');
      budget -= 1;
      revealedThisFrame += 1;
    }
    const tickDurationMs = performanceNow() - tickStartedAt;
    framesSinceDiagnosticSample += 1;
    revealedSinceDiagnosticSample += revealedThisFrame;
    longestFrameSinceDiagnosticSample = Math.max(longestFrameSinceDiagnosticSample, elapsed);
    tickDurationSinceDiagnosticSample += tickDurationMs;
    longestTickSinceDiagnosticSample = Math.max(
      longestTickSinceDiagnosticSample,
      tickDurationMs,
    );
    if (elapsed >= STREAMING_MARKDOWN_LONG_FRAME_MS) {
      recordAcpStreamingDiagnostic('markdown-playback-long-frame', () => diagnosticDetails({
        frameIntervalMs: roundDuration(elapsed),
        tickDurationMs: roundDuration(tickDurationMs),
        revealedThisFrame,
        charactersPerSecond,
      }));
    }
    if (
      lastDiagnosticSampleAt === 0
      || now - lastDiagnosticSampleAt >= STREAMING_MARKDOWN_DIAGNOSTIC_SAMPLE_MS
    ) {
      const sampleDurationMs = lastDiagnosticSampleAt === 0
        ? null
        : roundDuration(now - lastDiagnosticSampleAt);
      recordAcpStreamingDiagnostic('markdown-playback-sample', () => diagnosticDetails({
        sampleDurationMs,
        frameCount: framesSinceDiagnosticSample,
        revealedCount: revealedSinceDiagnosticSample,
        longestFrameMs: roundDuration(longestFrameSinceDiagnosticSample),
        tickDurationTotalMs: roundDuration(tickDurationSinceDiagnosticSample),
        longestTickMs: roundDuration(longestTickSinceDiagnosticSample),
        charactersPerSecond,
      }));
      lastDiagnosticSampleAt = now;
      framesSinceDiagnosticSample = 0;
      revealedSinceDiagnosticSample = 0;
      longestFrameSinceDiagnosticSample = 0;
      tickDurationSinceDiagnosticSample = 0;
      longestTickSinceDiagnosticSample = 0;
    }
    if (revealedUnitCount >= unitCount) resetIdlePlaybackTiming();
    schedule();
  };

  function schedule() {
    if (
      disposed
      || frameId !== 0
      || !streaming
      || reducedMotion
      || revealedUnitCount >= unitCount
    ) return;
    frameId = requestAnimationFrame(tick);
  }

  const observer = new MutationObserver((records) => {
    const changedBlocks = new Set<HTMLElement>();
    for (const record of records) {
      const target = record.target instanceof HTMLElement
        ? record.target
        : record.target.parentElement;
      const block = target?.closest<HTMLElement>(STREAMDOWN_BLOCK_SELECTOR);
      if (block && root.contains(block)) changedBlocks.add(block);
      for (const node of record.addedNodes) {
        if (!(node instanceof HTMLElement)) continue;
        if (node.matches(STREAMDOWN_BLOCK_SELECTOR)) changedBlocks.add(node);
        for (const addedBlock of node.querySelectorAll<HTMLElement>(STREAMDOWN_BLOCK_SELECTOR)) {
          changedBlocks.add(addedBlock);
        }
      }
    }
    reconcile(changedBlocks);
  });
  observer.observe(root, { characterData: true, childList: true, subtree: true });

  reconcile(new Set(), true);
  recordAcpStreamingDiagnostic('markdown-playback-init', () => diagnosticDetails());
  if (reducedMotion || !streaming) {
    settleAll(reducedMotion ? 'reduced-motion' : 'initial-static');
  }

  return {
    setCanonical(nextCanonical) {
      if (nextCanonical === canonical) return;
      const appendOnly = nextCanonical.startsWith(canonical);
      canonical = nextCanonical;
      if (!appendOnly) {
        rewriteBaseline = true;
        settleAll('non-append-rewrite', true);
        return;
      }
      rewriteBaseline = false;
    },
    setStreaming(nextStreaming) {
      if (streaming === nextStreaming) return;
      streaming = nextStreaming;
      if (!streaming || reducedMotion) {
        settleAll(reducedMotion ? 'reduced-motion' : 'stream-finished', true);
      } else {
        reconcile(new Set(), true);
      }
    },
    dispose() {
      disposed = true;
      observer.disconnect();
      if (frameId !== 0) cancelAnimationFrame(frameId);
      frameId = 0;
      resetIdlePlaybackTiming();
      recordAcpStreamingDiagnostic('markdown-playback-settle', () => diagnosticDetails({
        reason: 'dispose',
      }));
      blocks = [];
      unitCount = 0;
    },
  };
}

function performanceNow() {
  return typeof performance === 'undefined' ? 0 : performance.now();
}

function roundDuration(value: number) {
  return Math.round(value * 10) / 10;
}

function prefersReducedMotion() {
  return typeof window !== 'undefined'
    && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches === true;
}
