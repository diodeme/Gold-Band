import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink, Plug, Router, Terminal, X } from 'lucide-react';
import {
  closeTerminalSession,
  listDetachedWindows,
  listServerConnections,
  listTerminalSessions,
  openDetachedWindow,
  openLocalTerminal,
  openRemoteTerminal,
  pickDirectory,
  provideTerminalAuth,
  subscribeDetachedWindowClosed,
  subscribeTerminalEvents,
} from '@/api';
import { detachedTerminalSessionId } from '@/lib/detached';
import { emitTerminalQuote } from '@/lib/terminal-quote-bus';
import { useFeatureAvailability } from '@/lib/feature-availability';
import { TerminalView } from '@/components/terminal/TerminalView';
import {
  MAX_TERMINAL_SESSIONS,
  isTerminalSessionLimitReached,
  sessionLabel,
  terminalFailedMessage,
} from '@/components/terminal/sessionModel';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { Input } from '@/components/ui/input';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import type { ServerConnectionVm, TerminalEventVm, TerminalSessionVm } from '@/types';

/**
 * The live terminal panel: a session tab bar (local + remote), shared with the
 * backend session model. This is the single terminal surface; it is mounted both
 * as a `terminal` resource in the workspace right dock and (historically) on the
 * standalone connections page. Sessions are always global and shared.
 *
 * Local terminal cwd follows a Q10 fallback: the active workspace path, then the
 * workspace/repo root, then an explicit directory picker.
 */
export function TerminalPanel(
  { workspaceRoot, activeWorkspacePath }: { workspaceRoot?: string | null; activeWorkspacePath?: string | null },
) {
  const { t } = useTranslation();
  const { remoteConnectionsEnabled } = useFeatureAvailability();
  const [sessions, setSessions] = useState<TerminalSessionVm[]>([]);
  const [connections, setConnections] = useState<ServerConnectionVm[]>([]);
  const [detachedIds, setDetachedIds] = useState<Set<string>>(new Set());
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [terminalErrorCode, setTerminalErrorCode] = useState<string | null>(null);
  const [terminalLoading, setTerminalLoading] = useState(false);
  const [authPrompt, setAuthPrompt] = useState<{ sessionId: string; message: string } | null>(null);
  const [authAnswer, setAuthAnswer] = useState('');
  const [authSubmitting, setAuthSubmitting] = useState(false);

  // Keep the latest path inputs readable from async handlers without re-arming effects.
  const pathRef = useRef({ workspaceRoot, activeWorkspacePath });
  pathRef.current = { workspaceRoot, activeWorkspacePath };

  // Sessions opened outside this panel (an agent driving the shared connection
  // MCP tools) surface here through the same event stream; when an event names
  // an unknown session, refresh the list once so its tab appears.
  const sessionsRef = useRef<TerminalSessionVm[]>([]);
  sessionsRef.current = sessions;
  const unknownSessionRefreshInflightRef = useRef(false);

  const loadSessions = useCallback(() => {
    return listTerminalSessions()
      .then((next) => {
        setSessions(next);
        setActiveSessionId((current) => current && next.some((session) => session.sessionId === current)
          ? current
          : next.find((session) => session.status !== 'Closed')?.sessionId ?? null);
      })
      .catch(() => setTerminalErrorCode('terminal.io'));
  }, []);

  const refreshIfUnknownSession = useCallback((sessionId: string) => {
    if (sessionsRef.current.some((session) => session.sessionId === sessionId)) return;
    if (unknownSessionRefreshInflightRef.current) return;
    unknownSessionRefreshInflightRef.current = true;
    void loadSessions().finally(() => {
      unknownSessionRefreshInflightRef.current = false;
    });
  }, [loadSessions]);

  useEffect(() => {
    void loadSessions();
    // Remote connections are intranet-only; on external channels skip loading
    // them entirely (the backend rejects the calls anyway).
    if (remoteConnectionsEnabled) {
      void listServerConnections().then(setConnections).catch(() => {});
    }
  }, [loadSessions, remoteConnectionsEnabled]);

  // A session popped out to its own OS window is hidden from this panel (its tab
  // "disappears"); when that window closes it re-attaches and reappears.
  const refreshDetached = useCallback(() => {
    return listDetachedWindows()
      .then((windows) => {
        setDetachedIds(new Set(windows
          .map((window) => detachedTerminalSessionId(window.label))
          .filter((id): id is string => Boolean(id))));
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    void refreshDetached();
  }, [refreshDetached]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void subscribeTerminalEvents((event: TerminalEventVm) => {
      refreshIfUnknownSession(event.sessionId);
      if (event.event === 'output') {
        setSessions((current) => current.map((session) => session.sessionId === event.sessionId
          ? { ...session, status: session.status === 'Connected' || session.status === 'Closed' ? session.status : 'Connected' }
          : session));
        setAuthPrompt((current) => current?.sessionId === event.sessionId ? null : current);
      } else if (event.event === 'authPrompt') {
        setSessions((current) => current.map((session) => session.sessionId === event.sessionId
          ? { ...session, status: 'AwaitingAuth' }
          : session));
        setAuthPrompt({ sessionId: event.sessionId, message: event.message });
        setActiveSessionId(event.sessionId);
        setAuthAnswer('');
      } else if (event.event === 'failed') {
        setTerminalErrorCode(event.code);
        setSessions((current) => current.filter((session) => session.sessionId !== event.sessionId));
        setActiveSessionId((current) => current === event.sessionId ? null : current);
        setAuthPrompt((current) => current?.sessionId === event.sessionId ? null : current);
      } else if (event.event === 'exited') {
        setSessions((current) => current.filter((session) => session.sessionId !== event.sessionId));
        setActiveSessionId((current) => current === event.sessionId ? null : current);
        setAuthPrompt((current) => current?.sessionId === event.sessionId ? null : current);
      }
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refreshIfUnknownSession]);

  // Re-attach when a detached window closes: refresh the detached set + session
  // list so the session's tab reappears (it never ended, just came back).
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void subscribeDetachedWindowClosed(() => {
      void refreshDetached();
      void loadSessions();
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [refreshDetached, loadSessions]);

  const openLocalSession = async () => {
    if (terminalLoading || isTerminalSessionLimitReached(sessions.length, MAX_TERMINAL_SESSIONS)) return;
    setTerminalLoading(true);
    setTerminalErrorCode(null);
    try {
      // Q10: active workspace path → global repo root → explicit picker.
      const { activeWorkspacePath, workspaceRoot } = pathRef.current;
      const preset = activeWorkspacePath || workspaceRoot;
      const cwd = preset ?? (await pickDirectory());
      if (!cwd) return;
      const session = await openLocalTerminal(cwd);
      setSessions((current) => [...current, session]);
      setActiveSessionId(session.sessionId);
    } catch {
      setTerminalErrorCode('terminal.local-start-failed');
    } finally {
      setTerminalLoading(false);
    }
  };

  const openRemote = async (connection: ServerConnectionVm) => {
    if (terminalLoading || isTerminalSessionLimitReached(sessions.length, MAX_TERMINAL_SESSIONS)) return;
    setTerminalLoading(true);
    setTerminalErrorCode(null);
    try {
      const session = await openRemoteTerminal(connection.id);
      setSessions((current) => [...current, session]);
      setActiveSessionId(session.sessionId);
    } catch (error) {
      const code = (error as { code?: string } | null)?.code ?? 'terminal.connection-lost';
      setTerminalErrorCode(code);
    } finally {
      setTerminalLoading(false);
    }
  };

  const closeSession = async (sessionId: string) => {
    setSessions((current) => current.filter((session) => session.sessionId !== sessionId));
    setActiveSessionId((current) => current === sessionId ? null : current);
    setAuthPrompt((current) => current?.sessionId === sessionId ? null : current);
    try {
      await closeTerminalSession(sessionId);
    } catch {
      // Best-effort; the backend terminates the pty group on app teardown too.
    }
  };

  const submitAuth = async () => {
    if (!authPrompt || !authAnswer.trim() || authSubmitting) return;
    setAuthSubmitting(true);
    try {
      await provideTerminalAuth(authPrompt.sessionId, authAnswer);
      setAuthPrompt(null);
      setAuthAnswer('');
    } catch {
      // The answer was rejected; keep the prompt so the user can retry.
    } finally {
      setAuthSubmitting(false);
    }
  };

  // Sessions currently popped out to their own window are hidden here; they are
  // still live in the backend and reappear when the detached window closes.
  const visibleSessions = sessions.filter((session) => !detachedIds.has(session.sessionId));
  const visibleSessionIds = new Set(visibleSessions.map((session) => session.sessionId));
  const effectiveActiveSessionId = activeSessionId && visibleSessionIds.has(activeSessionId) ? activeSessionId : null;
  const activeSession = visibleSessions.find((session) => session.sessionId === effectiveActiveSessionId) ?? null;
  const limitReached = isTerminalSessionLimitReached(sessions.length, MAX_TERMINAL_SESSIONS);

  // OTP input for the active session: from a live authPrompt event, or derived
  // from the session sitting in AwaitingAuth — the panel may mount after the
  // event fired (an agent-driven session), and the user still needs to type.
  const awaitingAuthSession = visibleSessions.find((session) => session.status === 'AwaitingAuth');
  const visibleAuthPrompt =
    authPrompt && authPrompt.sessionId === effectiveActiveSessionId
      ? authPrompt
      : awaitingAuthSession && awaitingAuthSession.sessionId === effectiveActiveSessionId
        ? { sessionId: awaitingAuthSession.sessionId, message: t('terminal.authPending') }
        : null;

  const openInNewWindow = async (sessionId: string) => {
    await openDetachedWindow('terminal', sessionId);
    await refreshDetached();
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Session tab bar + compact actions in one row. The dock's own tab already
          labels the panel, so there is no separate heading. */}
      <div className="flex items-center gap-2 border-b border-border/60 px-3 pt-1.5">
        {visibleSessions.length > 0 ? (
          <div className="flex min-w-0 flex-1 items-end gap-1 overflow-x-auto">
            {visibleSessions.map((session, index) => {
              const active = session.sessionId === effectiveActiveSessionId;
              return (
                <button
                  key={session.sessionId}
                  type="button"
                  onClick={() => setActiveSessionId(session.sessionId)}
                  className={`group flex shrink-0 items-center gap-2 rounded-t-md border border-b-0 px-2.5 py-1.5 text-xs ${active ? 'border-border bg-muted text-foreground' : 'border-transparent text-muted-foreground hover:bg-muted/60 hover:text-foreground'}`}
                >
                  <Terminal className="size-3" />
                  <span className="max-w-36 truncate">{sessionLabel(t, session, index)}</span>
                  {session.kind === 'remote' && session.status !== 'Connected' ? (
                    <span className="shrink-0 text-[10px] text-amber-600 dark:text-amber-400">
                      {session.status === 'AwaitingAuth' ? t('terminal.awaitingAuth') : t('terminal.connecting')}
                    </span>
                  ) : null}
                  <span
                    role="button"
                    tabIndex={0}
                    className="rounded p-0.5 text-muted-foreground hover:bg-foreground/10 hover:text-foreground"
                    onClick={(event) => { event.stopPropagation(); void openInNewWindow(session.sessionId); }}
                    onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.stopPropagation(); void openInNewWindow(session.sessionId); } }}
                    aria-label={t('terminal.openInNewWindow')}
                  >
                    <ExternalLink className="size-3" />
                  </span>
                  <span
                    role="button"
                    tabIndex={0}
                    className="rounded p-0.5 text-muted-foreground hover:bg-foreground/10 hover:text-foreground"
                    onClick={(event) => { event.stopPropagation(); void closeSession(session.sessionId); }}
                    onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') { event.stopPropagation(); void closeSession(session.sessionId); } }}
                    aria-label={t('terminal.close')}
                  >
                    <X className="size-3" />
                  </span>
                </button>
              );
            })}
          </div>
        ) : (
          <span className="flex-1 text-xs text-muted-foreground">{t('terminal.empty')}</span>
        )}

        <div className="flex shrink-0 items-center gap-1 pb-1.5">
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="size-7"
                onClick={() => void openLocalSession()}
                disabled={terminalLoading || limitReached}
                aria-label={t('terminal.newLocal')}
              >
                <Terminal className="size-3.5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>{t('terminal.newLocal')}</TooltipContent>
          </Tooltip>
          {remoteConnectionsEnabled ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  className="size-7"
                  disabled={terminalLoading || limitReached}
                  aria-label={t('terminal.connect')}
                >
                  <Plug className="size-3.5" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {connections.length === 0 ? (
                  <DropdownMenuItem disabled>{t('connections.management.empty')}</DropdownMenuItem>
                ) : (
                  connections.map((connection) => (
                    <DropdownMenuItem key={connection.id} onClick={() => void openRemote(connection)}>
                      <Router className="size-3.5" />
                      <span className="truncate">{connection.name}</span>
                      <span className="ml-2 text-muted-foreground">{connection.host}:{connection.port}</span>
                    </DropdownMenuItem>
                  ))
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
        </div>
      </div>

      {terminalErrorCode ? (
        <div className="mx-3 mt-2 flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
          <span className="min-w-0 flex-1">{terminalFailedMessage(t, terminalErrorCode)}</span>
          <button type="button" onClick={() => setTerminalErrorCode(null)} aria-label={t('terminal.dismissError')} className="shrink-0 rounded p-0.5 hover:bg-destructive/10">
            <X className="size-3.5" />
          </button>
        </div>
      ) : null}

      <div className="flex min-h-0 flex-1 flex-col">
        {activeSession?.kind === 'local' && activeSession.cwd && effectiveActiveSessionId ? (
          <div className="flex shrink-0 items-center gap-2 border-b border-border/60 px-3 py-1.5">
            <Terminal className="size-3 shrink-0 text-muted-foreground" />
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="min-w-0 truncate font-mono text-xs text-muted-foreground">
                  {activeSession.cwd}
                </span>
              </TooltipTrigger>
              <TooltipContent>{activeSession.cwd}</TooltipContent>
            </Tooltip>
          </div>
        ) : null}
        {visibleAuthPrompt ? (
          <div className="flex shrink-0 items-center gap-2 border-b border-border/60 px-3 py-2">
            <span className="min-w-0 truncate text-xs text-muted-foreground">{visibleAuthPrompt.message}</span>
            <Input
              type="password"
              value={authAnswer}
              onChange={(event) => setAuthAnswer(event.target.value)}
              placeholder={t('terminal.authPromptPlaceholder')}
              className="h-8 max-w-56"
              autoFocus
              disabled={authSubmitting}
              onKeyDown={(event) => { if (event.key === 'Enter') void submitAuth(); }}
            />
            <Button size="sm" className="h-8" onClick={() => void submitAuth()} disabled={!authAnswer.trim() || authSubmitting}>
              {t('terminal.authPromptSubmit')}
            </Button>
          </div>
        ) : null}
        <div className="relative min-h-0 flex-1 bg-background">
          {/* Keep every session's TerminalView mounted so switching tabs never
              re-creates the xterm: each terminal keeps its scrollback and keeps
              receiving its own session output. Inactive terminals are hidden but
              stay live; TerminalView refits + refocuses when it becomes active. */}
          {visibleSessions.map((session) => {
            const active = session.sessionId === effectiveActiveSessionId;
            return (
              <div key={session.sessionId} className={active ? 'h-full' : 'hidden'}>
                <TerminalView
                  sessionId={session.sessionId}
                  active={active}
                  onSelectionQuote={(text) => emitTerminalQuote(text)}
                />
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
