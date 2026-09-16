import { resolveWorkspaceFileLink, openExternalUrl } from '@/api';
import type { RightWorkspaceCommands } from '../right-workspace-context';
import type { BrowserPreferences } from '@/types';
import { browserWorkspaceResourceKey } from '../right-workspace-context';
import { browserSessionStore } from './browser-session-store';
import {
  classifyWebTarget,
  isLocalhostUrl,
  localHtmlHrefFrom,
  normalizeBrowserAddress,
} from './web-target';

export interface OpenWebTargetContext {
  projectId: string | null;
  scopeKey: string | null;
  openResource: RightWorkspaceCommands['openResource'];
  browserTitle: string;
  resolveLocalFile?: typeof resolveWorkspaceFileLink;
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
  let url = kind === 'http'
    ? normalizeBrowserAddress(href, context.browserPreferences?.searchEngine)
    : href;
  if (kind === 'local-html') {
    const rawPath = localHtmlHrefFrom(href) ?? href;
    if (!context.projectId) {
      return { status: 'error', error: { code: 'workspace-file.project-not-found', params: {} } };
    }
    try {
      const resolved = await (context.resolveLocalFile ?? resolveWorkspaceFileLink)(
        context.projectId,
        rawPath,
      );
      url = resolved.locator.canonicalPath;
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
  }
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
    scopeKey: context.scopeKey,
    title: context.browserTitle,
    description: null,
    attention: false,
  });
  return { status: 'opened', kind: 'browser', pageId };
}
