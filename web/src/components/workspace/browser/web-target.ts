import { isExternalUrlHref, isHttpUrlHref, isLocalFileHref, isSystemHandlerHref, parseLocalFileLinkTarget } from '@/lib/file-link';
import type { BrowserSearchEngine } from '@/types';

export type { BrowserSearchEngine } from '@/types';

const SEARCH_URLS: Record<BrowserSearchEngine, string> = {
  baidu: 'https://www.baidu.com/s?wd=',
  google: 'https://www.google.com/search?q=',
  bing: 'https://www.bing.com/search?q=',
};

export { isHttpUrlHref, isSystemHandlerHref };

export function isHtmlDocumentPath(path: string) {
  const value = path.trim().replaceAll('\\', '/');
  const withoutHash = value.split('#')[0] ?? value;
  const withoutQuery = withoutHash.split('?')[0] ?? withoutHash;
  const withoutLine = withoutQuery.replace(/:\d+(?::\d+)?$/u, '');
  return /\.(?:html|htm)$/iu.test(withoutLine);
}

export function localHtmlHrefFrom(href: string) {
  const trimmed = href.trim();
  if (!trimmed) return null;
  if (isHtmlDocumentPath(trimmed)) {
    const target = parseLocalFileLinkTarget(trimmed);
    return target ? trimmed.slice(0, -target.sourceSuffix.length) : trimmed;
  }
  if (/^file:\/\//iu.test(trimmed) && isHtmlDocumentPath(trimmed)) return trimmed;
  return null;
}

export function classifyWebTarget(href: string): 'http' | 'local-html' | 'local-file' | 'system' | 'invalid' {
  const trimmed = href.trim();
  if (!trimmed) return 'invalid';
  if (isSystemHandlerHref(trimmed)) return 'system';
  if (isHttpUrlHref(trimmed)) return 'http';
  if (isExternalUrlHref(trimmed)) return 'system';
  if (localHtmlHrefFrom(trimmed) || (isLocalFileHref(trimmed) && isHtmlDocumentPath(trimmed))) return 'local-html';
  if (isLocalFileHref(trimmed)) return 'local-file';
  return 'invalid';
}

export function isLocalhostUrl(input: string) {
  try {
    const url = new URL(normalizeExplicitBrowserAddress(input));
    const hostname = url.hostname.toLowerCase();
    return hostname === 'localhost'
      || hostname.endsWith('.localhost')
      || hostname === '::1'
      || /^127(?:\.\d{1,3}){3}$/u.test(hostname);
  } catch {
    return false;
  }
}

function normalizeExplicitBrowserAddress(input: string) {
  const trimmed = input.trim();
  if (!trimmed) return 'about:blank';
  if (trimmed.toLowerCase() === 'about:blank') return 'about:blank';
  if (/^localhost(?::\d+)?(?:[/?#]|$)/iu.test(trimmed)) return `https://${trimmed}`;
  if (/^[a-z][a-z\d+.-]*:/iu.test(trimmed)) return trimmed;
  if (localHtmlHrefFrom(trimmed) || /^[a-z]:[\\/]/iu.test(trimmed) || trimmed.startsWith('\\\\')) return trimmed;
  return `https://${trimmed}`;
}

export function isBrowserAddressQuery(input: string) {
  return looksLikeBrowserAddress(input.trim());
}

function looksLikeBrowserAddress(input: string) {
  if (/^[a-z][a-z\d+.-]*:/iu.test(input)) return true;
  if (localHtmlHrefFrom(input) || /^[a-z]:[\\/]/iu.test(input) || input.startsWith('\\\\')) return true;
  if (/^localhost(?::\d+)?(?:[/?#]|$)/iu.test(input)) return true;
  if (/^\[[0-9a-f:]+\](?::\d+)?(?:[/?#]|$)/iu.test(input)) return true;
  if (/^\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?(?:[/?#]|$)/u.test(input)) return true;
  return /^(?:[^\s./]+\.)+[^\s./]+(?::\d+)?(?:[/?#]|$)/u.test(input);
}

export function normalizeBrowserAddress(input: string, searchEngine: BrowserSearchEngine = 'baidu') {
  const trimmed = input.trim();
  if (!trimmed) return 'about:blank';
  if (looksLikeBrowserAddress(trimmed)) return normalizeExplicitBrowserAddress(trimmed);
  return `${SEARCH_URLS[searchEngine]}${encodeURIComponent(trimmed)}`;
}

export function systemBrowserHref(
  address: string,
  currentUrl: string,
  searchEngine: BrowserSearchEngine = 'baidu',
) {
  const trimmed = address.trim();
  if (trimmed) return normalizeBrowserAddress(trimmed, searchEngine);
  return currentUrl === 'about:blank' ? null : currentUrl;
}
