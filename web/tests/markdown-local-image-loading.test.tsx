/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { MarkdownImagePreviewVm, ResolveMarkdownImageInput, WorkspaceRootRef } from '@/types';

const api = vi.hoisted(() => ({
  openExternalUrl: vi.fn(async () => undefined),
  resolveMarkdownImage: vi.fn<(input: ResolveMarkdownImageInput) => Promise<MarkdownImagePreviewVm>>(),
  releaseWorkspaceFilePreview: vi.fn(async (_token: string) => undefined),
  workspaceFilePreviewUrl: (token: string) => `http://gold-band-file.localhost/${token}`,
}));
vi.mock('@/api', () => api);
import { Markdown, MarkdownResourceLinkProvider } from '@/components/prompt-kit/markdown';
import { MarkdownLocalImageProvider } from '@/components/prompt-kit/markdown-local-images';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | null = null;
let container: HTMLDivElement;
const observers = new Map<Element, (visible: boolean) => void>();
const workspace = { projectId: 'project-a', workspacePath: 'D:/repo/.wt/one' };
const ready = (token: string): MarkdownImagePreviewVm => ({
  kind: 'ready', canonicalPath: 'D:/repo/.wt/one/docs/overview.png',
  previewGrant: { token, expiresAtMs: String(Date.now() + 300_000) },
  width: 1920, height: 1080, mimeType: 'image/png', animated: false,
});
const openLocalFile = vi.fn();

beforeEach(() => {
  api.resolveMarkdownImage.mockReset().mockResolvedValue(ready('preview-1'));
  api.releaseWorkspaceFilePreview.mockClear();
  api.openExternalUrl.mockClear();
  openLocalFile.mockClear();
  vi.stubGlobal('IntersectionObserver', class {
    private element: Element | null = null;
    constructor(private readonly callback: IntersectionObserverCallback) {}
    observe(element: Element) {
      this.element = element;
      observers.set(element, visible => this.callback([{ isIntersecting: visible } as IntersectionObserverEntry], this as unknown as IntersectionObserver));
    }
    disconnect() { if (this.element) observers.delete(this.element); }
  });
  container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
});
afterEach(async () => {
  await act(async () => root?.unmount());
  root = null;
  observers.clear();
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});
async function render(content = '![preview](docs/overview.png)', scopeKey = 'session-1', imageRoot: WorkspaceRootRef | null = workspace) {
  await act(async () => root?.render(
    <TooltipProvider><MarkdownResourceLinkProvider handler={{ openLocalFile }}>
      <MarkdownLocalImageProvider scopeKey={scopeKey} root={imageRoot}>
        <Markdown>{content}</Markdown>
      </MarkdownLocalImageProvider>
    </MarkdownResourceLinkProvider></TooltipProvider>,
  ));
}
async function visibility(visible: boolean, count = Infinity) {
  await act(async () => [...observers.values()].slice(0, count).forEach(update => update(visible)));
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}

describe('visible local Markdown image loading', () => {
  it('loads through the session root only on intersection and releases offscreen', async () => {
    await render();
    expect(api.resolveMarkdownImage).not.toHaveBeenCalled();
    expect(container.querySelector('img')).toBeNull();
    await visibility(true);
    expect(api.resolveMarkdownImage).toHaveBeenCalledExactlyOnceWith({ ...workspace, rawSrc: 'docs/overview.png', source: { kind: 'workspace' } });
    expect(container.querySelector('img')?.src).toBe('http://gold-band-file.localhost/preview-1');
    await act(async () => container.querySelector('img')?.click());
    expect(openLocalFile).toHaveBeenCalledWith('D:/repo/.wt/one/docs/overview.png');
    expect(api.openExternalUrl).not.toHaveBeenCalled();
    await visibility(false);
    expect(container.querySelector('img')).toBeNull();
    expect(api.releaseWorkspaceFilePreview).toHaveBeenCalledWith('preview-1');
  });

  it('never remounts an already released token when a picture reenters the viewport', async () => {
    await render(); await visibility(true); await visibility(false);
    const pending = deferred<MarkdownImagePreviewVm>();
    api.resolveMarkdownImage.mockReturnValueOnce(pending.promise);
    const attributes = vi.spyOn(Element.prototype, 'setAttribute');
    try {
      await visibility(true);
      expect(attributes.mock.calls.filter(([name, value]) => name === 'src' && value.includes('/preview-1'))).toHaveLength(0);
      expect(container.querySelector('img')).toBeNull();
      await act(async () => pending.resolve(ready('fresh')));
      expect(container.querySelector('img')?.src).toContain('/fresh');
    } finally { attributes.mockRestore(); }
  });

  it('does not reread or replace a settled image after unrelated parent rendering', async () => {
    await render(); await visibility(true);
    const image = container.querySelector('img');
    await render();
    expect(container.querySelector('img')).toBe(image);
    expect(api.resolveMarkdownImage).toHaveBeenCalledTimes(1);
  });

  it('discards a previous scope response and releases its late token', async () => {
    const pending = deferred<MarkdownImagePreviewVm>();
    api.resolveMarkdownImage.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(ready('new'));
    await render(); await visibility(true);
    await render(undefined, 'session-2', { projectId: 'project-b', workspacePath: null });
    expect(container.querySelector('img')?.src).toContain('/new');
    await act(async () => pending.resolve(ready('old')));
    expect(container.querySelector('img')?.src).toContain('/new');
    expect(api.releaseWorkspaceFilePreview).toHaveBeenCalledWith('old');
    expect(api.resolveMarkdownImage.mock.calls[1][0].projectId).toBe('project-b');
  });

  it('does not fallback to a project root while the session root is unavailable', async () => {
    await render(undefined, 'unavailable', null); await visibility(true);
    expect(api.resolveMarkdownImage).not.toHaveBeenCalled();
    expect(container.querySelector('img')).toBeNull();
  });

  it('reclaims a token received after unmount', async () => {
    const pending = deferred<MarkdownImagePreviewVm>();
    api.resolveMarkdownImage.mockReturnValue(pending.promise);
    await render(); await visibility(true);
    await act(async () => root?.unmount()); root = null;
    await act(async () => pending.resolve(ready('late')));
    expect(api.releaseWorkspaceFilePreview).toHaveBeenCalledExactlyOnceWith('late');
  });

  it('only requests the four visible references among a large mounted history', async () => {
    const content = Array.from({ length: 100 }, (_, index) => `![image ${index}](docs/${index}.png)`).join('\n\n');
    await render(content);
    expect(api.resolveMarkdownImage).not.toHaveBeenCalled();
    await visibility(true, 4);
    expect(api.resolveMarkdownImage).toHaveBeenCalledTimes(4);
    expect(container.querySelectorAll('img')).toHaveLength(4);
  });

  it('never turns a malformed internal locator into a remote image request', async () => {
    await render('![bad](https://gold-band.local-image.invalid/?src=javascript%3Aalert)');
    await visibility(true);
    expect(api.resolveMarkdownImage).not.toHaveBeenCalled();
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('[data-gb-markdown-image-blocked]')).toBeNull();
  });
});
