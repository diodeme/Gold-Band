/**
 * Prefixes a root-absolute public path with the deployment base so the same bundle can be hosted
 * under a subpath. Hosting at the root (desktop client, default website build) stays a no-op.
 */
export function publicAssetUrl(path: string, base = import.meta.env.BASE_URL) {
  return base === '/' || !path.startsWith('/') ? path : `${base.replace(/\/+$/, '')}${path}`;
}
