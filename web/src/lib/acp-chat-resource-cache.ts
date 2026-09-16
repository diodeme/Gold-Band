export const DEFAULT_ACP_RESOURCE_CACHE_SESSION_COUNT = 8;

export function normalizeAcpResourceCacheSessionCount(value?: number) {
  return Number.isFinite(value) && value && value > 0
    ? Math.floor(value)
    : DEFAULT_ACP_RESOURCE_CACHE_SESSION_COUNT;
}
