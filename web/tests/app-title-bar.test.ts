import { readFileSync } from 'node:fs';
import path from 'node:path';
import { createElement, type ComponentProps } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it, vi } from 'vitest';
import { APP_TITLE_BAR_LAYOUT, AppTitleBar as AppTitleBarComponent } from '../src/components/AppTitleBar';
import { TooltipProvider } from '../src/components/ui/tooltip';

function AppTitleBar(props: ComponentProps<typeof AppTitleBarComponent>) {
  return createElement(TooltipProvider, null, createElement(AppTitleBarComponent, props));
}

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

describe('AppTitleBar', () => {
  it('uses the compact shared desktop titlebar dimensions', () => {
    const html = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      feedbackEnabled: true,
      platform: 'windows',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
    }));

    expect(APP_TITLE_BAR_LAYOUT.rootClassName).toContain('h-9');
    expect(APP_TITLE_BAR_LAYOUT.brandMarkClassName).toContain('h-6 w-9');
    expect(APP_TITLE_BAR_LAYOUT.brandTitleClassName).toContain('text-base');
    expect(APP_TITLE_BAR_LAYOUT.brandTitleClassName).toContain('font-[700]');
    expect(APP_TITLE_BAR_LAYOUT.brandTitleClassName).not.toContain('font-bold');
    expect(APP_TITLE_BAR_LAYOUT.helpActionClassName).toContain('h-7');
    expect(html).toContain(APP_TITLE_BAR_LAYOUT.rootClassName);
    expect(html).toContain(APP_TITLE_BAR_LAYOUT.brandTitleClassName);
    expect(html).not.toContain('h-11');
    expect(html).not.toContain('font-semibold');
  });

  it('shows Help only when the channel capability is enabled', () => {
    const enabledHtml = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'MALING',
      feedbackEnabled: true,
      platform: 'windows',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
    }));
    const disabledHtml = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      feedbackEnabled: false,
      platform: 'windows',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
    }));

    expect(enabledHtml).toContain('common.help');
    expect(disabledHtml).not.toContain('common.help');
  });
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

  it('places the left workspace control by the brand and the right control in the trailing action area', () => {
    const source = readFileSync(path.resolve(__dirname, '../src/components/AppTitleBar.tsx'), 'utf8');
    const html = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      platform: 'windows',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
      rightWorkspaceOpen: true,
      onToggleRightWorkspace: () => {},
    }));

    const brandIndex = html.indexOf('data-titlebar-brand="true"');
    const leftIndex = html.indexOf('data-titlebar-sidebar-toggle="left"');
    const rightIndex = html.indexOf('data-titlebar-sidebar-toggle="right"');
    expect(brandIndex).toBeGreaterThanOrEqual(0);
    expect(leftIndex).toBeGreaterThan(brandIndex);
    expect(rightIndex).toBeGreaterThan(leftIndex);
    expect(html).toContain('workspace.closeWorkspace');
    expect(html).toContain('data-state="open"');
    expect(html).toContain('data-titlebar-trailing-actions="true"');
    expect(html.match(/size-7 rounded-\[6px\]/g)).toHaveLength(2);
    expect(source).toContain('<PanelLeft className="size-3.5" />');
    expect(source).toContain('<PanelRight className="size-3.5" />');
    expect(rightIndex).toBeLessThan(html.indexOf('common.minimizeWindow'));
  });

  it('keeps the macOS traffic-light inset on the left and the right workspace control in normal trailing flow', () => {
    const html = renderToStaticMarkup(createElement(AppTitleBar, {
      appName: 'Gold Band',
      platform: 'macos',
      sidebarCollapsed: false,
      onToggleSidebar: () => {},
      onToggleRightWorkspace: () => {},
    }));

    const insetIndex = html.indexOf('pl-[72px]');
    const brandIndex = html.indexOf('data-titlebar-brand="true"');
    const leftIndex = html.indexOf('data-titlebar-sidebar-toggle="left"');
    const rightIndex = html.indexOf('data-titlebar-sidebar-toggle="right"');
    expect(insetIndex).toBeGreaterThanOrEqual(0);
    expect(brandIndex).toBeGreaterThan(insetIndex);
    expect(leftIndex).toBeGreaterThan(brandIndex);
    expect(rightIndex).toBeGreaterThan(leftIndex);
    expect(html).toContain('data-titlebar-trailing-actions="true"');
    expect(html).toContain('pr-2.5');
    expect(html).not.toContain('common.minimizeWindow');
    expect(html).not.toContain('absolute');
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
