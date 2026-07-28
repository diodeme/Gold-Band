import { readFileSync } from 'node:fs';
import path from 'node:path';
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
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
    }));

    expect(html).toContain('pl-[72px]');
    expect(html).not.toContain('common.minimizeWindow');
    expect(html).not.toContain('common.closeWindow');
  });

  it('keeps custom window controls on non-macOS platforms', () => {
    const html = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      platform: 'windows',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
    }));

    expect(html).toContain('common.minimizeWindow');
    expect(html).toContain('common.closeWindow');
    expect(html).toContain('bg-titlebar text-titlebar-foreground');
    expect(html).not.toContain('bg-titlebar pr-2.5');
    expect(html).toContain('w-max flex-none items-stretch pl-2');
    expect(html.match(/w-11 flex-none/g)).toHaveLength(2);
    expect(html).toContain('w-12 flex-none');
  });

  it('keeps the shared titlebar draggable while excluding interactive controls', () => {
    const html = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      platform: 'windows',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
    }));

    expect(html).toContain('app-titlebar-drag-region');
    expect(html).toContain('data-tauri-drag-region');
    expect(html).toContain('app-titlebar-no-drag');
    expect(html).toContain('data-titlebar-no-drag="true"');
  });

  it('delegates titlebar mouse gestures to the single Tauri drag-region owner', () => {
    const source = readFileSync(path.resolve(__dirname, '../src/components/AppTitleBar.tsx'), 'utf8');

    expect(source).toContain('data-tauri-drag-region');
    expect(source).not.toContain('.startDragging()');
    expect(source).not.toContain('onMouseDown={handleDragMouseDown}');
    expect(source).not.toContain('onDoubleClick={handleTitleBarDoubleClick}');
  });

  it('synchronizes maximize state on native resize and disposes the listener', () => {
    const source = readFileSync(path.resolve(__dirname, '../src/components/AppTitleBar.tsx'), 'utf8');

    expect(source).toContain('appWindow.onResized(() =>');
    expect(source).toContain('unlisten?.()');
    expect(source).not.toContain('windowControlsRightOffset');
  });

  it('does not expose the workbench mode switch', () => {
    const html = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      platform: 'windows',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
    }));

    expect(html).not.toContain('common.workbench');
    expect(html).not.toContain('common.conversation');
    expect(html).not.toContain('aria-pressed');
  });
});
