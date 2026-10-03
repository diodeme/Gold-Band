import { useEffect, useRef, useState } from 'react';
import { BarChart3, Copy, MessageSquareWarning, Minus, NotebookText, PanelLeft, PanelRight, Square, X } from 'lucide-react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useTranslation } from 'react-i18next';
import type { DesktopPlatform } from '../types';
import { isTauriRuntime } from '../api/shared';
import { resolveWindowControlsPolicy } from '../lib/window-controls';
import { Button } from '@/components/ui/button';
import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { FeedbackDialog } from './feedback/FeedbackDialog';
import { cn } from '@/lib/utils';

interface AppTitleBarProps {
  trailingContent?: React.ReactNode;
  updateAction?: React.ReactNode;
  appName: string;
  feedbackEnabled?: boolean;
  platform?: DesktopPlatform | null;
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  rightWorkspaceOpen?: boolean;
  onToggleRightWorkspace?: () => void;
  onOpenPersonalAnalytics?: () => void;
  onOpenReleaseNotes?: () => void;
}

export const APP_TITLE_BAR_LAYOUT = {
  rootClassName: 'app-titlebar-drag-region flex h-9 shrink-0 select-none items-center bg-titlebar text-titlebar-foreground',
  brandMarkClassName: 'grid size-7 shrink-0 place-items-center rounded-[7px] border border-titlebar-border bg-background/55 p-0.5',
  brandTitleClassName: 'text-base font-[700] tracking-[0.01em] text-titlebar-foreground',
  helpActionClassName: 'flex h-7 items-center rounded-md px-2.5 text-sm font-medium text-titlebar-muted transition-colors hover:bg-titlebar-hover hover:text-titlebar-foreground',
} as const;

export function AppTitleBar({
  trailingContent,
  updateAction,
  appName,
  feedbackEnabled = false,
  platform,
  sidebarCollapsed,
  onToggleSidebar,
  rightWorkspaceOpen = false,
  onToggleRightWorkspace,
  onOpenPersonalAnalytics,
  onOpenReleaseNotes,
}: AppTitleBarProps) {
  const readOnly = useReadOnlyExperience();
  const { t } = useTranslation();
  const [windowState, setWindowState] = useState({ maximized: false, fullscreen: false });
  const isMaximized = windowState.maximized;
  const [feedbackOpen, setFeedbackOpen] = useState(false);
  const [helpMenuOpen, setHelpMenuOpen] = useState(false);
  const helpNavigationPendingRef = useRef(false);
  const hasHelpMenu = feedbackEnabled || Boolean(onOpenPersonalAnalytics) || Boolean(onOpenReleaseNotes);
  // Close the menu first so the opened surface keeps focus instead of the help trigger.
  const openFromHelpMenu = (open: () => void) => {
    helpNavigationPendingRef.current = true;
    setHelpMenuOpen(false);
    requestAnimationFrame(open);
  };
  const syncWindowStateRef = useRef<() => void>(() => {});
  const tauriRuntime = isTauriRuntime();
  const policy = resolveWindowControlsPolicy(platform, readOnly ? 'browser' : 'desktop');

  useEffect(() => {
    if (!tauriRuntime) return undefined;
    const appWindow = getCurrentWindow();
    let active = true;
    let revision = 0;
    let unlisten: (() => void) | undefined;

    const syncWindowState = () => {
      const requestedRevision = ++revision;
      Promise.all([appWindow.isMaximized(), appWindow.isFullscreen()]).then(([maximized, fullscreen]) => {
        if (active && revision === requestedRevision) {
          setWindowState((previous) => previous.maximized === maximized && previous.fullscreen === fullscreen
            ? previous
            : { maximized, fullscreen });
        }
      }).catch(() => {});
    };

    syncWindowStateRef.current = syncWindowState;
    syncWindowState();
    appWindow.onResized(() => {
      syncWindowState();
    }).then((dispose) => {
      if (active) {
        unlisten = dispose;
      } else {
        dispose();
      }
    }).catch(() => {});

    return () => {
      active = false;
      syncWindowStateRef.current = () => {};
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
      syncWindowStateRef.current();
    }).catch(() => {});
  };

  const handleClose = () => {
    if (!tauriRuntime) return;
    getCurrentWindow().close().catch(() => {});
  };

  const hasLeadingInset = policy.leadingInsetClassName.length > 0;

  return (
    <>
    <header
      data-tauri-drag-region
      className={APP_TITLE_BAR_LAYOUT.rootClassName}
      data-theme-role="titlebar"
      data-window-occludes-desktop={windowState.maximized || windowState.fullscreen}
    >
      <div className="flex items-center px-2.5">
        {hasLeadingInset ? <div aria-hidden="true" className={cn('shrink-0', policy.leadingInsetClassName)} /> : null}
        <div data-tauri-drag-region data-titlebar-brand="true" className="flex h-full items-center gap-2 pr-3">
          <span data-tauri-drag-region className={APP_TITLE_BAR_LAYOUT.brandMarkClassName}>
            <img src="/logo.svg" alt="" className="block size-full min-h-0 min-w-0 object-contain pointer-events-none" />
          </span>
          <span data-tauri-drag-region className={APP_TITLE_BAR_LAYOUT.brandTitleClassName}>
            {appName}
          </span>
        </div>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="ghost"
              size="icon"
              className={cn(
                'app-titlebar-no-drag size-7 rounded-[6px] text-titlebar-muted hover:bg-titlebar-hover hover:text-titlebar-foreground',
                !sidebarCollapsed && 'bg-titlebar-hover/70 text-titlebar-foreground',
              )}
              onClick={onToggleSidebar}
              aria-label={sidebarCollapsed ? t('common.showSidebar') : t('common.collapseSidebar')}
              data-titlebar-no-drag="true"
              data-titlebar-sidebar-toggle="left"
              data-state={sidebarCollapsed ? 'closed' : 'open'}
            >
              <PanelLeft className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{sidebarCollapsed ? t('common.showSidebar') : t('common.collapseSidebar')}</TooltipContent>
        </Tooltip>
      </div>

      <div
        data-tauri-drag-region
        className="min-w-0 flex-1 self-stretch"
      />

      {trailingContent}
      {hasHelpMenu || onToggleRightWorkspace || updateAction ? (
        <div
          className={cn(
            'app-titlebar-no-drag flex h-full flex-none items-center gap-0.5',
            !policy.showCustomControls && 'pr-2.5',
          )}
          data-titlebar-no-drag="true"
          data-titlebar-trailing-actions="true"
        >
          {!readOnly && hasHelpMenu ? (
            <DropdownMenu open={helpMenuOpen} onOpenChange={setHelpMenuOpen}>
              <DropdownMenuTrigger asChild>
                <button type="button" className={APP_TITLE_BAR_LAYOUT.helpActionClassName}>
                  {t('common.help')}
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                align="end"
                className="min-w-40"
                onCloseAutoFocus={(event) => {
                  if (!helpNavigationPendingRef.current) return;
                  helpNavigationPendingRef.current = false;
                  event.preventDefault();
                }}
              >
                {onOpenPersonalAnalytics ? (
                  <DropdownMenuItem onSelect={() => openFromHelpMenu(onOpenPersonalAnalytics)} className="gap-2">
                    <BarChart3 className="size-4" />
                    {t('common.personalAnalytics')}
                  </DropdownMenuItem>
                ) : null}
                {feedbackEnabled ? (
                <DropdownMenuItem onSelect={() => openFromHelpMenu(() => setFeedbackOpen(true))} className="gap-2">
                  <MessageSquareWarning className="size-4" />
                  {t('common.userFeedback')}
                </DropdownMenuItem>
                ) : null}
                {onOpenReleaseNotes ? (
                  <DropdownMenuItem onSelect={() => openFromHelpMenu(onOpenReleaseNotes)} className="gap-2">
                    <NotebookText className="size-4" />
                    {t('common.releaseNotes')}
                  </DropdownMenuItem>
                ) : null}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
          {updateAction}
          {onToggleRightWorkspace ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className={cn(
                    'size-7 rounded-[6px] text-titlebar-muted hover:bg-titlebar-hover hover:text-titlebar-foreground',
                    rightWorkspaceOpen && 'bg-titlebar-hover/70 text-titlebar-foreground',
                  )}
                  onClick={onToggleRightWorkspace}
                  aria-label={rightWorkspaceOpen ? t('workspace.closeWorkspace') : t('workspace.openWorkspace')}
                  data-titlebar-sidebar-toggle="right"
                  data-state={rightWorkspaceOpen ? 'open' : 'closed'}
                >
                  <PanelRight className="size-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{rightWorkspaceOpen ? t('workspace.closeWorkspace') : t('workspace.openWorkspace')}</TooltipContent>
            </Tooltip>
          ) : null}
        </div>
      ) : null}
      {policy.showCustomControls ? (
        <div
          className="app-titlebar-no-drag flex h-full w-max flex-none items-stretch pl-2"
          data-titlebar-no-drag="true"
        >
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="flex h-full w-11 flex-none items-center justify-center text-titlebar-muted transition-colors hover:bg-titlebar-hover hover:text-titlebar-foreground"
                onClick={handleMinimize}
                aria-label={t('common.minimizeWindow')}
              >
                <Minus className="size-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent>{t('common.minimizeWindow')}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="flex h-full w-11 flex-none items-center justify-center text-titlebar-muted transition-colors hover:bg-titlebar-hover hover:text-titlebar-foreground"
                onClick={handleToggleMaximize}
                aria-label={isMaximized ? t('common.restoreWindow') : t('common.maximizeWindow')}
              >
                {isMaximized ? <Copy className="size-3.5" /> : <Square className="size-3.5" />}
              </button>
            </TooltipTrigger>
            <TooltipContent>{isMaximized ? t('common.restoreWindow') : t('common.maximizeWindow')}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                className="flex h-full w-12 flex-none items-center justify-center text-titlebar-muted transition-colors hover:bg-destructive hover:text-white"
                onClick={handleClose}
                aria-label={t('common.closeWindow')}
              >
                <X className="size-4" />
              </button>
            </TooltipTrigger>
            <TooltipContent>{t('common.closeWindow')}</TooltipContent>
          </Tooltip>
        </div>
      ) : null}
    </header>
      <FeedbackDialog open={feedbackOpen} onOpenChange={setFeedbackOpen} />
    </>
  );
}
