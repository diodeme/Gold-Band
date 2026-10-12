import type { ConversationPromptInput, UserPromptQuote, UserPromptQuoteSource, UserPromptRole } from '@/types';
import type { CommittedSlashItem } from '@/lib/slash-command';
import { committedRoleSnapshot, slashSendableText } from '@/lib/slash-command';

/** Mirrors the Rust budgets in `src/provider/quotes.rs`. */
export const MAX_COMPOSER_QUOTE_CHARS = 12_000;
export const MAX_COMPOSER_DIFF_QUOTE_BYTES = 64_000;
export const MAX_COMPOSER_QUOTES = 64;
export const MAX_COMPOSER_CONTEXT_ITEMS = 10;

/** A quote is frozen when it is taken; the composer only adds or removes it. */
export type ComposerQuote = UserPromptQuote;
/** A quote taken from a surface, before the composer assigns its id. */
export type ComposerQuoteDraft = Omit<ComposerQuote, 'id'>;

export interface ComposerWorkspaceFileRef {
  id: string;
  projectId: string;
  relativePath: string;
  name: string;
  byteLength: number | null;
  mimeType: string;
  canonicalPath?: string;
}

export interface WorkspaceFileTimelineRef {
  projectId: string;
  relativePath: string;
  canonicalPath?: string;
  name?: string;
  mimeType?: string;
  size?: number;
}

export type AddComposerQuoteResult =
  | { ok: true; quotes: ComposerQuote[] }
  | { ok: false; code: 'composer.quote.limit-exceeded'; chars: number; maxChars: number; remainingChars: number }
  | { ok: false; code: 'composer.quote.diff-limit-exceeded'; bytes: number; maxBytes: number; remainingBytes: number }
  | { ok: false; code: 'composer.quote.count-exceeded'; maxQuotes: number }
  | { ok: false; code: 'composer.quote.duplicate' };

export type ComposerQuoteFailure = Extract<AddComposerQuoteResult, { ok: false }>;

const utf8 = new TextEncoder();

export function isWholeFileDiffQuote(quote: Pick<ComposerQuote, 'source'>) {
  return quote.source.kind === 'diff' && quote.source.scope === 'file';
}

/** Characters as Rust counts them (Unicode scalar values). */
function quoteChars(text: string) {
  let count = 0;
  for (const _ of text) count += 1;
  return count;
}

export function composerQuoteUsage(quotes: readonly ComposerQuote[]) {
  return quotes.reduce(
    (usage, quote) => isWholeFileDiffQuote(quote)
      ? { ...usage, diffBytes: usage.diffBytes + utf8.encode(quote.text).length }
      : { ...usage, chars: usage.chars + quoteChars(quote.text) },
    { chars: 0, diffBytes: 0 },
  );
}

/** Stable identity of where a quote came from, used to reject duplicate quotes. */
function quoteSourceIdentity(source: UserPromptQuoteSource) {
  switch (source.kind) {
    case 'agentMessage': return JSON.stringify([source.kind, source.messageKey]);
    case 'file': return JSON.stringify([source.kind, source.label, source.startLine, source.endLine]);
    case 'diff': return JSON.stringify([source.kind, source.origin, source.revision ?? null, source.path, source.scope]);
  }
}

/** Keeps indentation of quoted code while dropping blank lead-in and trailing whitespace. */
function normalizeQuoteText(text: string) {
  return text.replace(/^(?:[ \t]*\r?\n)+/u, '').trimEnd();
}

export function addComposerQuote(
  quotes: readonly ComposerQuote[],
  quote: ComposerQuote,
): AddComposerQuoteResult {
  const text = normalizeQuoteText(quote.text);
  if (quotes.length >= MAX_COMPOSER_QUOTES) {
    return { ok: false, code: 'composer.quote.count-exceeded', maxQuotes: MAX_COMPOSER_QUOTES };
  }
  const identity = quoteSourceIdentity(quote.source);
  if (quotes.some((item) => item.text === text && quoteSourceIdentity(item.source) === identity)) {
    return { ok: false, code: 'composer.quote.duplicate' };
  }
  const usage = composerQuoteUsage(quotes);
  if (isWholeFileDiffQuote(quote)) {
    const bytes = utf8.encode(text).length;
    const remainingBytes = MAX_COMPOSER_DIFF_QUOTE_BYTES - usage.diffBytes;
    if (bytes > remainingBytes) {
      return { ok: false, code: 'composer.quote.diff-limit-exceeded', bytes, maxBytes: MAX_COMPOSER_DIFF_QUOTE_BYTES, remainingBytes };
    }
  } else {
    const chars = quoteChars(text);
    const remainingChars = MAX_COMPOSER_QUOTE_CHARS - usage.chars;
    if (chars > remainingChars) {
      return { ok: false, code: 'composer.quote.limit-exceeded', chars, maxChars: MAX_COMPOSER_QUOTE_CHARS, remainingChars };
    }
  }
  return { ok: true, quotes: [...quotes, { ...quote, text }] };
}

export function createUserPromptSubmission(
  content: string,
  quotes: readonly ComposerQuote[],
  role?: UserPromptRole | null,
  workspaceFiles: readonly ComposerWorkspaceFileRef[] = [],
): ConversationPromptInput {
  const displayText = content.trim();
  return {
    displayText,
    quotes: quotes.map(({ id, text, source }) => ({ id, text, source })),
    workspaceFiles: workspaceFiles.map(({ projectId, relativePath }) => ({
      projectId,
      relativePath,
    })),
    ...(role ? { role } : {}),
  };
}

export function createComposerPromptSubmission(
  content: string,
  quotes: readonly ComposerQuote[],
  committed: CommittedSlashItem | null,
  workspaceFiles: readonly ComposerWorkspaceFileRef[] = [],
): ConversationPromptInput {
  return createUserPromptSubmission(
    slashSendableText(content, committed),
    quotes,
    committedRoleSnapshot(committed),
    workspaceFiles,
  );
}

export function normalizeComposerWorkspaceFileRef(ref: ComposerWorkspaceFileRef): ComposerWorkspaceFileRef {
  return {
    ...ref,
    relativePath: ref.relativePath.replaceAll('\\', '/'),
  };
}

function composerWorkspaceFileIdentity(ref: ComposerWorkspaceFileRef) {
  const projectId = ref.projectId.trim().toLowerCase();
  const relativePath = ref.relativePath.replaceAll('\\', '/');
  const platform = globalThis.navigator?.platform ?? '';
  const normalizedPath = /win/i.test(platform)
    ? relativePath.toLowerCase()
    : relativePath;
  return `${projectId}\0${normalizedPath}`;
}

export function addComposerWorkspaceFile(
  workspaceFiles: readonly ComposerWorkspaceFileRef[],
  attachmentCount: number,
  ref: ComposerWorkspaceFileRef,
): { ok: true; workspaceFiles: ComposerWorkspaceFileRef[] } | { ok: false; code: 'composer.context.limit-exceeded'; max: number } | { ok: false; code: 'composer.workspace-file.duplicate' } {
  const normalized = normalizeComposerWorkspaceFileRef(ref);
  const identity = composerWorkspaceFileIdentity;
  if (workspaceFiles.some((item) => identity(item) === identity(normalized))) {
    return { ok: false, code: 'composer.workspace-file.duplicate' };
  }
  if (workspaceFiles.length + attachmentCount >= MAX_COMPOSER_CONTEXT_ITEMS) {
    return { ok: false, code: 'composer.context.limit-exceeded', max: MAX_COMPOSER_CONTEXT_ITEMS };
  }
  return { ok: true, workspaceFiles: [...workspaceFiles, normalized] };
}

export function hasUserPromptPayload(
  content: string,
  attachmentCount = 0,
  role?: UserPromptRole | null,
  workspaceFileCount = 0,
) {
  return content.trim().length > 0
    || attachmentCount > 0
    || workspaceFileCount > 0
    || userPromptRoleFromRaw({ role }) != null;
}

export function workspaceFilesFromRaw(raw: unknown): WorkspaceFileTimelineRef[] {
  if (!raw || typeof raw !== 'object') return [];
  const files = (raw as { workspaceFiles?: unknown }).workspaceFiles;
  if (!Array.isArray(files)) return [];
  return files.flatMap((file): WorkspaceFileTimelineRef[] => {
    if (!file || typeof file !== 'object') return [];
    const { projectId, relativePath, canonicalPath, name, mimeType, size } = file as Record<string, unknown>;
    return typeof projectId === 'string'
      && projectId.length > 0
      && typeof relativePath === 'string'
      && relativePath.length > 0
      ? [{
        projectId,
        relativePath,
        ...(typeof canonicalPath === 'string' ? { canonicalPath } : {}),
        ...(typeof name === 'string' && name.length > 0 ? { name } : {}),
        ...(typeof mimeType === 'string' && mimeType.length > 0 ? { mimeType } : {}),
        ...(typeof size === 'number' && Number.isFinite(size) && size >= 0 ? { size } : {}),
      }]
      : [];
  });
}

function quoteSourceFromRaw(raw: unknown): UserPromptQuoteSource | null {
  if (!raw || typeof raw !== 'object') return null;
  const source = raw as Record<string, unknown>;
  const text = (value: unknown) => typeof value === 'string' && value.length > 0;
  const line = (value: unknown) => typeof value === 'number' && Number.isInteger(value) && value > 0;
  switch (source.kind) {
    case 'agentMessage':
      return text(source.messageKey) ? { kind: 'agentMessage', messageKey: source.messageKey as string } : null;
    case 'file':
      return text(source.label) && line(source.startLine) && line(source.endLine)
        ? { kind: 'file', label: source.label as string, startLine: source.startLine as number, endLine: source.endLine as number }
        : null;
    case 'diff':
      return text(source.path)
        && typeof source.origin === 'string'
        && (source.scope === 'selection' || source.scope === 'file')
        ? {
          kind: 'diff',
          path: source.path as string,
          origin: source.origin as Extract<UserPromptQuoteSource, { kind: 'diff' }>['origin'],
          revision: text(source.revision) ? source.revision as string : null,
          scope: source.scope,
        }
        : null;
    default:
      return null;
  }
}

export function userPromptQuotesFromRaw(raw: unknown): UserPromptQuote[] {
  if (!raw || typeof raw !== 'object') return [];
  const quotes = (raw as { quotes?: unknown }).quotes;
  if (!Array.isArray(quotes)) return [];
  return quotes.flatMap((quote): UserPromptQuote[] => {
    if (!quote || typeof quote !== 'object') return [];
    const { id, text, source: rawSource, sourceMessageKey } = quote as Record<string, unknown>;
    // Quotes recorded before typed sources only carried the Agent message key.
    const source = rawSource === undefined
      ? quoteSourceFromRaw({ kind: 'agentMessage', messageKey: sourceMessageKey })
      : quoteSourceFromRaw(rawSource);
    if (typeof id !== 'string' || id.length === 0 || typeof text !== 'string' || !source) return [];
    const quoteValue = { id, text, source };
    return text.length > 0 || isWholeFileDiffQuote(quoteValue) ? [quoteValue] : [];
  });
}

export function userPromptRoleFromRaw(raw: unknown): UserPromptRole | null {
  if (!raw || typeof raw !== 'object') return null;
  const role = (raw as { role?: unknown }).role;
  if (!role || typeof role !== 'object') return null;
  const { profileId, name, content } = role as Record<string, unknown>;
  return typeof profileId === 'string'
    && profileId.length > 0
    && typeof name === 'string'
    && name.length > 0
    && typeof content === 'string'
    && content.length > 0
    ? { profileId, name, content }
    : null;
}
