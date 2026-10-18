import { describe, expect, it, vi } from 'vitest';
import { openLocalDocumentInBrowser, openWebTarget } from '@/components/workspace/browser/open-web-target';
import { browserSessionStore } from '@/components/workspace/browser/browser-session-store';
import { classifyWebTarget, isBrowserDocumentPath, isHtmlDocumentPath, isLocalhostUrl, normalizeBrowserAddress, systemBrowserHref } from '@/components/workspace/browser/web-target';
import { browserWorkspaceResourceKey } from '@/components/workspace/right-workspace-context';

const MAIN_ROOT = { projectId: 'project-1', workspacePath: null };

describe('openWebTarget', () => {
  it('keeps the resolver URL query and fragment for the browser page', async () => {
    browserSessionStore.resetForTests();
    const url = 'file:///D:/repo/docs/index.html?ov=grid#section';
    const resolveLocalHtml = vi.fn(async () => ({ url }));
    const result = await openWebTarget('docs/index.html?ov=grid#section', {
      root: MAIN_ROOT, scopeKey: 'draft:project-1', openResource: vi.fn(),
      browserTitle: '浏览器', resolveLocalHtml,
    });
    expect(result).toMatchObject({ status: 'opened', kind: 'browser' });
    expect(browserSessionStore.activePage()?.url).toBe(url);
    browserSessionStore.resetForTests();
  });

  it('recognizes literal browser document filenames and encoded href paths separately', () => {
    expect(isBrowserDocumentPath('D:/repo/中文 # 100%.html')).toBe(true);
    expect(isBrowserDocumentPath('/repo/name?variant.html')).toBe(true);
    expect(classifyWebTarget('docs/%E4%B8%AD%E6%96%87%20%23%25.html?ov=grid#section')).toBe('local-html');
    expect(classifyWebTarget('docs/index%2Ehtml?ov=grid#section')).toBe('local-html');
    expect(classifyWebTarget('javascript:alert(1).html')).toBe('invalid');
  });

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
      root: MAIN_ROOT, scopeKey: 'draft:project-1', openResource,
      browserTitle: '浏览器', openSystemUrl, browserPreferences,
    });
    const publicWeb = await openWebTarget('https://example.com', {
      root: MAIN_ROOT, scopeKey: 'draft:project-1', openResource,
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
      root: MAIN_ROOT,
      scopeKey: 'draft:project-1',
      openResource,
      browserTitle: '浏览器',
    });
    const second = await openWebTarget('https://example.com/docs', {
      root: MAIN_ROOT,
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
      root: MAIN_ROOT,
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
      url: 'file:///D:/repo/docs/index.html',
    }));
    const result = await openWebTarget('docs/index.html', {
      root: MAIN_ROOT,
      scopeKey: 'draft:project-1',
      openResource: vi.fn(),
      browserTitle: '浏览器',
      resolveLocalHtml,
    });
    expect(resolveLocalHtml).toHaveBeenCalledWith({
      projectId: 'project-1',
      workspacePath: null,
      target: { kind: 'href', href: 'docs/index.html' },
    });
    expect(result).toMatchObject({ status: 'opened', kind: 'browser' });
    expect(browserSessionStore.activePage()?.url).toBe('file:///D:/repo/docs/index.html');
    browserSessionStore.resetForTests();
  });

  it('keeps SVG links on the file preview but opens SVG in the browser on request', async () => {
    expect(classifyWebTarget('art/pelican.svg')).toBe('local-file');
    expect(isHtmlDocumentPath('art/pelican.svg')).toBe(false);
    expect(isBrowserDocumentPath('art/pelican.SVG:12')).toBe(true);
    expect(isBrowserDocumentPath('docs/index.html')).toBe(true);
    expect(isBrowserDocumentPath('src/main.rs')).toBe(false);

    browserSessionStore.resetForTests();
    const openResource = vi.fn();
    const resolveLocalHtml = vi.fn(async () => ({ url: 'file:///D:/repo/art/pelican.svg' }));
    const result = await openLocalDocumentInBrowser('D:/repo/art/pelican.svg', {
      root: MAIN_ROOT,
      scopeKey: 'draft:project-1',
      openResource,
      browserTitle: '浏览器',
      resolveLocalHtml,
    });
    expect(resolveLocalHtml).toHaveBeenCalledWith({ projectId: 'project-1', workspacePath: null, target: { kind: 'path', path: 'D:/repo/art/pelican.svg' } });
    expect(result).toMatchObject({ status: 'opened', kind: 'browser' });
    expect(browserSessionStore.activePage()?.url).toBe('file:///D:/repo/art/pelican.svg');
    expect(openResource).toHaveBeenCalledWith(expect.objectContaining({ kind: 'browser', scopeKey: 'draft:project-1' }));
    browserSessionStore.resetForTests();
  });

  it('resolves local html in the session worktree and refuses it while unavailable', async () => {
    browserSessionStore.resetForTests();
    const resolveLocalHtml = vi.fn(async () => ({ url: 'file:///D:/repo/.wt/a/docs/index.html' }));
    const context = { scopeKey: 'draft:project-1', openResource: vi.fn(), browserTitle: '浏览器', resolveLocalHtml };
    await openWebTarget('docs/index.html', { ...context, root: { projectId: 'project-1', workspacePath: 'D:/repo/.wt/a' } });
    expect(resolveLocalHtml).toHaveBeenCalledWith({ projectId: 'project-1', workspacePath: 'D:/repo/.wt/a', target: { kind: 'href', href: 'docs/index.html' } });

    resolveLocalHtml.mockClear();
    const unavailable = await openWebTarget('docs/index.html', { ...context, root: null });
    expect(unavailable).toEqual({ status: 'error', error: { code: 'workspace-file.workspace-unavailable', params: {} } });
    expect(resolveLocalHtml).not.toHaveBeenCalled();
    browserSessionStore.resetForTests();
  });

  it('keeps the browser resolver failure on the clicked link', async () => {
    browserSessionStore.resetForTests();
    const resolveLocalHtml = vi.fn(async () => {
      throw { code: 'workspace-file.path-outside-workspace', params: { path: '../secret.html' } };
    });
    const result = await openWebTarget('../secret.html', {
      root: MAIN_ROOT,
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
      url: 'file:///D:/repo/docs/index.html',
    }));
    const context = {
      root: MAIN_ROOT,
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
