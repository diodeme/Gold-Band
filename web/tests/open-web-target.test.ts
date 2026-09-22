import { describe, expect, it, vi } from 'vitest';
import { openWebTarget } from '@/components/workspace/browser/open-web-target';
import { browserSessionStore } from '@/components/workspace/browser/browser-session-store';
import { classifyWebTarget, isLocalhostUrl, normalizeBrowserAddress, systemBrowserHref } from '@/components/workspace/browser/web-target';
import { browserWorkspaceResourceKey } from '@/components/workspace/right-workspace-context';

describe('openWebTarget', () => {
  it('classifies http, local html, mailto and ordinary files', () => {
    expect(classifyWebTarget('https://example.com')).toBe('http');
    expect(classifyWebTarget('docs/index.html')).toBe('local-html');
    expect(classifyWebTarget('mailto:a@b.com')).toBe('system');
    expect(classifyWebTarget('src/main.rs')).toBe('local-file');
    expect(normalizeBrowserAddress('example.com/a')).toBe('https://example.com/a');
    expect(normalizeBrowserAddress('baidu', 'baidu')).toBe('https://www.baidu.com/s?wd=baidu');
    expect(normalizeBrowserAddress('gold band', 'google')).toBe('https://www.google.com/search?q=gold%20band');
    expect(normalizeBrowserAddress('金带', 'bing')).toBe('https://www.bing.com/search?q=%E9%87%91%E5%B8%A6');
    expect(normalizeBrowserAddress('localhost:1420')).toBe('https://localhost:1420');
    expect(isLocalhostUrl('http://127.0.0.1:1420')).toBe(true);
    expect(isLocalhostUrl('https://example.com')).toBe(false);
    expect(systemBrowserHref('baidu.com', 'about:blank')).toBe('https://baidu.com');
    expect(systemBrowserHref('gold band', 'about:blank', 'google')).toBe('https://www.google.com/search?q=gold%20band');
    expect(systemBrowserHref('', 'about:blank')).toBeNull();
    expect(systemBrowserHref('', 'https://baidu/')).toBe('https://baidu/');
  });

  it('routes local and public links according to the persisted browser preferences', async () => {
    const openResource = vi.fn();
    const openSystemUrl = vi.fn(async () => undefined);
    const browserPreferences = {
      schemaVersion: 1 as const,
      searchEngine: 'baidu' as const,
      openLocalLinksInBrowser: true,
      openWebLinksInBrowser: false,
    };
    const local = await openWebTarget('http://localhost:1420', {
      projectId: 'project-1', scopeKey: 'draft:project-1', openResource,
      browserTitle: '浏览器', openSystemUrl, browserPreferences,
    });
    const publicWeb = await openWebTarget('https://example.com', {
      projectId: 'project-1', scopeKey: 'draft:project-1', openResource,
      browserTitle: '浏览器', openSystemUrl, browserPreferences,
    });
    expect(local).toMatchObject({ status: 'opened', kind: 'browser' });
    expect(publicWeb).toEqual({ status: 'opened', kind: 'system' });
    expect(openSystemUrl).toHaveBeenCalledWith('https://example.com');
  });

  it('opens the shared browser projection and reuses the same internal page', async () => {
    browserSessionStore.resetForTests();
    const openResource = vi.fn();
    const first = await openWebTarget('https://example.com/docs', {
      projectId: 'project-1',
      scopeKey: 'draft:project-1',
      openResource,
      browserTitle: '浏览器',
    });
    const second = await openWebTarget('https://example.com/docs', {
      projectId: 'project-1',
      scopeKey: 'draft:project-1',
      openResource,
      browserTitle: '浏览器',
    });
    expect(first).toMatchObject({ status: 'opened', kind: 'browser' });
    expect(second).toMatchObject({ status: 'opened', kind: 'browser', pageId: first.status === 'opened' ? first.pageId : null });
    expect(openResource).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'browser',
      key: browserWorkspaceResourceKey(),
      scopeKey: 'draft:project-1',
    }));
    expect(browserSessionStore.snapshot().pages).toHaveLength(1);
    browserSessionStore.resetForTests();
  });

  it('sends mailto to the system opener', async () => {
    const openSystemUrl = vi.fn(async () => undefined);
    const result = await openWebTarget('mailto:a@b.com', {
      projectId: 'project-1',
      scopeKey: 'draft:project-1',
      openResource: vi.fn(),
      browserTitle: '浏览器',
      openSystemUrl,
    });
    expect(result).toEqual({ status: 'opened', kind: 'system' });
    expect(openSystemUrl).toHaveBeenCalledWith('mailto:a@b.com');
  });

  it('resolves local html through the file link resolver', async () => {
    browserSessionStore.resetForTests();
    const resolveLocalHtml = vi.fn(async () => ({
      canonicalPath: 'D:/repo/docs/index.html',
    }));
    const result = await openWebTarget('docs/index.html', {
      projectId: 'project-1',
      scopeKey: 'draft:project-1',
      openResource: vi.fn(),
      browserTitle: '浏览器',
      resolveLocalHtml,
    });
    expect(resolveLocalHtml).toHaveBeenCalledWith({
      projectId: 'project-1',
      rawHref: 'docs/index.html',
    });
    expect(result).toMatchObject({ status: 'opened', kind: 'browser' });
    expect(browserSessionStore.activePage()?.url).toBe('D:/repo/docs/index.html');
    browserSessionStore.resetForTests();
  });

  it('keeps the browser resolver failure on the clicked link', async () => {
    browserSessionStore.resetForTests();
    const resolveLocalHtml = vi.fn(async () => {
      throw { code: 'workspace-file.path-outside-workspace', params: { path: '../secret.html' } };
    });
    const result = await openWebTarget('../secret.html', {
      projectId: 'project-1',
      scopeKey: 'draft:project-1',
      openResource: vi.fn(),
      browserTitle: '浏览器',
      resolveLocalHtml,
    });
    expect(result).toEqual({
      status: 'error',
      error: { code: 'workspace-file.path-outside-workspace', params: { path: '../secret.html' } },
    });
    expect(browserSessionStore.snapshot().pages).toHaveLength(0);
    browserSessionStore.resetForTests();
  });

  it('opens a local html page with the same canonical identity on repeated clicks', async () => {
    browserSessionStore.resetForTests();
    const resolveLocalHtml = vi.fn(async () => ({
      canonicalPath: 'D:/repo/docs/index.html',
    }));
    const context = {
      projectId: 'project-1',
      scopeKey: 'draft:project-1',
      openResource: vi.fn(),
      browserTitle: '浏览器',
      resolveLocalHtml,
    };
    const first = await openWebTarget('docs/index.html', context);
    const second = await openWebTarget('D:/repo/docs/index.html', context);
    expect(second).toMatchObject({
      status: 'opened',
      kind: 'browser',
      pageId: first.status === 'opened' ? first.pageId : null,
    });
    expect(browserSessionStore.snapshot().pages).toHaveLength(1);
    browserSessionStore.resetForTests();
  });
});
