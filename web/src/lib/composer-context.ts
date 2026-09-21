export const MAX_COMPOSER_QUOTE_CHARS = 12_000;
export const MAX_COMPOSER_QUOTES = 64;
export const MAX_COMPOSER_CONTEXT_ITEMS = 10;
export const MAX_COMPOSER_QUOTE_ID_LENGTH = 128;
export const MAX_COMPOSER_QUOTE_SOURCE_KEY_LENGTH = 512;

export interface ComposerQuote {
  id: string;
  sourceKey: string;
  text: string;
}

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

import type { ConversationPromptInput, UserPromptQuote, UserPromptRole } from '@/types';
import type { CommittedSlashItem } from '@/lib/slash-command';
import { committedRoleSnapshot, slashSendableText } from '@/lib/slash-command';

export type AddComposerQuoteResult =
  | { ok: true; quotes: ComposerQuote[] }
  | { ok: false; code: 'composer.quote.limit-exceeded'; maxChars: number }
  | { ok: false; code: 'composer.quote.count-exceeded'; maxQuotes: number }
  | { ok: false; code: 'composer.quote.duplicate' };

export function composerQuoteChars(quotes: readonly ComposerQuote[]) {
  return quotes.reduce((total, quote) => total + quote.text.length, 0);
}

export function addComposerQuote(
  quotes: readonly ComposerQuote[],
  quote: ComposerQuote,
  maxChars = MAX_COMPOSER_QUOTE_CHARS,
): AddComposerQuoteResult {
  const text = quote.text.trim();
  if (quotes.length >= MAX_COMPOSER_QUOTES) {
    return { ok: false, code: 'composer.quote.count-exceeded', maxQuotes: MAX_COMPOSER_QUOTES };
  }
  if (quotes.some((item) => item.sourceKey === quote.sourceKey && item.text === text)) {
    return { ok: false, code: 'composer.quote.duplicate' };
  }
  if (composerQuoteChars(quotes) + text.length > maxChars) {
    return { ok: false, code: 'composer.quote.limit-exceeded', maxChars };
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
  const promptQuotes = quotes.map(({ id, sourceKey, text }) => ({
    id,
    sourceMessageKey: sourceKey,
    text,
  }));
  return {
    displayText,
    quotes: promptQuotes,
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

export function serializeUserPromptSubmission(input: ConversationPromptInput) {
  const displayText = input.displayText.trim();
  if (input.quotes.length === 0) return displayText;
  const quoteBlocks = input.quotes.map((quote) =>
    quote.text
      .split('\n')
      .map((line) => `> ${line}`)
      .join('\n'),
  );
  return `${quoteBlocks.join('\n\n')}\n\n${displayText}`;
}

export function userPromptQuotesFromRaw(raw: unknown): UserPromptQuote[] {
  if (!raw || typeof raw !== 'object') return [];
  const quotes = (raw as { quotes?: unknown }).quotes;
  if (!Array.isArray(quotes)) return [];
  return quotes.flatMap((quote) => {
    if (!quote || typeof quote !== 'object') return [];
    const { id, sourceMessageKey, text } = quote as Record<string, unknown>;
    return typeof id === 'string'
      && id.length > 0
      && typeof sourceMessageKey === 'string'
      && sourceMessageKey.length > 0
      && typeof text === 'string'
      && text.length > 0
      ? [{ id, sourceMessageKey, text }]
      : [];
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
