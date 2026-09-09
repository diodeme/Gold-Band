import { describe, expect, it } from 'vitest';
import { demoHref, siteConfig } from '../../marketing/site/config';
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
  });
});
