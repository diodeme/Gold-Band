import { useTranslation } from 'react-i18next';
import { ArrowDownToLine, Plug } from 'lucide-react';
import { closeDetachedWindow } from '@/api';
import { isTauriRuntime } from '@/api/shared';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { DetachedTerminal } from '@/components/terminal/DetachedTerminal';
import { ConnectionsTerminalPage } from '@/pages/ConnectionsTerminalPage';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { DetachedWindowKind } from '@/types';

function currentWindowLabel(): string {
  return isTauriRuntime() ? getCurrentWindow().label : '';
}

/**
 * The UI tree rendered only in a pop-out "detached" webview window (not the main
 * one). `main.tsx` renders this instead of the full `<App/>` shell when it
 * detects `?window=...` in the URL. It hosts exactly one resource (an isolated
 * terminal session or the connections page) with a slim title bar that offers
 * "return to main window".
 */
export function DetachedApp({ kind, sessionId }: { kind: DetachedWindowKind; sessionId: string | null }) {
  const { t } = useTranslation();

  const returnToMain = async () => {
    await closeDetachedWindow(currentWindowLabel());
  };

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 bg-muted/40 px-3" data-detached-titlebar>
        <Plug className="size-3.5 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate text-xs font-medium">
          {kind === 'connections' ? t('connections.management.title') : t('terminal.panelTitle')}
        </span>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button variant="ghost" size="icon" className="size-7" onClick={() => void returnToMain()} aria-label={t('terminal.returnToMain')}>
              <ArrowDownToLine className="size-3.5" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t('terminal.returnToMain')}</TooltipContent>
        </Tooltip>
      </div>
      <div className="min-h-0 flex-1">
        {kind === 'connections' ? (
          <ConnectionsTerminalPage />
        ) : (
          <DetachedTerminal sessionId={sessionId ?? ''} />
        )}
      </div>
    </div>
  );
}
