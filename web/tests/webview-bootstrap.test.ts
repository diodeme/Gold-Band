// @vitest-environment jsdom

import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  renderWebviewStartupError,
  startWebviewBootstrap,
  unsupportedWebviewError,
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
    renderWebviewStartupError(unsupportedWebviewError(snapshot), undefined, 'zh-CN');

    const shell = document.querySelector('[data-webview-startup-error]');
    expect(shell?.getAttribute('data-webview-startup-error')).toBe('webview.capability.unsupported');
    expect(shell?.textContent).toContain('Gold Band 无法在当前 WebView 中启动');
    expect(shell?.textContent).toContain('不能通过单独更新 Safari 替换');
    expect(shell?.querySelector('pre')?.textContent).toContain('missingCapabilities');
    expect(shell?.querySelector('button')?.textContent).toBe('复制诊断信息');
  });
});
