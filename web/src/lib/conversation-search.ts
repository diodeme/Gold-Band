import type { ConversationPage, ConversationSearchResultVm } from '@/types';

export interface ConversationSearchHighlightSegment {
  text: string;
  highlighted: boolean;
}

export function conversationSearchHighlightSegments(
  text: string,
  query: string,
): ConversationSearchHighlightSegment[] {
  const uniqueTerms = new Map<string, string>();
  for (const term of query.trim().split(/\s+/u)) {
    if (!term) continue;
    uniqueTerms.set(term.toLocaleLowerCase(), term);
  }
  const terms = [...uniqueTerms.values()].sort((left, right) => right.length - left.length);
  if (!text || terms.length === 0) {
    return text ? [{ text, highlighted: false }] : [];
  }

  const matcher = new RegExp(terms.map(escapeRegExp).join('|'), 'giu');
  const segments: ConversationSearchHighlightSegment[] = [];
  let cursor = 0;
  for (const match of text.matchAll(matcher)) {
    const index = match.index ?? 0;
    if (index > cursor) {
      segments.push({ text: text.slice(cursor, index), highlighted: false });
    }
    segments.push({ text: match[0], highlighted: true });
    cursor = index + match[0].length;
  }
  if (cursor < text.length) {
    segments.push({ text: text.slice(cursor), highlighted: false });
  }
  return segments.length > 0 ? segments : [{ text, highlighted: false }];
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

export function conversationPageForSearchResult(
  result: ConversationSearchResultVm,
): ConversationPage | null {
  const runId = result.latestRun?.runId;
  if (!runId) return null;
  return {
    kind: 'conversation-run',
    projectId: result.projectId,
    taskId: result.taskId,
    runId,
  };
}
