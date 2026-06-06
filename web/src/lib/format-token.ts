/**
 * Format a token count with smart units for display.
 * - < 1K: raw number (e.g. 842)
 * - ≥ 1K and < 1M: K with one decimal (e.g. 12.0K, 123.4K)
 * - ≥ 1M: M with one decimal (e.g. 1.2M)
 */
export function formatTokenCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}
