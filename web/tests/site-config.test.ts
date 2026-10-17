import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { demoHref, siteConfig } from '../../marketing/site/config';
import { embeddedDemoParentHref } from '../../marketing/demo/routes';
describe('production website entrances', () => {
  it('requires an explicit production demo URL and permits a same-origin deployment path', () => {
    expect(() => siteConfig({}, true)).toThrow();
    expect(siteConfig({ VITE_DEMO_URL: '/demo/' }, true).demoUrl).toBe('/demo/');
    expect(siteConfig({}, false).demoUrl).toBe('http://127.0.0.1:1450/');
  });
  it('rejects executable, malformed and loopback URLs', () => {
    for (const VITE_DEMO_URL of ['javascript:alert(1)', '//example.com', 'https://user:pass@example.com', 'https://127.0.0.1', 'https://localhost', 'http://example.com', 'https://[::1]', 'https://[::ffff:127.0.0.1]', 'https://localhost.', 'https://2130706433', 'https://example.com\\@localhost']) {
      expect(() => siteConfig({ VITE_DEMO_URL }, true)).toThrow();
    }
    expect(() => siteConfig({ VITE_DEMO_URL: '/demo/', VITE_SITE_BASE: '/site/../' }, true)).toThrow();
  });
  it('preserves demo hash and supplies explicit language through the URL API', () => {
    expect(demoHref('/demo/#/session/one', 'en', 'https://example.com')).toBe('https://example.com/demo/?language=en#/session/one');
    expect(demoHref('/demo/#conversation-run', 'zh', 'https://example.com')).toBe('https://example.com/demo/?language=zh-cn#conversation-run');
    expect(demoHref('/demo/', 'zh', 'https://gold-band.dion.blue', 'dark', '#task-025?project=ji&run=run-001')).toBe('https://gold-band.dion.blue/demo/?language=zh-cn&theme=dark#task-025?project=ji&run=run-001');
  });
  it('publishes the demo session hash onto the website demo page', () => {
    expect(embeddedDemoParentHref('https://gold-band.dion.blue/zh/demo', '#task-025?project=ji&run=run-001')).toBe('https://gold-band.dion.blue/zh/demo#task-025?project=ji&run=run-001');
    expect(embeddedDemoParentHref('https://gold-band.dion.blue/zh/demo?x=1', 'task-025?project=ji')).toBe('https://gold-band.dion.blue/zh/demo?x=1#task-025?project=ji');
    expect(embeddedDemoParentHref('https://gold-band.dion.blue/zh/', '#task-025')).toBeNull();
  });
  it('keeps demo overlays inside the client window', () => {
    const frame = readFileSync('marketing/demo/DemoFrame.tsx', 'utf8');
    const css = readFileSync('marketing/demo/demo.css', 'utf8');
    const hostAt = frame.indexOf('id="gold-band-overlay-portal-host"');
    const clientAt = frame.indexOf('data-demo-client="true"');
    expect(clientAt).toBeGreaterThan(-1);
    expect(hostAt).toBeGreaterThan(clientAt);
    expect(css).toMatch(/\.demo-client\s*\{[^}]*transform:\s*translateZ\(0\)/s);
    expect(css).toMatch(/\.demo-client > \.app-window-frame\s*\{[^}]*height:\s*100%/s);
  });
});
