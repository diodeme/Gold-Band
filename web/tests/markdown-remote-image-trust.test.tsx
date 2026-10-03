/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { RemoteImageTrustVm } from '@/types';

vi.mock('react-i18next', () => ({
  initReactI18next: { type: '3rdParty', init: () => {} },
  useTranslation: () => ({ t: (key: string) => key }),
}));

const api = vi.hoisted(() => ({
  openExternalUrl: vi.fn(() => Promise.resolve()),
  trustRemoteImageHosts: vi.fn<(hosts: string[]) => Promise<RemoteImageTrustVm>>(),
  revokeRemoteImageHost: vi.fn<(host: string) => Promise<RemoteImageTrustVm>>(),
}));
vi.mock('@/api', () => api);

import { Markdown, MarkdownImagePreviewProvider, MarkdownResourceLinkProvider } from '@/components/prompt-kit/markdown';
import { RemoteImageTrustSettings, TRUSTED_HOST_PREVIEW_LIMIT } from '@/components/settings/RemoteImageTrustSettings';
import { TooltipProvider } from '@/components/ui/tooltip';
import { remoteImageHost } from '@/lib/remote-image-trust';
import {
  resetRemoteImageTrustForTests,
  revokeRemoteImageHost,
  seedRemoteImageTrust,
  trustRemoteImageHosts,
  useRemoteImageTrust,
} from '@/lib/remote-image-trust-store';

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const vm = (...trustedHosts: string[]): RemoteImageTrustVm => ({ schemaVersion: 1, trustedHosts });
let root: Root | null = null;

beforeEach(() => {
  resetRemoteImageTrustForTests();
  api.trustRemoteImageHosts.mockReset();
  api.revokeRemoteImageHost.mockReset();
  api.openExternalUrl.mockClear();
});

afterEach(async () => {
  if (root) await act(async () => root?.unmount());
  root = null;
  document.body.replaceChildren();
});

async function render(node: React.ReactNode) {
  const container = document.createElement('div');
  document.body.append(container);
  root = createRoot(container);
  await act(async () => root?.render(<TooltipProvider>{node}</TooltipProvider>));
  return container;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe('remote image host identity', () => {
  it('normalizes hosts like the backend and ignores non-remote sources', () => {
    expect(remoteImageHost('https://Static.Dion.BLUE/a.png')).toBe('static.dion.blue');
    expect(remoteImageHost('//cdn.example:8443/a.png')).toBe('cdn.example:8443');
    expect(remoteImageHost('https://example.com:443/a.png')).toBe('example.com');
    expect(remoteImageHost('https://例子.测试/a.png')).toBe('xn--fsqu00a.xn--0zwm56d');
    expect(remoteImageHost('docs/a.png')).toBeNull();
    expect(remoteImageHost('data:image/png;base64,AAAA')).toBeNull();
  });
});

describe('remote image trust store', () => {
  it('converges to the latest write and ignores a stale response', async () => {
    const first = deferred<RemoteImageTrustVm>();
    api.trustRemoteImageHosts.mockReturnValueOnce(first.promise);
    api.revokeRemoteImageHost.mockResolvedValueOnce(vm('b.example'));
    seedRemoteImageTrust(vm('a.example'));

    const trusting = trustRemoteImageHosts(['b.example']);
    await revokeRemoteImageHost('a.example');
    first.resolve(vm('a.example', 'b.example'));
    await trusting;

    let snapshot: RemoteImageTrustVm | null = null;
    function Probe() {
      snapshot = useRemoteImageTrust();
      return null;
    }
    await render(<Probe />);
    expect(snapshot).toEqual(vm('b.example'));
  });

  it('does not let a bootstrap seed roll back an in-flight write, and skips trusted hosts', async () => {
    const pending = deferred<RemoteImageTrustVm>();
    api.trustRemoteImageHosts.mockReturnValueOnce(pending.promise);
    const trusting = trustRemoteImageHosts(['a.example']);
    seedRemoteImageTrust(vm());
    pending.resolve(vm('a.example'));
    await trusting;

    await trustRemoteImageHosts(['a.example']);
    expect(api.trustRemoteImageHosts).toHaveBeenCalledTimes(1);
  });
});

describe('trusted image domain settings', () => {
  it('lists trusted hosts and removes one through the backend', async () => {
    seedRemoteImageTrust(vm('a.example', 'b.example'));
    api.revokeRemoteImageHost.mockResolvedValue(vm('b.example'));
    const container = await render(<RemoteImageTrustSettings />);
    expect([...container.querySelectorAll('li')].map((row) => row.textContent)).toEqual(['a.example', 'b.example']);

    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="settings.remoteImages.remove"]')!.click());
    expect(api.revokeRemoteImageHost).toHaveBeenCalledWith('a.example');
    expect([...container.querySelectorAll('li')].map((row) => row.textContent)).toEqual(['b.example']);
  });

  it('keeps the host chip and reports a failed removal', async () => {
    seedRemoteImageTrust(vm('a.example'));
    api.revokeRemoteImageHost.mockRejectedValue({ code: 'remote-image.host-invalid', params: {} });
    const container = await render(<RemoteImageTrustSettings />);

    await act(async () => container.querySelector<HTMLButtonElement>('button[aria-label="settings.remoteImages.remove"]')!.click());
    expect([...container.querySelectorAll('li')].map((row) => row.textContent)).toEqual(['a.example']);
    expect(container.querySelector('[data-slot="badge"]')?.getAttribute('aria-invalid')).toBe('true');
    expect(container.textContent).toContain('settings.remoteImages.removeFailed');
  });

  it('folds hosts beyond the preview limit behind a show-all toggle', async () => {
    const hosts = Array.from({ length: TRUSTED_HOST_PREVIEW_LIMIT + 5 }, (_, index) => `h${String(index).padStart(2, '0')}.example`);
    seedRemoteImageTrust(vm(...hosts));
    const container = await render(<RemoteImageTrustSettings />);
    const chips = () => container.querySelectorAll('[data-slot="badge"]').length;
    const toggle = () => container.querySelector<HTMLButtonElement>('button[aria-expanded]')!;

    expect(chips()).toBe(TRUSTED_HOST_PREVIEW_LIMIT);
    expect(toggle().textContent).toBe('settings.remoteImages.hiddenCount');
    expect(toggle().getAttribute('aria-label')).toBe('settings.remoteImages.showAll');
    await act(async () => toggle().click());
    expect(chips()).toBe(hosts.length);
    expect(toggle().getAttribute('aria-expanded')).toBe('true');
    await act(async () => toggle().click());
    expect(chips()).toBe(TRUSTED_HOST_PREVIEW_LIMIT);
  });

  it('shows no toggle when every host fits', async () => {
    seedRemoteImageTrust(vm(...Array.from({ length: TRUSTED_HOST_PREVIEW_LIMIT }, (_, index) => `h${index}.example`)));
    const container = await render(<RemoteImageTrustSettings />);
    expect(container.querySelectorAll('[data-slot="badge"]')).toHaveLength(TRUSTED_HOST_PREVIEW_LIMIT);
    expect(container.querySelector('button[aria-expanded]')).toBeNull();
  });

  it('shows the empty state', async () => {
    const container = await render(<RemoteImageTrustSettings />);
    expect(container.textContent).toBe('settings.remoteImages.empty');
  });
});

describe('chat Markdown remote images', () => {
  const source = 'https://static.dion.blue/shot.png';

  it('makes no request for an untrusted host until the user loads it', async () => {
    api.trustRemoteImageHosts.mockResolvedValue(vm('static.dion.blue'));
    const container = await render(<Markdown>{`![shot](${source})\n\n![again](${source.replace('shot', 'two')})`}</Markdown>);

    expect(container.querySelector('img')).toBeNull();
    const placeholders = container.querySelectorAll('[data-gb-markdown-image-blocked="static.dion.blue"]');
    expect(placeholders).toHaveLength(2);
    await act(async () => placeholders[0]!.querySelector('button')!.click());

    expect(api.trustRemoteImageHosts).toHaveBeenCalledWith(['static.dion.blue']);
    expect([...container.querySelectorAll('img')].map((image) => image.getAttribute('src'))).toEqual([
      source,
      source.replace('shot', 'two'),
    ]);
  });

  it('keeps the placeholder and reports a failed trust request', async () => {
    api.trustRemoteImageHosts.mockRejectedValue({ code: 'remote-image.host-invalid', params: {} });
    const container = await render(<Markdown>{`![shot](${source})`}</Markdown>);
    await act(async () => container.querySelector('button')!.click());
    expect(container.querySelector('img')).toBeNull();
    expect(container.textContent).toContain('common.remoteImage.trustFailed');
  });

  it('opens a loaded image through the workspace link handler, and a linked image follows its link', async () => {
    seedRemoteImageTrust(vm('static.dion.blue'));
    const openWebUrl = vi.fn();
    const handler = { openLocalFile: vi.fn(), openWebUrl };
    const container = await render(
      <MarkdownResourceLinkProvider handler={handler}>
        <Markdown>{`![shot](${source})\n\n[![badge](https://static.dion.blue/b.svg)](https://example.com/repo)`}</Markdown>
      </MarkdownResourceLinkProvider>,
    );
    const [image, badge] = [...container.querySelectorAll<HTMLImageElement>('img')];
    for (const loaded of [image!, badge!]) await act(async () => loaded.dispatchEvent(new Event('load')));

    expect(image!.className).toContain('cursor-pointer');
    await act(async () => image!.click());
    expect(openWebUrl).toHaveBeenCalledWith(source);

    openWebUrl.mockClear();
    await act(async () => badge!.click());
    expect(openWebUrl.mock.calls).toEqual([['https://example.com/repo']]);
  });

  it('opens a loaded image in the system browser when no workspace handler exists', async () => {
    seedRemoteImageTrust(vm('static.dion.blue'));
    const container = await render(<Markdown>{`![shot](${source})`}</Markdown>);
    const image = container.querySelector('img')!;
    await act(async () => image.dispatchEvent(new Event('load')));
    await act(async () => image.click());
    expect(api.openExternalUrl).toHaveBeenCalledWith(source);
  });

  it('loads release-note images without host trust and zooms them in the dialog', async () => {
    await render(
      <MarkdownImagePreviewProvider>
        <Markdown flavor="github-release">{`![shot](${source})`}</Markdown>
      </MarkdownImagePreviewProvider>,
    );
    const image = document.querySelector<HTMLImageElement>(`img[src="${source}"]`)!;
    await act(async () => image.dispatchEvent(new Event('load')));
    await act(async () => image.click());
    expect(document.querySelectorAll(`img[src="${source}"]`)).toHaveLength(2);
    expect(api.trustRemoteImageHosts).not.toHaveBeenCalled();
  });
});
