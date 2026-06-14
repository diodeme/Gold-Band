import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { AppTitleBar } from '../src/components/AppTitleBar';

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe('AppTitleBar', () => {
  it('reserves native traffic light space on macOS without custom controls', () => {
    const html = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      platform: 'macos',
      uiMode: 'workbench',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
      onToggleUiMode: () => {},
    }));

    expect(html).toContain('pl-[72px]');
    expect(html).not.toContain('common.minimizeWindow');
    expect(html).not.toContain('common.closeWindow');
  });

  it('keeps custom window controls on non-macOS platforms', () => {
    const html = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      platform: 'windows',
      uiMode: 'conversation',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
      onToggleUiMode: () => {},
    }));

    expect(html).toContain('common.minimizeWindow');
    expect(html).toContain('common.closeWindow');
  });
});
