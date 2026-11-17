// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  appChunkLoadError,
  renderWebviewStartupError,
  startWebviewBootstrap,
  unsupportedWebviewError,
  webviewStartupPlatform,
} from '@/lib/webview-bootstrap-core';
import {
  createWebviewEnvironmentSnapshot,
  applyWebviewEnvironmentToDocument,
} from '@/lib/webview-environment';
import {
  fullWebviewCapabilityProfile,
  unsupportedWebviewCapabilityProfile,
  webkit613CapabilityProfile,
} from '@/lib/webview-capabilities';

describe('WebView bootstrap', () => {
  beforeEach(() => {
    document.body.innerHTML = '<div id="root"></div>';
    document.documentElement.removeAttribute('data-webview-tier');
  });

  it('does not load the business application for unsupported engines', async () => {
    const snapshot = createWebviewEnvironmentSnapshot(unsupportedWebviewCapabilityProfile);
    const loadApp = vi.fn(async () => {});
    const renderError = vi.fn();

    const result = await startWebviewBootstrap({ snapshot, loadApp, renderError });

    expect(result.loaded).toBe(false);
    expect(loadApp).not.toHaveBeenCalled();
    expect(renderError).toHaveBeenCalledWith(expect.objectContaining({
      code: 'webview.capability.unsupported',
    }));
  });

  it.each([webkit613CapabilityProfile, fullWebviewCapabilityProfile])(
    'loads the same business application once for supported capability profiles',
    async (capabilities) => {
      const snapshot = createWebviewEnvironmentSnapshot(capabilities);
      const loadApp = vi.fn(async () => {});

      const result = await startWebviewBootstrap({ snapshot, loadApp, renderError: vi.fn() });

      expect(result).toEqual({ loaded: true, error: null });
      expect(loadApp).toHaveBeenCalledTimes(1);
    },
  );

  it('converts application chunk failures into a visible structured startup error', async () => {
    const snapshot = createWebviewEnvironmentSnapshot(webkit613CapabilityProfile);
    const renderError = vi.fn();

    const result = await startWebviewBootstrap({
      snapshot,
      loadApp: async () => { throw new SyntaxError('unsupported syntax'); },
      renderError,
    });

    expect(result.error).toMatchObject({
      code: 'webview.app_chunk.load_failed',
      msg: 'unsupported syntax',
    });
    expect(renderError).toHaveBeenCalledTimes(1);
  });

  it('projects the immutable policy to document attributes', () => {
    const snapshot = createWebviewEnvironmentSnapshot(webkit613CapabilityProfile);
    applyWebviewEnvironmentToDocument(snapshot);

    expect(document.documentElement.dataset).toMatchObject({
      webviewTier: 'compatible',
      webviewThemeRendering: 'fallback-tokens',
      webviewResponsiveLayout: 'measured',
      webviewCodeHighlighting: 'wasm',
      webviewVisualMaterial: 'solid',
    });
  });

  it('renders a localized boot-safe error without React or application styles', () => {
    const snapshot = createWebviewEnvironmentSnapshot(unsupportedWebviewCapabilityProfile);
    renderWebviewStartupError(unsupportedWebviewError(snapshot), undefined, 'zh-CN', 'macos');

    const shell = document.querySelector('[data-webview-startup-error]');
    expect(shell?.getAttribute('data-webview-startup-error')).toBe('webview.capability.unsupported');
    expect(shell?.textContent).toContain('Gold Band 无法在当前 WebView 中启动');
    expect(shell?.textContent).toContain('不能通过单独更新 Safari 替换');
    expect(shell?.querySelector('pre')?.textContent).toContain('missingCapabilities');
    expect(shell?.querySelector('button')?.textContent).toBe('复制诊断信息');

    renderWebviewStartupError(unsupportedWebviewError(snapshot), undefined, 'ja-JP');
    expect(document.querySelector('[data-webview-startup-error]')?.textContent).toContain('Gold Band はこの WebView では起動できません');
    expect(document.documentElement.lang).toBe('ja-JP');

    renderWebviewStartupError(unsupportedWebviewError(snapshot), undefined, 'pt-PT');
    expect(document.querySelector('[data-webview-startup-error]')?.textContent).toContain('Gold Band cannot start in this WebView');
    expect(document.documentElement.lang).toBe('en');
  });

  it('detects supported desktop WebView platforms from the user agent', () => {
    expect(webviewStartupPlatform('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36')).toBe('windows');
    expect(webviewStartupPlatform('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15')).toBe('macos');
    expect(webviewStartupPlatform('Mozilla/5.0 (X11; Linux x86_64)')).toBe('other');
  });

  it('uses Windows WebView2 guidance and keeps chunk failures platform-neutral', () => {
    const snapshot = createWebviewEnvironmentSnapshot(unsupportedWebviewCapabilityProfile);

    renderWebviewStartupError(unsupportedWebviewError(snapshot), undefined, 'zh-CN', 'windows');
    expect(document.querySelector('[data-webview-startup-error]')?.textContent).toContain('WebView2');
    expect(document.querySelector('[data-webview-startup-error]')?.textContent).not.toContain('macOS');

    renderWebviewStartupError(appChunkLoadError(new Error('chunk failed'), snapshot), undefined, 'zh-CN', 'windows');
    const shellText = document.querySelector('[data-webview-startup-error]')?.textContent;
    expect(shellText).toContain('重新安装最新版 Gold Band');
    expect(shellText).not.toContain('macOS');
  });

  it.each([
    ['zh-Hans-HK', 'zh-CN', 'Gold Band 无法在当前 WebView 中启动'],
    ['zh-Hans-TW', 'zh-CN', 'Gold Band 无法在当前 WebView 中启动'],
    ['zh-Hans-MO', 'zh-CN', 'Gold Band 无法在当前 WebView 中启动'],
    ['zh-Hant-CN', 'zh-TW', 'Gold Band 無法在目前 WebView 中啟動'],
    ['zh-HK', 'zh-TW', 'Gold Band 無法在目前 WebView 中啟動'],
  ])('renders the Chinese script selected by %s', (language, locale, title) => {
    const snapshot = createWebviewEnvironmentSnapshot(unsupportedWebviewCapabilityProfile);

    renderWebviewStartupError(unsupportedWebviewError(snapshot), undefined, language);

    expect(document.querySelector('[data-webview-startup-error] h1')?.textContent).toBe(title);
    expect(document.documentElement.lang).toBe(locale);
  });
});
