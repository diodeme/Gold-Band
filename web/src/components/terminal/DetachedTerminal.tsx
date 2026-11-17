import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { ArrowDownToLine, Terminal as TerminalIcon, X } from 'lucide-react';
import { closeDetachedWindow, closeTerminalSession, listTerminalSessions } from '@/api';
import { isTauriRuntime } from '@/api/shared';
import { TerminalView } from '@/components/terminal/TerminalView';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { TerminalSessionVm } from '@/types';

function currentWindowLabel(): string {
  return isTauriRuntime() ? getCurrentWindow().label : '';
}

/**
 * Pop-out window hosting a single terminal session. It renders the SAME global
 * session (the backend state + event broadcast are window-agnostic) through the
 * reusable `<TerminalView>`. The session stays alive while the window is open.
 *
 * Two distinct actions (semantics per the agreed design):
 * - "收回到主窗口" (return-to-main): just closes this window; the session keeps
 *   running and the main dock tab reappears (it never ended).
 * - The native OS close (✕) also closes the window, but the session is NOT
 *   automatically ended here — the session ends when it is closed in the main
 *   window or via the terminal's own close action. We surface a "close this
 *   session" affordance here so the user can end it from the detached window too.
 */
export function DetachedTerminal({ sessionId, label }: { sessionId: string; label?: string | null }) {
  const { t } = useTranslation();
  const [session, setSession] = useState<TerminalSessionVm | null>(null);
  const [notFound, setNotFound] = useState(false);

  useEffect(() => {
    let disposed = false;
    void listTerminalSessions()
      .then((sessions) => {
        if (disposed) return;
        const found = sessions.find((s) => s.sessionId === sessionId) ?? null;
        setSession(found);
        setNotFound(!found);
      })
      .catch(() => {
        if (!disposed) setNotFound(true);
      });
    return () => {
      disposed = true;
    };
  }, [sessionId]);

  const returnToMain = async () => {
    // Session keeps running; the main window refreshes and the dock tab is
    // restored. Just close this window.
    await closeDetachedWindow(currentWindowLabel());
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 bg-muted/40 px-3" data-detached-terminal-titlebar>
        <TerminalIcon className="size-3.5 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate text-xs font-medium text-foreground">
          {label ?? session?.label ?? t('terminal.panelTitle')}
        </span>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="size-7" onClick={() => void returnToMain()} aria-label={t('terminal.returnToMain')}>
              <ArrowDownToLine className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t('terminal.returnToMain')}</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="size-7" onClick={() => void closeTerminalSession(sessionId)} aria-label={t('terminal.close')}>
              <X className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t('terminal.close')}</TooltipContent>
        </Tooltip>
      </div>

      <div className="min-h-0 flex-1 bg-background">
        {notFound ? (
          <div className="flex h-full items-center justify-center px-4 text-sm text-muted-foreground">{t('terminal.detachedSessionNotFound')}</div>
        ) : (
          <TerminalView sessionId={sessionId} active />
        )}
      </div>
    </div>
  );
}
