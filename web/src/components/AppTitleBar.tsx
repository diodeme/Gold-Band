import { useEffect, useState } from 'react';
import type { MouseEvent as ReactMouseEvent } from 'react';
import { Copy, Minus, PanelLeft, Square, X } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useTranslation } from 'react-i18next';
import type { DesktopPlatform, DesktopUiMode } from '../types';
import { isTauriRuntime } from '../api/shared';
import { resolveWindowControlsPolicy } from '../lib/window-controls';
import { Button } from '@/components/ui/button';
import { cn } from '@/lib/utils';

const titleBarNonDragSelector = 'button, a, input, textarea, select, [role="button"], [data-titlebar-no-drag="true"]';

interface AppTitleBarProps {
  appName: string;
  platform?: DesktopPlatform | null;
  uiMode: DesktopUiMode;
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  onToggleUiMode: () => void;
}

export function AppTitleBar({
  appName,
  platform,
  uiMode,
  sidebarCollapsed,
  onToggleSidebar,
  onToggleUiMode,
}: AppTitleBarProps) {
  const { t } = useTranslation();
  const [isMaximized, setIsMaximized] = useState(false);
  const tauriRuntime = isTauriRuntime();
  const policy = resolveWindowControlsPolicy(platform);

  useEffect(() => {
    if (!tauriRuntime) return undefined;
    const appWindow = getCurrentWindow();
    let active = true;
    let unlisten: (() => void) | undefined;

    const syncMaximized = () => {
      appWindow.isMaximized().then((value) => {
        if (active) setIsMaximized(value);
      }).catch(() => {});
    };

    syncMaximized();
    appWindow.onResized(() => {
      syncMaximized();
    }).then((dispose) => {
      if (active) {
        unlisten = dispose;
      } else {
        dispose();
      }
    }).catch(() => {});

    return () => {
      active = false;
      unlisten?.();
    };
  }, [tauriRuntime]);

  const handleMinimize = () => {
    if (!tauriRuntime) return;
    getCurrentWindow().minimize().catch(() => {});
  };

  const handleToggleMaximize = () => {
    if (!tauriRuntime) return;
    getCurrentWindow().toggleMaximize().then(() => {
      setIsMaximized((value) => !value);
    }).catch(() => {});
  };

  const handleClose = () => {
    if (!tauriRuntime) return;
    getCurrentWindow().close().catch(() => {});
  };

  const handleTitleBarDoubleClick = (event: ReactMouseEvent<HTMLElement>) => {
    const target = event.target as HTMLElement;
    if (target.closest(titleBarNonDragSelector)) return;
    handleToggleMaximize();
  };

  const handleDragMouseDown = (event: ReactMouseEvent<HTMLElement>) => {
    if (!tauriRuntime || event.button !== 0 || event.detail > 1) return;
    const target = event.target as HTMLElement;
    if (target.closest(titleBarNonDragSelector)) return;
    getCurrentWindow().startDragging().catch(() => {});
  };

  const modeLabel = uiMode === 'conversation' ? t('common.conversation') : t('common.workbench');
  const hasLeadingInset = policy.leadingInsetClassName.length > 0;

  return (
    <header
      data-tauri-drag-region
      className="app-titlebar-drag-region flex h-11 shrink-0 select-none items-center bg-titlebar text-titlebar-foreground"
      onDoubleClick={handleTitleBarDoubleClick}
      onMouseDown={handleDragMouseDown}
    >
      <div className="flex items-center gap-2 px-2.5">
        {hasLeadingInset ? <div aria-hidden="true" className={cn('shrink-0', policy.leadingInsetClassName)} /> : null}
        <Button
          variant="ghost"
          size="icon"
          className="app-titlebar-no-drag size-8 rounded-md text-titlebar-muted hover:bg-titlebar-hover hover:text-titlebar-foreground"
          onClick={onToggleSidebar}
          aria-label={sidebarCollapsed ? t('common.showSidebar') : t('common.collapseSidebar')}
          title={sidebarCollapsed ? t('common.showSidebar') : t('common.collapseSidebar')}
          data-titlebar-no-drag="true"
        >
          <PanelLeft className="size-4" />
        </Button>
        <div data-tauri-drag-region className="flex h-full items-center gap-2 pr-2">
          <span data-tauri-drag-region className="grid h-7 w-10 shrink-0 place-items-center rounded-lg border border-titlebar-border bg-background/55 p-1">
            <img src="/logo.svg" alt="" className="h-full w-full object-contain pointer-events-none" />
          </span>
          <span data-tauri-drag-region className="text-sm font-semibold tracking-[0.01em] text-titlebar-foreground">
            {appName}
          </span>
        </div>
      </div>

      <div className="app-titlebar-no-drag flex items-center gap-1 rounded-lg border border-titlebar-border bg-background/40 p-0.5" data-titlebar-no-drag="true">
        <button
          type="button"
          className={cn(
            'rounded-md px-3 py-1 text-xs font-medium transition-colors',
            uiMode === 'conversation'
              ? 'bg-primary/14 text-primary'
              : 'text-titlebar-muted hover:bg-titlebar-hover hover:text-titlebar-foreground',
          )}
          onClick={uiMode === 'conversation' ? undefined : onToggleUiMode}
          aria-pressed={uiMode === 'conversation'}
        >
          {t('common.conversation')}
        </button>
        <button
          type="button"
          className={cn(
            'rounded-md px-3 py-1 text-xs font-medium transition-colors',
            uiMode === 'workbench'
              ? 'bg-primary/14 text-primary'
              : 'text-titlebar-muted hover:bg-titlebar-hover hover:text-titlebar-foreground',
          )}
          onClick={uiMode === 'workbench' ? undefined : onToggleUiMode}
          aria-pressed={uiMode === 'workbench'}
        >
          {t('common.workbench')}
        </button>
      </div>

      <div
        data-tauri-drag-region
        className="min-w-0 flex-1 self-stretch"
        aria-label={modeLabel}
      />

      {policy.showCustomControls ? (
        <div className="app-titlebar-no-drag flex h-full items-stretch pl-2" data-titlebar-no-drag="true">
          <button
            type="button"
            className="flex h-full w-11 items-center justify-center text-titlebar-muted transition-colors hover:bg-titlebar-hover hover:text-titlebar-foreground"
            onClick={handleMinimize}
            aria-label={t('common.minimizeWindow')}
            title={t('common.minimizeWindow')}
          >
            <Minus className="size-4" />
          </button>
          <button
            type="button"
            className="flex h-full w-11 items-center justify-center text-titlebar-muted transition-colors hover:bg-titlebar-hover hover:text-titlebar-foreground"
            onClick={handleToggleMaximize}
            aria-label={isMaximized ? t('common.restoreWindow') : t('common.maximizeWindow')}
            title={isMaximized ? t('common.restoreWindow') : t('common.maximizeWindow')}
          >
            {isMaximized ? <Copy className="size-3.5" /> : <Square className="size-3.5" />}
          </button>
          <button
            type="button"
            className="flex h-full w-12 items-center justify-center text-titlebar-muted transition-colors hover:bg-destructive hover:text-white"
            onClick={handleClose}
            aria-label={t('common.closeWindow')}
            title={t('common.closeWindow')}
          >
            <X className="size-4" />
          </button>
        </div>
      ) : null}
    </header>
  );
}
