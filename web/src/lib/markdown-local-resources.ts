import type { Root } from 'hast';
import { visit } from 'unist-util-visit';
import { isLocalFileHref } from './file-link';

const LOCAL_FILE_PROXY_PREFIX = 'https://gold-band.local-file.invalid/?href=';
const LOCAL_IMAGE_PROXY_PREFIX = 'https://gold-band.local-image.invalid/?src=';

/** Transport through sanitizer/harden only. The locator never grants filesystem access. */
export function rehypeLocalResources() {
  return (tree: Root) => {
    visit(tree, 'element', (node) => {
      const property = node.tagName === 'a' ? 'href' : node.tagName === 'img' ? 'src' : null;
      if (!property) return;
      const value = node.properties[property];
      if (typeof value !== 'string' || !isLocalFileHref(value)) return;
      const prefix = property === 'href' ? LOCAL_FILE_PROXY_PREFIX : LOCAL_IMAGE_PROXY_PREFIX;
      node.properties[property] = `${prefix}${encodeURIComponent(value)}`;
    });
  };
}

function decodeLocator(value: string, prefix: string) {
  try {
    const decoded = decodeURIComponent(value.slice(prefix.length));
    return isLocalFileHref(decoded) ? decoded : null;
  } catch {
    return null;
  }
}

export function localHrefFromRenderedHref(href: string | undefined) {
  if (!href) return null;
  if (href.startsWith(LOCAL_FILE_PROXY_PREFIX)) return decodeLocator(href, LOCAL_FILE_PROXY_PREFIX);
  return isLocalFileHref(href) ? href : null;
}

export function isLocalImageLocator(src: string | undefined) {
  return Boolean(src?.startsWith(LOCAL_IMAGE_PROXY_PREFIX));
}

export function localImageSource(src: string | undefined) {
  return src?.startsWith(LOCAL_IMAGE_PROXY_PREFIX) ? decodeLocator(src, LOCAL_IMAGE_PROXY_PREFIX) : null;
}
