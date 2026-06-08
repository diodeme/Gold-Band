import { useCallback, useEffect, useRef, useState } from 'react';
import type { ConversationPage, ConversationSidebarVm } from '../../types';
import { ConversationSidebar } from './ConversationSidebar';
import { saveConversationPreference } from '../../api';
import { AppTitleBar } from '../AppTitleBar';
import { cn } from '@/lib/utils';

interface ConversationShellProps {
  appName: string;
  vm: ConversationSidebarVm;
  active: ConversationPage;
  sidebarCollapsed: boolean;
  onSelect: (page: ConversationPage) => void;
  onToggleUiMode: () => void;
  onToggleSidebar: () => void;
  onNewConversation: () => void;
  onSearch: () => void;
  onSelectTask: (projectId: string, taskId: string) => void;
  onSelectRun: (projectId: string, taskId: string, runId: string) => void;
  onPinTask: (projectId: string, taskId: string) => void;
  onUnpinTask: (projectId: string, taskId: string) => void;
  onRenameTask: (projectId: string, taskId: string, title: string) => void;
  onNewConversationInWorkspace?: (projectId: string) => void;
  onAddWorkspace?: () => void;
  onRemoveWorkspace?: (projectId: string) => void;
  children: React.ReactNode;
}

const SIDEBAR_MIN = 200;
const SIDEBAR_MAX = 420;
const SIDEBAR_DEFAULT = 256;

function clampWidth(n: number): number {
  return Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, n));
}

function loadSidebarWidth(prefs?: Record<string, unknown> | null): number {
  const fromPrefs = prefs?.['sidebar.width'];
  const fromVm = typeof fromPrefs === 'number' ? clampWidth(fromPrefs) : 0;
  if (fromVm > 0) return fromVm;
  if (typeof localStorage !== 'undefined') {
    const stored = localStorage.getItem('gold-band-sidebar-width');
    if (stored) {
      const n = parseInt(stored, 10);
      if (n >= SIDEBAR_MIN && n <= SIDEBAR_MAX) return n;
    }
  }
  return SIDEBAR_DEFAULT;
}

export function ConversationShell({
  appName,
  vm,
  active,
  sidebarCollapsed,
  onSelect,
  onToggleUiMode,
  onToggleSidebar,
  onNewConversation,
  onSearch,
  onSelectTask,
  onSelectRun,
  onPinTask,
  onUnpinTask,
  onRenameTask,
  onNewConversationInWorkspace,
  onAddWorkspace,
  onRemoveWorkspace,
  children,
}: ConversationShellProps) {
  const [sidebarWidth, setSidebarWidth] = useState(() => loadSidebarWidth(vm.preferences));
  const [resizing, setResizing] = useState(false);
  const startXRef = useRef(0);
  const startWidthRef = useRef(SIDEBAR_DEFAULT);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    startXRef.current = e.clientX;
    startWidthRef.current = sidebarWidth;
    setResizing(true);
  }, [sidebarWidth]);

  const handleMouseMove = useCallback((e: MouseEvent) => {
    const delta = e.clientX - startXRef.current;
    const next = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startWidthRef.current + delta));
    setSidebarWidth(next);
  }, []);

  const finalWidthRef = useRef(SIDEBAR_DEFAULT);

  const handleMouseUp = useCallback(() => {
    setResizing(false);
    setSidebarWidth((current) => {
      finalWidthRef.current = current;
      if (typeof localStorage !== 'undefined') {
        localStorage.setItem('gold-band-sidebar-width', String(current));
      }
      return current;
    });
  }, []);

  // Persist to backend after resize ends
  useEffect(() => {
    if (resizing) return;
    saveConversationPreference('sidebar.width', finalWidthRef.current).catch(() => {});
  }, [resizing]);

  useEffect(() => {
    if (!resizing) return;
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [resizing, handleMouseMove, handleMouseUp]);

  return (
    <div
      className={cn('flex h-screen flex-col bg-gold-workspace text-foreground', resizing && 'select-none cursor-col-resize')}
      onContextMenu={(event) => event.preventDefault()}
    >
      <AppTitleBar
        appName={appName}
        uiMode="conversation"
        sidebarCollapsed={sidebarCollapsed}
        onToggleSidebar={onToggleSidebar}
        onToggleUiMode={onToggleUiMode}
      />
      <div className="flex min-h-0 flex-1">
        <div
          className={cn(
            'relative h-full shrink-0 overflow-hidden transition-[width] duration-250 ease-out',
            sidebarCollapsed && 'pointer-events-none',
          )}
          style={{ width: sidebarCollapsed ? 0 : sidebarWidth }}
        >
          <div
            className={cn(
              'h-full transition-opacity duration-200 ease-out',
              sidebarCollapsed ? 'opacity-0' : 'opacity-100',
            )}
            style={{ width: sidebarWidth }}
          >
            <ConversationSidebar
              vm={vm}
              active={active}
              onSelect={onSelect}
              onToggleUiMode={onToggleUiMode}
              onNewConversation={onNewConversation}
              onSearch={onSearch}
              onSelectTask={onSelectTask}
              onSelectRun={onSelectRun}
              onPinTask={onPinTask}
              onUnpinTask={onUnpinTask}
              onRenameTask={onRenameTask}
              onNewConversationInWorkspace={onNewConversationInWorkspace}
              onAddWorkspace={onAddWorkspace}
              onRemoveWorkspace={onRemoveWorkspace}
            />
          </div>
          <div
            className={cn(
              'absolute right-0 top-0 bottom-0 z-20 w-1 cursor-col-resize transition-colors hover:bg-primary/40 active:bg-primary/60',
              sidebarCollapsed && 'pointer-events-none opacity-0',
            )}
            onMouseDown={handleMouseDown}
          />
        </div>
        <main className="relative flex min-w-0 flex-1 flex-col overflow-hidden bg-gold-workspace">{children}</main>
      </div>
    </div>
  );
}
