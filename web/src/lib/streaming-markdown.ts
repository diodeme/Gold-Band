export const STREAMING_MARKDOWN_FRAME_MS = 32;
export const STREAMING_MARKDOWN_MIN_CHARS_PER_SECOND = 42;
export const STREAMING_MARKDOWN_MAX_CHARS_PER_SECOND = 180;
export const STREAMING_MARKDOWN_TARGET_CATCH_UP_MS = 320;

export type StreamingMarkdownPresentation = {
  canonical: string;
  offset: number;
  carry: number;
};

const trailingMarkdownDelimiter = /(?:\\|[*_~]+|`+)$/u;
const partialMarkdownImage = /(^|[^\\])!\[([^\]\n]*)(?:\](?:\([^\)\n]*)?)?$/u;
const partialMarkdownLink = /(^|[^\\!])\[([^\]\n]*)(?:\](?:\([^\)\n]*)?)?$/u;

/**
 * Keeps syntax-only suffixes out of the visible draft until they have enough
 * following content to be parsed as Markdown. The canonical source is never
 * changed; this only shapes the temporary presentation prefix.
 */
export function normalizeStreamingMarkdownPrefix(prefix: string) {
  return prefix
    .replace(partialMarkdownImage, '$1$2')
    .replace(partialMarkdownLink, '$1$2')
    .replace(trailingMarkdownDelimiter, '');
}

export function createStreamingMarkdownPresentation(
  canonical: string,
  streaming: boolean,
): StreamingMarkdownPresentation {
  if (!streaming || canonical.length === 0) {
    return { canonical, offset: canonical.length, carry: 0 };
  }

  return {
    canonical,
    offset: advanceUntilVisibleChange(canonical, 0, 1),
    carry: 0,
  };
}

export function syncStreamingMarkdownPresentation(
  current: StreamingMarkdownPresentation,
  canonical: string,
  streaming: boolean,
): StreamingMarkdownPresentation {
  if (canonical === current.canonical) return current;
  if (current.canonical.length === 0 && current.offset === 0) {
    return createStreamingMarkdownPresentation(canonical, streaming);
  }

  const visibleCanonicalPrefix = current.canonical.slice(0, current.offset);
  if (canonical.startsWith(visibleCanonicalPrefix)) {
    return {
      canonical,
      offset: Math.min(current.offset, canonical.length),
      carry: current.carry,
    };
  }

  return createStreamingMarkdownPresentation(canonical, streaming);
}

export function advanceStreamingMarkdownPresentation(
  current: StreamingMarkdownPresentation,
  elapsedMs = STREAMING_MARKDOWN_FRAME_MS,
): StreamingMarkdownPresentation {
  const backlog = current.canonical.length - current.offset;
  if (backlog <= 0) return current;

  const charsPerSecond = Math.min(
    STREAMING_MARKDOWN_MAX_CHARS_PER_SECOND,
    Math.max(
      STREAMING_MARKDOWN_MIN_CHARS_PER_SECOND,
      (backlog * 1000) / STREAMING_MARKDOWN_TARGET_CATCH_UP_MS,
    ),
  );
  const nextCarry =
    current.carry +
    (charsPerSecond * Math.min(Math.max(elapsedMs, 0), 64)) / 1000;
  const charBudget = Math.max(1, Math.floor(nextCarry));
  const nextOffset = advanceUntilVisibleChange(
    current.canonical,
    current.offset,
    charBudget,
  );

  return {
    canonical: current.canonical,
    offset: nextOffset,
    carry: Math.max(0, nextCarry - charBudget),
  };
}

export function streamingMarkdownPresentationText(
  presentation: StreamingMarkdownPresentation,
  streaming: boolean,
) {
  if (!streaming && presentation.offset >= presentation.canonical.length) {
    return presentation.canonical;
  }
  return normalizeStreamingMarkdownPrefix(
    presentation.canonical.slice(0, presentation.offset),
  );
}

export function isStreamingMarkdownPresentationPending(
  presentation: StreamingMarkdownPresentation,
) {
  return presentation.offset < presentation.canonical.length;
}

function advanceUntilVisibleChange(
  canonical: string,
  offset: number,
  charBudget: number,
) {
  const currentText = normalizeStreamingMarkdownPrefix(
    canonical.slice(0, offset),
  );
  let nextOffset = advanceCodePoints(canonical, offset, charBudget);

  // Markdown control characters and partial link destinations should not
  // consume their own visual frame. Advance through them atomically until the
  // rendered draft can actually change or the current snapshot is exhausted.
  while (
    nextOffset < canonical.length &&
    normalizeStreamingMarkdownPrefix(canonical.slice(0, nextOffset)) ===
      currentText
  ) {
    nextOffset = advanceCodePoints(canonical, nextOffset, 1);
  }

  return nextOffset;
}

function advanceCodePoints(source: string, offset: number, count: number) {
  let nextOffset = offset;
  let remaining = count;
  for (const char of source.slice(offset)) {
    nextOffset += char.length;
    remaining -= 1;
    if (remaining <= 0) break;
  }
  return nextOffset;
}
