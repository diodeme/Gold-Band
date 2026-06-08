import type { AcpUsageVm, NodeDetailVm } from '@/types';
import { formatTokenCount } from './format-token';

/**
 * Resolve accumulated token usage for a node across all conversations and attempts.
 * - Consumptive fields (inputTokens, outputTokens, etc.) are summed across all attempts.
 * - Context window fields (used, size) take the latest attempt's real-time value.
 */
export function resolveNodeTokenUsage(detail: NodeDetailVm): AcpUsageVm | null {
  const conversations = detail.acpConversations;
  // Fallback to legacy single-session path when no conversations
  if (!conversations || conversations.length === 0) {
    const usage = detail.acpSession?.usage;
    if (!usage || (usage.inputTokens == null && usage.outputTokens == null
      && usage.cachedReadTokens == null && usage.totalTokens == null)) return null;
    return usage;
  }
  const acc: AcpUsageVm = {};
  let latestUsed: number | null = null;
  let latestSize: number | null = null;
  let hasAnyBreakdown = false;
  for (const conv of conversations) {
    for (const attempt of conv.attempts) {
      const usage = attempt.acpSession?.usage;
      if (!usage) continue;
      if (usage.inputTokens != null) { acc.inputTokens = (acc.inputTokens ?? 0) + usage.inputTokens; hasAnyBreakdown = true; }
      if (usage.outputTokens != null) { acc.outputTokens = (acc.outputTokens ?? 0) + usage.outputTokens; hasAnyBreakdown = true; }
      if (usage.cachedReadTokens != null) { acc.cachedReadTokens = (acc.cachedReadTokens ?? 0) + usage.cachedReadTokens; hasAnyBreakdown = true; }
      if (usage.cachedWriteTokens != null) { acc.cachedWriteTokens = (acc.cachedWriteTokens ?? 0) + usage.cachedWriteTokens; hasAnyBreakdown = true; }
      if (usage.totalTokens != null) { acc.totalTokens = (acc.totalTokens ?? 0) + usage.totalTokens; hasAnyBreakdown = true; }
      if (usage.used != null) latestUsed = usage.used;
      if (usage.size != null) latestSize = usage.size;
    }
  }
  if (!hasAnyBreakdown) return null;
  acc.used = latestUsed;
  acc.size = latestSize;
  return acc;
}

/**
 * Format a nullable token count for display in InfoGrid.
 * Returns '-' for null/undefined, otherwise delegates to formatTokenCount.
 */
export function formatDisplayToken(n: number | null | undefined): string {
  if (n == null) return '-';
  return formatTokenCount(n);
}
