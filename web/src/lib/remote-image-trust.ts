/**
 * `user-trust` loads remote images only from hosts the user trusted (Settings → Advanced);
 * `allow` is reserved for content Gold Band publishes itself, such as release notes.
 */
export type RemoteImagePolicy = 'user-trust' | 'allow';

const REMOTE_IMAGE_SOURCE_PATTERN = /^(?:https?:)?\/\//iu;

export function isRemoteImageSource(source: string) {
  return REMOTE_IMAGE_SOURCE_PATTERN.test(source.trim());
}

/**
 * Host identity of a remote image, normalized by the WHATWG URL parser exactly like the
 * backend normalizes trusted hosts (lowercase, punycode, default port dropped).
 */
export function remoteImageHost(source: string): string | null {
  const trimmed = source.trim();
  if (!isRemoteImageSource(trimmed)) return null;
  try {
    const url = new URL(trimmed.startsWith('//') ? `https:${trimmed}` : trimmed);
    return url.host || null;
  } catch {
    return null;
  }
}
