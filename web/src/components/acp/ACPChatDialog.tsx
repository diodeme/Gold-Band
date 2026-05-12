import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { StickToBottomContext } from 'use-stick-to-bottom';
import { Bot, CheckCircle2, CircleAlert, Clock, FileText, Loader2, Search, Send, Terminal, User } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { ChatContainerContent, ChatContainerRoot, ChatContainerScrollAnchor } from '@/components/prompt-kit/chat-container';
import { ChainOfThought, ChainOfThoughtContent, ChainOfThoughtItem, ChainOfThoughtStep, ChainOfThoughtTrigger } from '@/components/prompt-kit/chain-of-thought';
import { Message, MessageContent } from '@/components/prompt-kit/message';
import { PromptInput, PromptInputActions, PromptInputAction, PromptInputTextarea } from '@/components/prompt-kit/prompt-input';
import { Tool, type ToolLabels, type ToolPart } from '@/components/prompt-kit/tool';
import { cn } from '@/lib/utils';
import { getAcpRawFrames, getAcpSession, respondAcpPermission, sendAcpPrompt } from '@/api';
import { displayStatus } from '@/i18n';
import type { AcpPermissionRequestVm, AcpSessionVm, AcpUiEventVm } from '@/types';

interface ACPChatDialogProps {
  session?: AcpSessionVm | null;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
}

type AcpCanvasMode = 'chat' | 'raw';
type ToolTone = 'muted' | 'pending' | 'running' | 'success' | 'danger';
type AcpProcessingKind = 'launching' | 'processing' | 'thinking' | 'tool' | 'responding';
type AcpTimelineEvent = AcpUiEventVm & {
  startedAt?: string;
  endedAt?: string;
  durationMs?: number;
  optimistic?: boolean;
};

const hiddenSessionUpdates = new Set([
  'available_commands_update',
  'usage_update',
  'session_info_update',
  'current_mode_update',
  'config_option_update',
]);

const hiddenEventKinds = new Set([
  'availableCommands',
  'usageUpdate',
  'sessionInfo',
  'modeUpdate',
  'configUpdate',
  'permissionRequest',
  'rawDiagnostic',
]);

export function ACPChatDialog({ session, taskId, runId, roundId, nodeId, attemptId }: ACPChatDialogProps) {
  const { t } = useTranslation();
  const [currentSession, setCurrentSession] = useState<AcpSessionVm | null>(session ?? null);
  const [optimisticEvents, setOptimisticEvents] = useState<AcpUiEventVm[]>([]);
  const [prompt, setPrompt] = useState('');
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);
  const [canvasMode, setCanvasMode] = useState<AcpCanvasMode>('chat');
  const [rawFrames, setRawFrames] = useState<string | null>(null);
  const [rawLoading, setRawLoading] = useState(false);
  const [dismissedPermissionIds, setDismissedPermissionIds] = useState<Set<string>>(() => new Set());
  const [permissionError, setPermissionError] = useState<string | null>(null);
  const scrollContextRef = useRef<StickToBottomContext | null>(null);
  const sessionKey = `${taskId}:${runId}:${roundId}:${nodeId}:${attemptId}`;

  useEffect(() => {
    setCurrentSession(session ?? null);
  }, [session]);

  useEffect(() => {
    setOptimisticEvents([]);
    setDismissedPermissionIds(new Set());
    setPermissionError(null);
    setSendError(null);
    setRawFrames(null);
    setCanvasMode('chat');
  }, [sessionKey]);

  useEffect(() => {
    if (!sending) return;
    let active = true;
    const timer = window.setInterval(async () => {
      try {
        const updated = await getAcpSession(taskId, runId, roundId, nodeId, attemptId, currentSession ?? session ?? null);
        if (active && updated) setCurrentSession(updated);
      } catch {
        // The send request owns user-visible error handling.
      }
    }, 1500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [attemptId, currentSession, nodeId, roundId, runId, sending, session, taskId]);

  const baseSession = currentSession ?? session;
  const effective = useMemo(() => mergeOptimisticSession(baseSession, optimisticEvents), [baseSession, optimisticEvents]);
  const pendingPermission = effective?.pendingPermissions?.find((request) => !dismissedPermissionIds.has(request.requestId)) ?? null;
  const lastEvent = effective?.events.at(-1);
  const eventScrollSignature = `${effective?.events.length ?? 0}:${lastEvent?.seq ?? ''}:${lastEvent?.kind ?? ''}:${sending}`;

  useEffect(() => {
    if (canvasMode !== 'chat') return;
    void scrollContextRef.current?.scrollToBottom({ preserveScrollPosition: true });
  }, [canvasMode, eventScrollSignature]);

  const preserveScrollPosition = () => {
    const context = scrollContextRef.current;
    const scrollElement = context?.scrollRef.current;
    if (!context || !scrollElement) return;
    const scrollTop = scrollElement.scrollTop;
    context.stopScroll();
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        scrollElement.scrollTop = scrollTop;
        context.stopScroll();
      });
    });
  };

  const send = async () => {
    const trimmed = prompt.trim();
    if (!trimmed || pendingPermission) return;
    const optimisticEvent = optimisticUserEvent(trimmed);
    setPrompt('');
    setSendError(null);
    setOptimisticEvents((current) => [...current, optimisticEvent]);
    setSending(true);
    try {
      const updated = await sendAcpPrompt(taskId, runId, roundId, nodeId, attemptId, trimmed, effective ?? null);
      setCurrentSession(updated);
      setOptimisticEvents((current) => current.filter((event) => !hasMatchingUserPrompt(updated?.events ?? [], event)));
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      setSendError(message);
      setOptimisticEvents((current) => current.map((event) => event.id === optimisticEvent.id ? { ...event, status: 'failed' } : event));
    } finally {
      setSending(false);
    }
  };

  const answerPermission = async (request: AcpPermissionRequestVm, optionId: string) => {
    setPermissionError(null);
    setDismissedPermissionIds((current) => new Set(current).add(request.requestId));
    try {
      const updated = await respondAcpPermission(taskId, runId, roundId, nodeId, attemptId, request.requestId, optionId, effective);
      setCurrentSession(updated);
    } catch (error) {
      setDismissedPermissionIds((current) => {
        const next = new Set(current);
        next.delete(request.requestId);
        return next;
      });
      setPermissionError(error instanceof Error ? error.message : String(error));
    }
  };

  const toggleRawFrames = async () => {
    preserveScrollPosition();
    if (canvasMode === 'raw') {
      setCanvasMode('chat');
      return;
    }
    if (rawFrames == null) {
      setRawLoading(true);
      try {
        const raw = await getAcpRawFrames(taskId, runId, roundId, nodeId, attemptId);
        setRawFrames(raw.content || t('common.empty'));
      } finally {
        setRawLoading(false);
      }
    }
    setCanvasMode('raw');
  };

  if (!effective) {
    return <AcpErrorState reason={t('acp.missingSessionReason')} />;
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-background">
      <ACPSessionHeader session={effective} rawActive={canvasMode === 'raw'} rawLoading={rawLoading} onToggleRaw={toggleRawFrames} />
      {effective.diagnostics.lastError ? <AcpErrorBanner reason={effective.diagnostics.lastError} /> : null}
      <ChatContainerRoot resize="instant" initial="instant" contextRef={scrollContextRef} className="min-h-0 min-w-0 max-w-full flex-1 overflow-x-hidden">
        <ChatContainerContent className="w-full min-w-0 max-w-full space-y-4 overflow-hidden p-5">
          {canvasMode === 'raw' ? (
            <RawFrameViewer content={rawFrames ?? ''} loading={rawLoading} onLayoutChange={preserveScrollPosition} />
          ) : (
            <>
              <ACPMessageList events={effective.events} sessionStatus={effective.status} sending={sending} onLayoutChange={preserveScrollPosition} />
              {sendError ? <AcpErrorBanner reason={`${t('acp.sendFailed')}：${sendError}`} /> : null}
              {permissionError ? <AcpErrorBanner reason={permissionError} /> : null}
              {pendingPermission ? <PermissionRequestCard request={pendingPermission} onSelect={(optionId) => answerPermission(pendingPermission, optionId)} /> : null}
              <ChatContainerScrollAnchor />
            </>
          )}
        </ChatContainerContent>
      </ChatContainerRoot>
      {canvasMode === 'chat' ? (
        <div className="shrink-0 border-t bg-background/95 p-4 backdrop-blur">
          <PromptInput
            value={prompt}
            onValueChange={setPrompt}
            onSubmit={send}
            isLoading={sending}
            disabled={Boolean(pendingPermission)}
            className="rounded-2xl bg-card/80 shadow-sm shadow-background/30 transition-colors focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/10"
          >
            <PromptInputTextarea
              className="min-h-16 text-sm leading-6 text-foreground placeholder:text-muted-foreground"
              placeholder={pendingPermission ? t('acp.permissionPending') : t('acp.composerPlaceholder')}
            />
            <div className="flex items-center justify-between gap-2 px-2 pb-1">
              <span className="text-xs text-muted-foreground">{sending ? t('acp.sending') : t('acp.promptInputHint')}</span>
              <PromptInputActions>
                <PromptInputAction tooltip={t('acp.send')}>
                  <Button className="h-8 gap-1.5 rounded-full px-3" size="sm" disabled={sending || !prompt.trim() || Boolean(pendingPermission)} onClick={send}>
                    {sending ? <Loader2 className="size-3.5 animate-spin" /> : <Send className="size-3.5" />}
                    {t('acp.send')}
                  </Button>
                </PromptInputAction>
              </PromptInputActions>
            </div>
          </PromptInput>
        </div>
      ) : null}
    </div>
  );
}

function AcpErrorState({ reason }: { reason: string }) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <AcpErrorBanner reason={reason} />
      <div className="flex-1" />
    </div>
  );
}

function AcpErrorBanner({ reason }: { reason: string }) {
  const { t } = useTranslation();
  return (
    <div className="shrink-0 border-b border-destructive/20 bg-destructive/5 px-5 py-3 text-sm">
      <span className="font-semibold text-destructive">{t('acp.sessionFailed')}</span>
      <span className="ml-2 text-muted-foreground">{reason}</span>
    </div>
  );
}

export function ACPSessionHeader({ session, rawActive, rawLoading, onToggleRaw }: { session: AcpSessionVm; rawActive: boolean; rawLoading: boolean; onToggleRaw: () => void }) {
  const { t } = useTranslation();
  return (
    <div className="shrink-0 border-b bg-muted/10 px-5 py-3">
      <div className="min-w-0 space-y-1.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className="min-w-0 truncate text-base font-semibold">{session.adapterDisplayName ?? session.provider}</span>
          <StatusBadge status={session.status} tone={toolStatusTone(session.status)} />
          <Button size="sm" variant={rawActive ? 'default' : 'outline'} className="ml-auto h-7 gap-1.5 px-2.5 text-xs" onClick={onToggleRaw} disabled={rawLoading}>
            {rawLoading ? <Loader2 className="size-3 animate-spin" /> : null}
            {t('acp.rawFrames')}
          </Button>
        </div>
        <div className="truncate text-xs text-muted-foreground">{session.sessionId ?? t('acp.noSessionId')}</div>
      </div>
    </div>
  );
}

export function ACPMessageList({ events, sessionStatus, sending, onLayoutChange }: { events: AcpUiEventVm[]; sessionStatus: string; sending: boolean; onLayoutChange?: () => void }) {
  const timeline = useMemo(() => buildAcpTimeline(events), [events]);
  const active = isSessionActive(sessionStatus) || sending;
  const totalStartAt = events[0]?.timestamp ?? timeline[0]?.timestamp ?? null;
  const latestEvent = timeline.at(-1) ?? null;

  if (timeline.length === 0) {
    return active ? (
      <AssistantTimelineRow>
        <AcpProcessingStatus kind="launching" active={active} startAt={totalStartAt} totalStartAt={totalStartAt} />
      </AssistantTimelineRow>
    ) : <EmptyAcpState />;
  }

  return (
    <div className="min-w-0 space-y-4">
      {timeline.map((event) => <ACPEventRenderer key={`${event.kind}-${event.id}-${event.seq}`} event={event} onLayoutChange={onLayoutChange} />)}
      {active ? (
        <AssistantTimelineRow>
          <AcpProcessingStatus
            kind={processingKindFromTimeline(latestEvent, sending)}
            active={active}
            startAt={latestEvent?.startedAt ?? latestEvent?.timestamp ?? totalStartAt}
            totalStartAt={totalStartAt}
          />
        </AssistantTimelineRow>
      ) : null}
    </div>
  );
}

function EmptyAcpState() {
  const { t } = useTranslation();
  return <div className="rounded-2xl border border-dashed bg-muted/10 p-8 text-center text-sm text-muted-foreground">{t('acp.noEvents')}</div>;
}

export function ACPEventRenderer({ event, onLayoutChange }: { event: AcpTimelineEvent; onLayoutChange?: () => void }) {
  if (event.kind === 'textDelta' || event.kind === 'userTextDelta') return <MessageBubble event={event} />;
  if (event.kind === 'thoughtDelta') return <AssistantTimelineRow><ThoughtBlock event={event} /></AssistantTimelineRow>;
  if (event.kind === 'toolCall' || event.kind === 'toolCallUpdate') return <AssistantTimelineRow><ToolCallCard event={event} onLayoutChange={onLayoutChange} /></AssistantTimelineRow>;
  if (event.kind === 'plan') return <AssistantTimelineRow><PlanBlock event={event} /></AssistantTimelineRow>;
  return null;
}

function AssistantTimelineRow({ children }: { children: React.ReactNode }) {
  return (
    <Message className="min-w-0 items-start justify-start gap-2">
      <MessageAvatar tone="assistant" />
      <div className="w-full min-w-0 max-w-[82%] flex-1">{children}</div>
    </Message>
  );
}

function AcpProcessingStatus({ kind, active, startAt, totalStartAt }: { kind: AcpProcessingKind; active: boolean; startAt?: string | null; totalStartAt?: string | null }) {
  const { t } = useTranslation();
  const stepSeconds = useElapsedSeconds(active, startAt);
  const totalSeconds = useElapsedSeconds(active, totalStartAt ?? startAt);
  const label = processingLabel(t, kind);
  return (
    <div className="min-w-0 rounded-2xl border border-primary/20 bg-primary/5 px-4 py-3 text-sm text-muted-foreground shadow-sm shadow-background/20">
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <Loader2 className="size-4 shrink-0 animate-spin text-primary" />
        <span className="font-medium text-foreground">{label}</span>
        <span className="rounded-full bg-background/70 px-2 py-0.5 text-xs tabular-nums">{t('acp.stepElapsed', { seconds: stepSeconds })}</span>
        <span className="rounded-full bg-background/70 px-2 py-0.5 text-xs tabular-nums">{t('acp.totalElapsed', { seconds: totalSeconds })}</span>
      </div>
    </div>
  );
}

function MessageBubble({ event }: { event: AcpTimelineEvent }) {
  const { t } = useTranslation();
  const isUser = event.kind === 'userTextDelta';
  const failed = event.status === 'failed';
  return (
    <Message className={cn('min-w-0 items-start gap-2', isUser ? 'justify-end' : 'justify-start')}>
      {!isUser ? <MessageAvatar tone="assistant" /> : null}
      <div className={cn('min-w-0 max-w-[82%] space-y-1', isUser && 'items-end')}>
        <MessageContent className={cn(
          'whitespace-pre-wrap rounded-2xl px-4 py-3 text-sm leading-6 shadow-sm [overflow-wrap:anywhere]',
          isUser ? 'rounded-br-md bg-primary text-primary-foreground' : 'rounded-bl-md border bg-card text-card-foreground',
          failed && 'border border-destructive/40 bg-destructive/10 text-destructive',
        )}>
          {event.content}
        </MessageContent>
        {event.optimistic || failed ? (
          <div className={cn('px-1 text-xs text-muted-foreground', isUser && 'text-right')}>
            {failed ? t('acp.sendFailed') : t('acp.sending')}
          </div>
        ) : null}
      </div>
      {isUser ? <MessageAvatar tone="user" /> : null}
    </Message>
  );
}

function MessageAvatar({ tone }: { tone: 'assistant' | 'user' }) {
  const Icon = tone === 'assistant' ? Bot : User;
  return (
    <div className={cn(
      'mt-1 flex size-7 shrink-0 items-center justify-center rounded-full border',
      tone === 'assistant' ? 'bg-card text-muted-foreground' : 'bg-primary/10 text-primary',
    )}>
      <Icon className="size-3.5" />
    </div>
  );
}

export function ThoughtBlock({ event }: { event: AcpTimelineEvent }) {
  const { t } = useTranslation();
  if (!event.content?.trim()) return null;
  const duration = formatThinkingDuration(t, event.durationMs);
  return (
    <ChainOfThought className="min-w-0 max-w-full overflow-hidden rounded-2xl border border-border/60 bg-muted/15 px-4 py-3 shadow-sm shadow-background/20">
      <ChainOfThoughtStep>
        <ChainOfThoughtTrigger leftIcon={<Clock className="size-4" />} className="w-full min-w-0 justify-between">
          <span className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="font-medium">{t('acp.thought')}</span>
            {duration ? <span className="rounded-full bg-muted px-2 py-0.5 text-xs tabular-nums">{duration}</span> : null}
          </span>
        </ChainOfThoughtTrigger>
        <ChainOfThoughtContent>
          <ChainOfThoughtItem className="break-words whitespace-pre-wrap text-muted-foreground [overflow-wrap:anywhere]">{event.content}</ChainOfThoughtItem>
        </ChainOfThoughtContent>
      </ChainOfThoughtStep>
    </ChainOfThought>
  );
}

export function ToolCallCard({ event, onLayoutChange }: { event: AcpTimelineEvent; onLayoutChange?: () => void }) {
  const { t } = useTranslation();
  const details = toolDetails(event);
  const ToolIcon = toolIcon(details.name);
  const input = Object.fromEntries(details.queryBlocks.map((block) => [t(block.labelKey), block.value]));
  const toolPart: ToolPart = {
    type: details.name ?? t('acp.toolCall'),
    state: toolState(event.status),
    input,
    output: details.output ?? undefined,
    toolCallId: event.toolCallId ?? undefined,
    errorText: event.status && toolStatusTone(event.status) === 'danger' ? event.content ?? undefined : undefined,
  };
  const labels: ToolLabels = {
    input: t('acp.toolParameters'),
    output: t('acp.toolOutput'),
    error: t('status.error'),
    processing: displayStatus(t, 'running'),
    pending: displayStatus(t, 'pending'),
    ready: t('acp.toolReady'),
    completed: displayStatus(t, 'completed'),
  };
  return <Tool toolPart={toolPart} labels={labels} icon={<ToolIcon className="size-4" />} onOpenChange={onLayoutChange} />;
}

function StatusBadge({ status, tone }: { status?: string | null; tone: ToolTone }) {
  const { t } = useTranslation();
  const label = status ? displayStatus(t, status) : t('acp.unknownStatus');
  return (
    <Badge
      variant="outline"
      className={cn(
        'shrink-0 gap-1 rounded-full px-2 py-0 text-xs',
        tone === 'pending' && 'border-primary/30 bg-primary/10 text-primary',
        tone === 'running' && 'border-primary/30 bg-primary/10 text-primary',
        tone === 'success' && 'border-emerald-500/40 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300',
        tone === 'danger' && 'border-destructive/40 bg-destructive/10 text-destructive',
      )}
    >
      {tone === 'running' || tone === 'pending' ? <span className="size-1.5 rounded-full bg-current animate-pulse" /> : null}
      {tone === 'success' ? <CheckCircle2 className="size-3" /> : null}
      {tone === 'danger' ? <CircleAlert className="size-3" /> : null}
      {label}
    </Badge>
  );
}

export function PlanBlock({ event }: { event: AcpTimelineEvent }) {
  const { t } = useTranslation();
  const entries = ((event.raw as { entries?: Array<{ content?: string; status?: string; priority?: string }> } | undefined)?.entries ?? []);
  return (
    <Card className="min-w-0 max-w-full overflow-hidden border-primary/20 bg-primary/5 shadow-none">
      <CardContent className="space-y-2 p-4">
        {entries.map((entry, index) => (
          <div className="flex min-w-0 items-start gap-2 text-sm" key={`${entry.content ?? index}-${index}`}>
            <Badge variant="secondary">{entry.status ? displayStatus(t, entry.status) : entry.priority ?? index + 1}</Badge>
            <span className="min-w-0 break-words [overflow-wrap:anywhere]">{entry.content}</span>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

export function PermissionRequestCard({ request, onSelect }: { request: AcpPermissionRequestVm; onSelect: (optionId: string) => void }) {
  return (
    <Card className="min-w-0 max-w-full overflow-hidden border-primary/20 bg-primary/5 shadow-none">
      <CardContent className="space-y-3 p-4">
        <div className="text-sm font-semibold">{request.title}</div>
        <div className="flex flex-wrap gap-2">
          {request.options.map((option) => (
            <Button key={option.optionId} size="sm" variant={option.kind.startsWith('allow') ? 'default' : 'outline'} onClick={() => onSelect(option.optionId)}>{option.name || option.optionId}</Button>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

export function RawFrameViewer({ content, loading, onLayoutChange }: { content: string; loading: boolean; onLayoutChange?: () => void }) {
  const { t } = useTranslation();
  const frames = rawFrameLines(content);
  if (loading) {
    return <div className="flex items-center gap-2 rounded-2xl border bg-card/70 p-4 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />{t('acp.loadingRawFrames')}</div>;
  }
  return (
    <div className="w-full min-w-0 max-w-full space-y-2 overflow-hidden">
      {frames.map((frame, index) => {
        const scrollable = isLongRawFrame(frame.expanded);
        return (
          <details key={`${index}-${frame.compact.slice(0, 24)}`} onToggle={onLayoutChange} className="group w-full min-w-0 max-w-full overflow-hidden rounded-xl border border-border/60 bg-card/50 font-mono text-[11px] leading-5 shadow-sm shadow-background/20 open:border-primary/20 open:bg-card/70 open:ring-1 open:ring-primary/10">
            <summary className="flex w-full min-w-0 cursor-pointer list-none gap-3 overflow-hidden px-3 py-2 text-muted-foreground outline-none transition-colors marker:hidden hover:bg-muted/20 focus-visible:bg-muted/20">
              <span className="shrink-0 select-none tabular-nums text-muted-foreground/80">{String(index + 1).padStart(3, '0')}</span>
              <code className="block min-w-0 flex-1 truncate text-foreground/75">{truncateFrameLine(frame.compact)}</code>
            </summary>
            <pre className={cn('block w-full min-w-0 max-w-full overflow-x-hidden whitespace-pre-wrap break-all border-t border-border/50 bg-background/40 px-4 py-3 text-foreground/75 outline-none [overflow-wrap:anywhere]', scrollable ? 'max-h-[38rem] overflow-y-auto [scrollbar-color:hsl(var(--muted-foreground)/0.35)_transparent] [scrollbar-width:thin] [&::-webkit-scrollbar]:w-2 [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-muted-foreground/30 [&::-webkit-scrollbar-thumb]:hover:bg-muted-foreground/45 [&::-webkit-scrollbar-track]:bg-transparent' : 'overflow-y-visible')}>{frame.expanded}</pre>
          </details>
        );
      })}
    </div>
  );
}

function useElapsedSeconds(active: boolean, startAt?: string | null) {
  const fallbackStart = useRef(Date.now());
  const startMs = parseAcpTimestamp(startAt) ?? fallbackStart.current;
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active, startMs]);

  return Math.max(0, Math.floor(((active ? now : Date.now()) - startMs) / 1000));
}

function isSessionActive(status?: string | null) {
  return ['pending', 'running', 'in_progress', 'sending'].includes(status?.toLowerCase() ?? '');
}

function processingKindFromTimeline(event: AcpTimelineEvent | null, sending: boolean): AcpProcessingKind {
  if (!event) return sending ? 'processing' : 'launching';
  if (event.kind === 'thoughtDelta') return 'thinking';
  if (event.kind === 'toolCall' || event.kind === 'toolCallUpdate') return 'tool';
  if (event.kind === 'textDelta') return 'responding';
  return 'processing';
}

function processingLabel(t: ReturnType<typeof useTranslation>['t'], kind: AcpProcessingKind) {
  if (kind === 'launching') return t('acp.launchingClaude');
  if (kind === 'thinking') return t('acp.thinkingNow');
  if (kind === 'tool') return t('acp.toolRunning');
  if (kind === 'responding') return t('acp.responding');
  return t('acp.processing');
}

function buildAcpTimeline(events: AcpUiEventVm[]) {
  const timeline: AcpTimelineEvent[] = [];
  const toolIndex = new Map<string, AcpTimelineEvent>();
  for (const event of events) {
    if (!isRenderableEvent(event)) continue;
    const previous = timeline[timeline.length - 1];
    if (event.kind === 'userTextDelta' && previous?.kind === 'userTextDelta' && sameText(previous.content, event.content)) continue;
    if (previous && previous.kind === event.kind && isMergeableDelta(event.kind)) {
      previous.content = `${previous.content ?? ''}${event.content ?? ''}`;
      previous.seq = event.seq;
      previous.endedAt = event.timestamp;
      previous.raw = event.raw;
      continue;
    }
    if ((event.kind === 'toolCall' || event.kind === 'toolCallUpdate') && event.toolCallId) {
      const existing = toolIndex.get(event.toolCallId);
      if (existing) {
        existing.kind = 'toolCall';
        existing.seq = event.seq;
        existing.endedAt = event.timestamp;
        existing.title = event.title ?? existing.title;
        existing.status = event.status ?? existing.status;
        existing.content = event.content ?? existing.content;
        existing.raw = mergeRaw(existing.raw, event.raw);
        continue;
      }
      const copy = { ...event, kind: 'toolCall', startedAt: event.timestamp, endedAt: event.timestamp };
      toolIndex.set(event.toolCallId, copy);
      timeline.push(copy);
      continue;
    }
    if (event.kind === 'thoughtDelta' && !event.content?.trim()) continue;
    timeline.push({ ...event, startedAt: event.timestamp, endedAt: event.timestamp, optimistic: isOptimisticEvent(event) });
  }
  return timeline.map((event, index) => {
    if (event.kind !== 'thoughtDelta') return event;
    const start = parseAcpTimestamp(event.startedAt ?? event.timestamp);
    const next = timeline.slice(index + 1).find((item) => parseAcpTimestamp(item.timestamp) != null);
    const end = parseAcpTimestamp(next?.timestamp) ?? parseAcpTimestamp(event.endedAt) ?? start;
    return start != null && end != null && end >= start ? { ...event, durationMs: Math.max(0, end - start) } : event;
  });
}

function isRenderableEvent(event: AcpUiEventVm) {
  if (hiddenEventKinds.has(event.kind)) return false;
  const sessionUpdate = rawObject(event.raw)?.sessionUpdate;
  return typeof sessionUpdate !== 'string' || !hiddenSessionUpdates.has(sessionUpdate);
}

function isMergeableDelta(kind: string) {
  return kind === 'textDelta' || kind === 'userTextDelta' || kind === 'thoughtDelta';
}

function rawObject(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function mergeRaw(previous: unknown, next: unknown) {
  const previousObject = rawObject(previous);
  const nextObject = rawObject(next);
  if (!previousObject || !nextObject) return next ?? previous;
  return { ...previousObject, ...nextObject };
}

function mergeOptimisticSession(session: AcpSessionVm | null | undefined, optimisticEvents: AcpUiEventVm[]) {
  if (!session || optimisticEvents.length === 0) return session ?? null;
  const pending = optimisticEvents.filter((event) => !hasMatchingUserPrompt(session.events, event));
  if (pending.length === 0) return session;
  return { ...session, events: [...session.events, ...pending] };
}

function optimisticUserEvent(content: string): AcpUiEventVm {
  const createdAt = Math.floor(Date.now() / 1000);
  return {
    id: `optimistic-user-${createdAt}-${Math.random().toString(36).slice(2)}`,
    seq: Number.MAX_SAFE_INTEGER - createdAt,
    timestamp: `${createdAt}Z`,
    kind: 'userTextDelta',
    content,
    status: 'sending',
    raw: { source: 'goldBandPrompt', optimistic: true },
  };
}

function isOptimisticEvent(event: AcpUiEventVm) {
  return rawObject(event.raw)?.optimistic === true;
}

function hasMatchingUserPrompt(events: AcpUiEventVm[], candidate: AcpUiEventVm) {
  if (candidate.kind !== 'userTextDelta') return false;
  return events.some((event) => event.kind === 'userTextDelta' && sameText(event.content, candidate.content));
}

function sameText(left?: string | null, right?: string | null) {
  return Boolean(left?.trim()) && left?.trim() === right?.trim();
}

function toolDetails(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const toolCall = rawObject(raw?.toolCall) ?? rawObject(raw?.content) ?? raw;
  const fields = rawObject(toolCall?.fields);
  const meta = rawObject(raw?._meta);
  const claudeCode = rawObject(meta?.claudeCode);
  const title = stringValue(toolCall?.title) ?? event.title;
  const claudeToolName = stringValue(claudeCode?.toolName);
  const name = claudeToolName ?? parseToolTitle(title).name ?? stringValue(toolCall?.name) ?? title;
  const output = cleanToolOutput(toolCall?.output ?? raw?.output ?? fields?.output ?? raw?.content);
  return {
    name,
    output,
    queryBlocks: queryBlocksFromTool(title),
  };
}

function queryBlocksFromTool(title: string | null | undefined) {
  const parsedTitle = parseToolTitle(title);
  const blocks: Array<{ labelKey: string; value: string }> = [];
  if (parsedTitle.scope) blocks.push({ labelKey: 'acp.toolPath', value: parsedTitle.scope });
  if (parsedTitle.query) blocks.push({ labelKey: 'acp.toolQuery', value: parsedTitle.query });
  return blocks;
}

function parseToolTitle(title: string | null | undefined) {
  if (!title) return { name: null, scope: null, query: null };
  const [name] = title.split(' ');
  const quoted = [...title.matchAll(/`([^`]+)`/g)].map((match) => match[1]);
  return {
    name: name || title,
    scope: quoted[0] ?? null,
    query: quoted[1] ?? null,
  };
}

function toolIcon(name: string | null | undefined) {
  const normalized = name?.toLowerCase();
  if (normalized === 'read') return FileText;
  if (normalized === 'glob' || normalized === 'grep') return Search;
  if (normalized === 'bash' || normalized === 'powershell') return Terminal;
  return Terminal;
}

function cleanToolOutput(value: unknown): unknown {
  if (Array.isArray(value) && value.length === 1) {
    const item = rawObject(value[0]);
    const content = rawObject(item?.content);
    const text = stringValue(content?.text);
    if (text) return text;
  }
  return value;
}

function rawFrameLines(content: string) {
  return content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => {
      try {
        const value = JSON.parse(line);
        return {
          compact: JSON.stringify(value),
          expanded: wrapLongSegments(JSON.stringify(value, null, 2)),
        };
      } catch {
        return {
          compact: line,
          expanded: wrapLongSegments(line),
        };
      }
    });
}

function truncateFrameLine(line: string) {
  return line.length > 300 ? `${line.slice(0, 300)}…` : line;
}

function isLongRawFrame(content: string) {
  return content.split('\n').length > 36 || content.length > 5000;
}

function wrapLongSegments(text: string) {
  return text.replace(/\S{120,}/g, (segment) => segment.match(/.{1,120}/g)?.join('\n') ?? segment);
}

function stringValue(value: unknown) {
  return typeof value === 'string' && value.trim() ? value : null;
}

function toolState(status?: string | null): ToolPart['state'] {
  const tone = toolStatusTone(status);
  if (tone === 'running') return 'input-streaming';
  if (tone === 'danger') return 'output-error';
  if (tone === 'success') return 'output-available';
  return 'input-available';
}

function toolStatusTone(status?: string | null): ToolTone {
  if (!status) return 'muted';
  if (['pending', 'sending'].includes(status)) return 'pending';
  if (['running', 'in_progress'].includes(status)) return 'running';
  if (['completed', 'success', 'succeeded'].includes(status)) return 'success';
  if (['failed', 'error', 'cancelled'].includes(status)) return 'danger';
  return 'muted';
}

function parseAcpTimestamp(value?: string | null) {
  if (!value) return null;
  const numeric = value.match(/^(\d+(?:\.\d+)?)Z?$/);
  if (numeric) return Number(numeric[1]) * 1000;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function formatThinkingDuration(t: ReturnType<typeof useTranslation>['t'], durationMs?: number) {
  if (durationMs == null) return null;
  const seconds = Math.max(1, Math.round(durationMs / 1000));
  return t('acp.thinkingDuration', { seconds });
}
