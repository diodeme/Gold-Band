import { browserResolveLocalHtml, openExternalUrl } from '@/api';
import type { RightWorkspaceCommands } from '../right-workspace-context';
import type { BrowserPreferences, WorkspaceRootRef } from '@/types';
import { browserWorkspaceResourceKey } from '../right-workspace-context';
import { browserSessionStore } from './browser-session-store';
import {
  classifyWebTarget,
  isLocalhostUrl,
  localHtmlHrefFrom,
  normalizeBrowserAddress,
} from './web-target';

export interface OpenWebTargetContext {
  /** File root local documents resolve against; `null` when none is available. */
  root: WorkspaceRootRef | null;
  scopeKey: string | null;
  openResource: RightWorkspaceCommands['openResource'];
  browserTitle: string;
  resolveLocalHtml?: typeof browserResolveLocalHtml;
  openSystemUrl?: typeof openExternalUrl;
  browserPreferences?: BrowserPreferences;
}

export type OpenWebTargetResult =
  | { status: 'opened'; kind: 'browser' | 'file' | 'system'; pageId?: string }
  | { status: 'error'; error: { code: string; params: Record<string, unknown> } };

export async function openWebTarget(
  href: string,
  context: OpenWebTargetContext,
): Promise<OpenWebTargetResult> {
  const kind = classifyWebTarget(href);
  if (kind === 'invalid') {
    return { status: 'error', error: { code: 'browser.navigation.invalid', params: {} } };
  }
  if (kind === 'system') {
    await (context.openSystemUrl ?? openExternalUrl)(href);
    return { status: 'opened', kind: 'system' };
  }
  if (kind === 'http' && context.browserPreferences) {
    const openInBrowser = isLocalhostUrl(href)
      ? context.browserPreferences.openLocalLinksInBrowser
      : context.browserPreferences.openWebLinksInBrowser;
    if (!openInBrowser) {
      await (context.openSystemUrl ?? openExternalUrl)(normalizeBrowserAddress(href, context.browserPreferences.searchEngine));
      return { status: 'opened', kind: 'system' };
    }
  }
  if (kind === 'local-file') {
    return { status: 'error', error: { code: 'browser.navigation.invalid', params: {} } };
  }
  if (!context.scopeKey) {
    return { status: 'error', error: { code: 'workspace-file.project-not-found', params: {} } };
  }
  if (kind === 'local-html') {
    return openLocalDocumentInBrowser(localHtmlHrefFrom(href) ?? href, context);
  }
  return openBrowserPage(normalizeBrowserAddress(href, context.browserPreferences?.searchEngine), context.scopeKey, context);
}

/** Opens a local HTML/SVG document in the built-in browser once the backend grants its directory. */
export async function openLocalDocumentInBrowser(
  rawPath: string,
  context: OpenWebTargetContext,
): Promise<OpenWebTargetResult> {
  if (!context.scopeKey) {
    return { status: 'error', error: { code: 'workspace-file.project-not-found', params: {} } };
  }
  if (!context.root) {
    return { status: 'error', error: { code: 'workspace-file.workspace-unavailable', params: {} } };
  }
  let url: string;
  try {
    const resolved = await (context.resolveLocalHtml ?? browserResolveLocalHtml)({
      ...context.root,
      rawHref: rawPath,
    });
    url = resolved.canonicalPath;
  } catch (reason) {
    const value = reason as { code?: unknown; params?: unknown };
    return {
      status: 'error',
      error: {
        code: typeof value.code === 'string' ? value.code : 'browser.local_html.grant_failed',
        params: typeof value.params === 'object' && value.params
          ? value.params as Record<string, unknown>
          : {},
      },
    };
  }
  return openBrowserPage(url, context.scopeKey, context);
}

async function openBrowserPage(url: string, scopeKey: string, context: OpenWebTargetContext): Promise<OpenWebTargetResult> {
  let pageId: string;
  try {
    pageId = browserSessionStore.openUrl(url);
  } catch (reason) {
    const value = reason as { code?: unknown; params?: unknown };
    return {
      status: 'error',
      error: {
        code: typeof value.code === 'string' ? value.code : 'browser.page.limit_reached',
        params: {},
      },
    };
  }
  await context.openResource({
    kind: 'browser',
    key: browserWorkspaceResourceKey(),
    scopeKey,
    title: context.browserTitle,
    description: null,
    attention: false,
  });
  return { status: 'opened', kind: 'browser', pageId };
}
