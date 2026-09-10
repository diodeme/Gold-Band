// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { App } from '../../marketing/site/main';

describe('embedded website demo navigation', () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
    vi.stubGlobal('matchMedia', vi.fn(() => ({ matches: false, addEventListener() {}, removeEventListener() {} })));
    vi.stubGlobal('scrollTo', vi.fn());
    history.replaceState(null, '', '/en/documentation');
    host = document.createElement('div'); document.body.append(host);
    root = createRoot(host);
  });
  afterEach(() => { act(() => root.unmount()); host.remove(); vi.unstubAllGlobals(); });
  it('keeps the header and parent document while entering demo and unloads it on navigation', () => {
    act(() => root.render(<App />));
    const header = host.querySelector('header');
    const demo = host.querySelector<HTMLAnchorElement>('nav a[href="/en/demo"]')!;
    act(() => demo.click());
    expect(location.pathname).toBe('/en/demo');
    expect(host.querySelector('header')).toBe(header);
    expect(host.querySelector('iframe')?.src).toContain('language=en');
    expect(host.querySelector('[role="status"]')?.textContent).toBe('Loading demo');
    act(() => host.querySelector('iframe')!.dispatchEvent(new Event('load')));
    expect(host.querySelector('[role="status"]')).toBeNull();
    expect(host.querySelector('footer')).toBeNull();
    act(() => host.querySelector<HTMLAnchorElement>('nav a[href="/en/documentation"]')!.click());
    expect(host.querySelector('iframe')).toBeNull();
    expect(host.querySelector('header')).toBe(header);
    act(() => { history.replaceState(null, '', '/zh/demo'); window.dispatchEvent(new PopStateEvent('popstate')); });
    expect(host.querySelector('iframe')?.src).toContain('language=zh-cn');
    expect(host.querySelector('nav a[aria-current="page"]')?.textContent).toBe('Demo');
  });
  it('renders demo directly on refresh', () => {
    history.replaceState(null, '', '/en/demo');
    act(() => root.render(<App />));
    expect(host.querySelector('iframe')).not.toBeNull();
    expect(location.pathname).toBe('/en/demo');
  });
});
