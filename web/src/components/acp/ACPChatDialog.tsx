import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Bot, ChevronDown, CircleStop, Clock, FileText, Loader2, Search, Send, ShieldQuestion, Terminal, User, UsersRound } from 'lucide-react';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Card, CardContent } from '@/components/ui/card';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { ChainOfThought, ChainOfThoughtContent, ChainOfThoughtItem, ChainOfThoughtStep, ChainOfThoughtTrigger } from '@/components/prompt-kit/chain-of-thought';
import { Markdown } from '@/components/prompt-kit/markdown';
import { Message, MessageContent } from '@/components/prompt-kit/message';
import { PromptInput, PromptInputActions, PromptInputAction, PromptInputTextarea } from '@/components/prompt-kit/prompt-input';
import { Tool, type ToolLabels, type ToolPart } from '@/components/prompt-kit/tool';
import { cn } from '@/lib/utils';
import { cancelAcpSession, getAcpRawFrames, getAcpSession, respondAcpPermission, sendAcpPrompt, submitManualCheck } from '@/api';
import { displayStatus } from '@/i18n';
import type { AcpPermissionRequestVm, AcpRawFramePageVm, AcpRawFrameQueryInput, AcpRawFrameVm, AcpSessionVm, AcpUiEventVm } from '@/types';

interface ACPChatDialogProps {
  session?: AcpSessionVm | null;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  runtimeStatus?: string | null;
  manualCheckPending?: boolean;
  optimisticEvents?: AcpUiEventVm[];
  onOptimisticEventsChange?: (events: AcpUiEventVm[]) => void;
  onManualCheckSubmitted?: () => void;
}

type AcpCanvasMode = 'chat' | 'raw';
type ToolTone = 'muted' | 'pending' | 'running' | 'success' | 'danger';
type AcpProcessingKind = 'sending' | 'launching' | 'processing' | 'thinking' | 'tool' | 'responding' | 'stopping';
type AcpTimelineEvent = AcpUiEventVm & {
  startedAt?: string;
  endedAt?: string;
  startedSeq?: number;
  endedSeq?: number;
  durationMs?: number;
  optimistic?: boolean;
};

type AcpChildAgentGroup = {
  kind: 'childAgentGroup';
  id: string;
  seq: number;
  timestamp?: string;
  startedSeq: number;
  endedSeq?: number;
  startedAt?: string;
  endedAt?: string;
  status?: string | null;
  title?: string | null;
  toolCallId?: string | null;
  toolEvent: AcpTimelineEvent;
  events: AcpTimelineItem[];
};

type AcpTimelineItem = AcpTimelineEvent | AcpChildAgentGroup;

const EVENT_WINDOW_LIMIT = 360;
const EVENT_PAGE_SIZE = 60;
const HISTORY_LOAD_THRESHOLD_PX = 240;
const BOTTOM_STICK_THRESHOLD_PX = 48;

function initialFirstItemIndex(session?: AcpSessionVm | null) {
  if (!session) return 1_000_000;
  return Math.max(1, 1_000_000 - session.eventPage.total + session.events.length);
}

function timelineItemCount(events: AcpUiEventVm[]) {
  return buildAcpTimeline(events).length;
}

function prependedTimelineItemCount(previous: AcpUiEventVm[], next: AcpUiEventVm[]) {
  const previousFirst = buildAcpTimeline(previous)[0];
  if (!previousFirst) return timelineItemCount(next);
  const previousFirstKey = timelineEventKey(previousFirst);
  const nextIndex = buildAcpTimeline(next).findIndex((event) => timelineEventKey(event) === previousFirstKey);
  return nextIndex < 0 ? 0 : nextIndex;
}

function removedTimelineItemCount(before: AcpUiEventVm[], after: AcpUiEventVm[]) {
  return Math.max(0, timelineItemCount(before) - timelineItemCount(after));
}

function timelineEventKey(event: AcpTimelineItem) {
  if (isChildAgentGroup(event)) return event.id;
  if ((event.kind === 'toolCall' || event.kind === 'toolCallUpdate') && event.toolCallId) return `tool-${event.toolCallId}`;
  return `${event.kind}-${event.id}-${event.seq}`;
}

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
  'runtimeError',
]);

const optimisticEventStore = new Map<string, AcpUiEventVm[]>();
const optimisticEventListeners = new Map<string, Set<(events: AcpUiEventVm[]) => void>>();

function readStoredOptimisticEvents(sessionKey: string) {
  return optimisticEventStore.get(sessionKey) ?? [];
}

function updateStoredOptimisticEvents(sessionKey: string, updater: (current: AcpUiEventVm[]) => AcpUiEventVm[]) {
  const next = updater(readStoredOptimisticEvents(sessionKey));
  if (next.length === 0) optimisticEventStore.delete(sessionKey);
  else optimisticEventStore.set(sessionKey, next);
  optimisticEventListeners.get(sessionKey)?.forEach((listener) => listener(next));
  return next;
}

export function updateAcpOptimisticEvents(sessionKey: string, updater: (current: AcpUiEventVm[]) => AcpUiEventVm[]) {
  return updateStoredOptimisticEvents(sessionKey, updater);
}

function subscribeStoredOptimisticEvents(sessionKey: string, listener: (events: AcpUiEventVm[]) => void) {
  const listeners = optimisticEventListeners.get(sessionKey) ?? new Set<(events: AcpUiEventVm[]) => void>();
  listeners.add(listener);
  optimisticEventListeners.set(sessionKey, listeners);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) optimisticEventListeners.delete(sessionKey);
  };
}

function latestSendingOptimisticEvent(events: AcpUiEventVm[]) {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event.kind === 'userTextDelta' && event.status === 'sending') return event;
  }
  return null;
}

export function ACPChatDialog({ session, taskId, runId, roundId, nodeId, attemptId, runtimeStatus, manualCheckPending = false, optimisticEvents: controlledOptimisticEvents, onOptimisticEventsChange, onManualCheckSubmitted }: ACPChatDialogProps) {
  const { t } = useTranslation();
  const sessionKey = `${taskId}:${runId}:${roundId}:${nodeId}:${attemptId}`;
  const restoredOptimisticEvents = controlledOptimisticEvents ?? readStoredOptimisticEvents(sessionKey);
  const restoredPromptEvent = latestSendingOptimisticEvent(restoredOptimisticEvents);
  const restoredPrompt = restoredPromptEvent?.content?.trim() || null;
  const restoredPromptId = promptIdFromEvent(restoredPromptEvent);
  const [currentSession, setCurrentSession] = useState<AcpSessionVm | null>(session ?? null);
  const [loadedEvents, setLoadedEvents] = useState<AcpUiEventVm[]>(() => session?.events ?? []);
  const [firstItemIndex, setFirstItemIndex] = useState(() => initialFirstItemIndex(session));
  const [optimisticEvents, setOptimisticEvents] = useState<AcpUiEventVm[]>(() => restoredOptimisticEvents);
  const [prompt, setPrompt] = useState('');
  const [sending, setSending] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [awaitingResponse, setAwaitingResponse] = useState(Boolean(restoredPromptEvent));
  const [activeTurnPrompt, setActiveTurnPrompt] = useState<string | null>(restoredPrompt);
  const [activeTurnPromptId, setActiveTurnPromptId] = useState<string | null>(restoredPromptId);
  const [activeTurnStartedAt, setActiveTurnStartedAt] = useState<string | null>(null);
  const [sendError, setSendError] = useState<string | null>(null);
  const [cancelError, setCancelError] = useState<string | null>(null);
  const [manualCheckError, setManualCheckError] = useState<string | null>(null);
  const [manualCheckSubmitting, setManualCheckSubmitting] = useState(false);
  const [manualCheckResolved, setManualCheckResolved] = useState(false);
  const [canvasMode, setCanvasMode] = useState<AcpCanvasMode>('chat');
  const [rawPage, setRawPage] = useState<AcpRawFramePageVm | null>(null);
  const [rawQuery, setRawQuery] = useState<AcpRawFrameQueryInput>({ page: 0, pageSize: 100 });
  const [rawLoading, setRawLoading] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [hasOlderEvents, setHasOlderEvents] = useState(() => session?.eventPage.hasOlder ?? false);
  const [hasNewerEvents, setHasNewerEvents] = useState(() => session?.eventPage.hasNewer ?? false);
  const [chatPrimed, setChatPrimed] = useState(false);
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [dismissedPermissionIds, setDismissedPermissionIds] = useState<Set<string>>(() => new Set());
  const [permissionError, setPermissionError] = useState<string | null>(null);
  const [queuedInterventionPrompt, setQueuedInterventionPrompt] = useState<string | null>(null);
  const scrollerElementRef = useRef<HTMLDivElement | null>(null);
  const contentElementRef = useRef<HTMLDivElement | null>(null);
  const historyScrollAnchorRef = useRef<{ scrollTop: number; scrollHeight: number } | null>(null);
  const loadingOlderRef = useRef(false);
  const loadingNewerRef = useRef(false);
  const pinToBottomRef = useRef(true);
  const ignoreHistoryAtBottomRef = useRef(false);
  const suppressNextAutoScrollRef = useRef(false);
  const historyBrowsingRef = useRef(false);
  const cancelRequestedRef = useRef(false);
  const latestSessionRef = useRef<AcpSessionVm | null>(session ?? null);

  const updateOptimisticEvents = (updater: (current: AcpUiEventVm[]) => AcpUiEventVm[]) => {
    const next = updateStoredOptimisticEvents(sessionKey, updater);
    setOptimisticEvents(next);
    onOptimisticEventsChange?.(next);
  };

  useEffect(() => {
    if (controlledOptimisticEvents) setOptimisticEvents(controlledOptimisticEvents);
  }, [controlledOptimisticEvents]);

  useEffect(() => subscribeStoredOptimisticEvents(sessionKey, setOptimisticEvents), [sessionKey]);

  useEffect(() => {
    setManualCheckResolved(false);
    setManualCheckSubmitting(false);
    setManualCheckError(null);
  }, [attemptId, manualCheckPending, nodeId, roundId, runId, taskId]);

  useEffect(() => {
    setCurrentSession(session ?? null);
    if (!session) {
      setLoadedEvents([]);
      setFirstItemIndex(0);
      setHasOlderEvents(false);
      setHasNewerEvents(false);
      return;
    }
    setLoadedEvents((events) => {
      if (events.length === 0) {
        setFirstItemIndex(initialFirstItemIndex(session));
        return session.events;
      }
      if (!pinToBottomRef.current) return events;
      const merged = mergeAcpEvents(events, session.events);
      const limited = limitAcpEvents(merged, 'start');
      setFirstItemIndex((current) => current + removedTimelineItemCount(merged, limited));
      return limited;
    });
    setHasOlderEvents((current) => current || session.eventPage.hasOlder);
    setHasNewerEvents((current) => current || session.eventPage.hasNewer);
  }, [session]);

  useEffect(() => {
    const storedOptimisticEvents = controlledOptimisticEvents ?? readStoredOptimisticEvents(sessionKey);
    const storedPromptEvent = latestSendingOptimisticEvent(storedOptimisticEvents);
    setOptimisticEvents(storedOptimisticEvents);
    setDismissedPermissionIds(new Set());
    setPermissionError(null);
    setSendError(null);
    setCancelError(null);
    setCancelling(false);
    setAwaitingResponse(Boolean(storedPromptEvent));
    setActiveTurnPrompt(storedPromptEvent?.content?.trim() || null);
    setActiveTurnPromptId(promptIdFromEvent(storedPromptEvent));
    setActiveTurnStartedAt(null);
    setRawPage(null);
    setRawQuery({ page: 0, pageSize: 100 });
    setLoadingOlder(false);
    setChatPrimed(false);
    setFirstItemIndex(initialFirstItemIndex(session));
    setHasOlderEvents(session?.eventPage.hasOlder ?? false);
    setHasNewerEvents(session?.eventPage.hasNewer ?? false);
    setIsAtBottom(true);
    loadingOlderRef.current = false;
    loadingNewerRef.current = false;
    pinToBottomRef.current = true;
    ignoreHistoryAtBottomRef.current = false;
    suppressNextAutoScrollRef.current = false;
    historyBrowsingRef.current = false;
    cancelRequestedRef.current = false;
    latestSessionRef.current = session ?? null;
    historyScrollAnchorRef.current = null;
    setCanvasMode('chat');
  }, [sessionKey]);

  const baseSession = currentSession ?? session;
  const visibleSession = useMemo(() => baseSession ? { ...baseSession, events: loadedEvents } : null, [baseSession, loadedEvents]);
  const pendingOptimisticPrompt = latestSendingOptimisticEvent(optimisticEvents.filter((event) => !hasMatchingUserPrompt(loadedEvents, event)));
  const waitingForOptimisticPrompt = Boolean(pendingOptimisticPrompt);
  const effective = useMemo(() => mergeOptimisticSession(visibleSession, optimisticEvents), [visibleSession, optimisticEvents]);
  const effectiveEvents = effective?.events ?? [];
  const pendingPermission = effective?.pendingPermissions?.find((request) => !dismissedPermissionIds.has(request.requestId)) ?? null;
  const waitingForPermission = Boolean(pendingPermission);
  const planInterventionOption = pendingPermission ? findPlanInterventionOption(pendingPermission) : null;
  const timeline = useMemo(() => buildAcpTimeline(effectiveEvents), [effectiveEvents]);
  const sessionActive = isSessionActive(effective?.status) || isRuntimeActiveStatus(runtimeStatus);
  const showManualCheckActions = manualCheckPending && !manualCheckResolved;
  const composerLocked = (waitingForPermission && !planInterventionOption) || showManualCheckActions;
  const turnAccepted = Boolean(activeTurnStartedAt);
  const submittingPrompt = (sending || waitingForOptimisticPrompt) && !turnAccepted;
  const activePromptLocked = sending || awaitingResponse || waitingForOptimisticPrompt || sessionActive || cancelling;
  const composerLatestEvent = timeline.at(-1) ?? null;
  const awaitingFirstResponse = !waitingForPermission && awaitingResponse && turnAccepted && !hasResponseAfterTurn(effectiveEvents, activeTurnStartedAt);
  const composerStatusActive = !waitingForPermission && !composerLocked && (submittingPrompt || awaitingResponse || sessionActive || cancelling);
  const composerSessionSeconds = effective?.sessionElapsedSeconds ?? null;
  const composerProcessingKind: AcpProcessingKind = cancelling ? 'stopping' : submittingPrompt ? 'sending' : awaitingFirstResponse ? 'processing' : timeline.length === 0 ? 'launching' : processingKindFromTimeline(composerLatestEvent, false);
  const showComposerStatus = !waitingForPermission && (composerStatusActive || composerSessionSeconds != null);
  const composerStatusStartAt = submittingPrompt || awaitingFirstResponse || cancelling ? activeTurnStartedAt : composerLatestEvent?.startedAt ?? composerLatestEvent?.timestamp ?? activeTurnStartedAt;
  const composerInputHint = waitingForPermission ? t('acp.permissionPending') : cancelling ? t('acp.stopping') : submittingPrompt ? t('acp.sending') : composerStatusActive ? t('acp.processing') : t('acp.promptInputHint');
  const composerPlaceholder = planInterventionOption ? t('acp.planInterventionHint') : t('acp.composerPlaceholder');
  const canSubmitPrompt = Boolean(prompt.trim()) && !cancelling && (planInterventionOption ? !sending : !activePromptLocked);
  const canStopSession = sessionActive || awaitingResponse || sending || waitingForOptimisticPrompt || cancelling;
  const sendButtonBusy = (sending || waitingForOptimisticPrompt) && !planInterventionOption;
  const lastEvent = effectiveEvents.at(-1);
  const eventScrollSignature = `${effectiveEvents.length}:${lastEvent?.seq ?? ''}:${lastEvent?.kind ?? ''}:${sending}:${cancelling}:${effective?.status ?? ''}`;

  const scrollChatToBottom = () => {
    const scroller = scrollerElementRef.current;
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  };

  const applySessionUpdate = (updated: AcpSessionVm | null) => {
    latestSessionRef.current = updated;
    setCurrentSession(updated);
    if (!updated) return;
    setHasNewerEvents(false);
    setHasOlderEvents((current) => current || updated.eventPage.hasOlder);
    setLoadedEvents((events) => {
      const merged = mergeAcpEvents(events, updated.events);
      const limited = limitAcpEvents(merged, 'start');
      setFirstItemIndex((current) => current + removedTimelineItemCount(merged, limited));
      return limited;
    });
  };

  useEffect(() => {
    latestSessionRef.current = effective ?? currentSession ?? session ?? null;
  }, [currentSession, effective, session]);

  useEffect(() => {
    if (!activePromptLocked) return;
    let active = true;
    const refreshSession = async () => {
      try {
        const updated = await getAcpSession(taskId, runId, roundId, nodeId, attemptId, undefined, latestSessionRef.current);
        if (active) applySessionUpdate(updated);
      } catch {
        // The send or stop request owns user-visible error handling.
      }
    };
    void refreshSession();
    const timer = window.setInterval(refreshSession, 1500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [activePromptLocked, attemptId, nodeId, roundId, runId, taskId]);

  useEffect(() => {
    if ((!awaitingResponse && !cancelling) || sessionActive || sending || waitingForOptimisticPrompt) return;
    setAwaitingResponse(false);
    setCancelling(false);
    cancelRequestedRef.current = false;
  }, [awaitingResponse, cancelling, sending, sessionActive, waitingForOptimisticPrompt]);

  useEffect(() => {
    const acceptedPrompt = findMatchingGoldBandUserPrompt(loadedEvents, activeTurnPrompt, activeTurnPromptId);
    if (acceptedPrompt && !activeTurnStartedAt) setActiveTurnStartedAt(acceptedPrompt.timestamp);
    updateOptimisticEvents((current) => {
      const next = current.filter((event) => !hasMatchingUserPrompt(loadedEvents, event));
      return next.length === current.length ? current : next;
    });
  }, [activeTurnPrompt, activeTurnPromptId, activeTurnStartedAt, loadedEvents]);

  useEffect(() => {
    if (canvasMode !== 'chat' || chatPrimed) return;
    if (timeline.length === 0) {
      setChatPrimed(true);
      return;
    }
    const firstFrame = window.requestAnimationFrame(() => {
      scrollChatToBottom();
      window.requestAnimationFrame(() => setChatPrimed(true));
    });
    return () => window.cancelAnimationFrame(firstFrame);
  }, [canvasMode, chatPrimed, timeline.length]);

  useEffect(() => {
    if (canvasMode !== 'chat' || !chatPrimed || loadingOlder || historyBrowsingRef.current) return;
    if (suppressNextAutoScrollRef.current) {
      suppressNextAutoScrollRef.current = false;
      return;
    }
    if (hasNewerEvents || !pinToBottomRef.current) return;
    scrollChatToBottom();
  }, [canvasMode, chatPrimed, eventScrollSignature, hasNewerEvents, loadingOlder, timeline.length]);

  useEffect(() => {
    if (canvasMode !== 'chat' || !chatPrimed) return;
    const content = contentElementRef.current;
    if (!content) return;
    let frame: number | null = null;
    const stickToBottom = () => {
      if (frame != null) window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(() => {
        frame = null;
        if (historyScrollAnchorRef.current || loadingOlderRef.current || historyBrowsingRef.current) return;
        if (hasNewerEvents || !pinToBottomRef.current) return;
        scrollChatToBottom();
      });
    };
    const observer = new ResizeObserver(stickToBottom);
    observer.observe(content);
    return () => {
      if (frame != null) window.cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [canvasMode, chatPrimed, hasNewerEvents]);

  useLayoutEffect(() => {
    const anchor = historyScrollAnchorRef.current;
    const scroller = scrollerElementRef.current;
    if (!anchor || !scroller) return;
    historyScrollAnchorRef.current = null;
    const restore = () => {
      const delta = scroller.scrollHeight - anchor.scrollHeight;
      scroller.scrollTop = anchor.scrollTop + delta;
    };
    restore();
    const frame = window.requestAnimationFrame(restore);
    return () => window.cancelAnimationFrame(frame);
  }, [firstItemIndex, timeline.length]);

  const releaseHistoryAtBottomGuard = () => {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        ignoreHistoryAtBottomRef.current = false;
      });
    });
  };

  const captureHistoryScrollAnchor = () => {
    const scroller = scrollerElementRef.current;
    if (!scroller) return;
    historyScrollAnchorRef.current = {
      scrollTop: scroller.scrollTop,
      scrollHeight: scroller.scrollHeight,
    };
  };

  const preserveScrollPosition = () => {};

  const handleChatScroll = () => {
    const scroller = scrollerElementRef.current;
    if (!scroller) return;
    const distanceFromBottom = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    const atBottom = distanceFromBottom <= BOTTOM_STICK_THRESHOLD_PX;
    if (atBottom) historyBrowsingRef.current = false;
    pinToBottomRef.current = atBottom && !hasNewerEvents;
    setIsAtBottom(atBottom);
    if (!atBottom && !ignoreHistoryAtBottomRef.current && scroller.scrollTop <= HISTORY_LOAD_THRESHOLD_PX) void loadOlderEvents();
  };

  const loadOlderEvents = async () => {
    if (loadingOlderRef.current || !hasOlderEvents || loadedEvents.length === 0) return;
    const oldestSeq = loadedEvents[0].seq;
    loadingOlderRef.current = true;
    pinToBottomRef.current = false;
    historyBrowsingRef.current = true;
    ignoreHistoryAtBottomRef.current = true;
    suppressNextAutoScrollRef.current = true;
    setIsAtBottom(false);
    setLoadingOlder(true);
    try {
      const updated = await getAcpSession(taskId, runId, roundId, nodeId, attemptId, { beforeSeq: oldestSeq, eventLimit: EVENT_PAGE_SIZE }, baseSession);
      if (!updated) return;
      captureHistoryScrollAnchor();
      setCurrentSession(updated);
      setHasOlderEvents(updated.eventPage.hasOlder);
      setLoadedEvents((events) => {
        const merged = mergeAcpEvents(updated.events, events);
        const limited = limitAcpEvents(merged, 'end');
        setFirstItemIndex((current) => Math.max(1, current - prependedTimelineItemCount(events, limited)));
        setHasNewerEvents(updated.eventPage.hasNewer || limited.length < merged.length);
        return limited;
      });
    } finally {
      loadingOlderRef.current = false;
      setLoadingOlder(false);
      releaseHistoryAtBottomGuard();
    }
  };

  const loadNewerEvents = async () => {
    if (loadingNewerRef.current || !hasNewerEvents || loadedEvents.length === 0) return;
    const newestSeq = loadedEvents[loadedEvents.length - 1].seq;
    loadingNewerRef.current = true;
    try {
      const updated = await getAcpSession(taskId, runId, roundId, nodeId, attemptId, { afterSeq: newestSeq, eventLimit: EVENT_PAGE_SIZE }, baseSession);
      if (!updated) return;
      setCurrentSession(updated);
      setHasNewerEvents(updated.eventPage.hasNewer);
      setLoadedEvents((events) => {
        const merged = mergeAcpEvents(events, updated.events);
        const limited = limitAcpEvents(merged, 'start');
        setFirstItemIndex((current) => current + removedTimelineItemCount(merged, limited));
        setHasOlderEvents(updated.eventPage.hasOlder || limited.length < merged.length);
        return limited;
      });
    } finally {
      loadingNewerRef.current = false;
    }
  };

  const submitPrompt = async (trimmed: string) => {
    if (sending || awaitingResponse || sessionActive || cancelling) return;
    const optimisticEvent = optimisticUserEvent(trimmed);
    const promptId = promptIdFromEvent(optimisticEvent);
    setPrompt('');
    setSendError(null);
    historyBrowsingRef.current = false;
    pinToBottomRef.current = true;
    setActiveTurnPrompt(trimmed);
    setActiveTurnPromptId(promptId);
    setActiveTurnStartedAt(null);
    setAwaitingResponse(true);
    updateOptimisticEvents((current) => [...current, optimisticEvent]);
    setSending(true);
    try {
      const updated = await sendAcpPrompt(taskId, runId, roundId, nodeId, attemptId, trimmed, promptId, effective ?? null);
      applySessionUpdate(updated);
      if (updated) {
        updateOptimisticEvents((current) => current.filter((event) => !hasMatchingUserPrompt(updated.events, event)));
      }
    } catch (error) {
      if (cancelRequestedRef.current) {
        setAwaitingResponse(true);
        setActiveTurnPrompt(null);
        setActiveTurnPromptId(null);
        setActiveTurnStartedAt(null);
        updateOptimisticEvents((current) => current.filter((event) => event.id !== optimisticEvent.id));
        return;
      }
      const message = error instanceof Error ? error.message : String(error);
      setSendError(message);
      setAwaitingResponse(false);
      setActiveTurnPrompt(null);
      setActiveTurnPromptId(null);
      setActiveTurnStartedAt(null);
      updateOptimisticEvents((current) => current.map((event) => event.id === optimisticEvent.id ? { ...event, status: 'failed' } : event));
    } finally {
      setSending(false);
    }
  };

  const send = async () => {
    const trimmed = prompt.trim();
    if (!trimmed) return;
    if (pendingPermission && planInterventionOption) {
      setPrompt('');
      setQueuedInterventionPrompt(trimmed);
      setAwaitingResponse(true);
      await answerPermission(pendingPermission, planInterventionOption.optionId);
      return;
    }
    await submitPrompt(trimmed);
  };

  const stopSession = async () => {
    if (cancelling || !canStopSession) return;
    cancelRequestedRef.current = true;
    setCancelling(true);
    setCancelError(null);
    setAwaitingResponse(true);
    try {
      const updated = await cancelAcpSession(taskId, runId, roundId, nodeId, attemptId, effective ?? null);
      applySessionUpdate(updated);
    } catch (error) {
      setCancelError(error instanceof Error ? error.message : String(error));
      setCancelling(false);
      cancelRequestedRef.current = false;
    }
  };

  const submitManualDecision = async (outcome: 'success' | 'failure') => {
    if (!showManualCheckActions || manualCheckSubmitting) return;
    setManualCheckError(null);
    setManualCheckSubmitting(true);
    try {
      await submitManualCheck(taskId, runId, roundId, nodeId, attemptId, outcome);
      setManualCheckResolved(true);
      onManualCheckSubmitted?.();
    } catch (error) {
      setManualCheckError(error instanceof Error ? error.message : String(error));
    } finally {
      setManualCheckSubmitting(false);
    }
  };

  const answerPermission = async (request: AcpPermissionRequestVm, optionId: string) => {
    setPermissionError(null);
    setDismissedPermissionIds((current) => new Set(current).add(request.requestId));
    try {
      const updated = await respondAcpPermission(taskId, runId, roundId, nodeId, attemptId, request.requestId, optionId, effective);
      applySessionUpdate(updated);
    } catch (error) {
      setDismissedPermissionIds((current) => {
        const next = new Set(current);
        next.delete(request.requestId);
        return next;
      });
      setQueuedInterventionPrompt(null);
      setPermissionError(error instanceof Error ? error.message : String(error));
    }
  };

  useEffect(() => {
    if (!queuedInterventionPrompt || sending || pendingPermission || sessionActive || awaitingResponse || cancelling) return;
    const queued = queuedInterventionPrompt;
    setQueuedInterventionPrompt(null);
    void submitPrompt(queued);
  }, [awaitingResponse, cancelling, pendingPermission, queuedInterventionPrompt, sending, sessionActive]);

  const loadRawFrames = async (query: AcpRawFrameQueryInput) => {
    setRawLoading(true);
    try {
      const next = await getAcpRawFrames(taskId, runId, roundId, nodeId, attemptId, query);
      setRawPage(next);
      setRawQuery({
        page: next.page,
        pageSize: next.pageSize,
        search: next.search ?? undefined,
        kind: next.kind ?? undefined,
        direction: next.direction ?? undefined,
      });
    } finally {
      setRawLoading(false);
    }
  };

  const toggleRawFrames = async () => {
    preserveScrollPosition();
    if (canvasMode === 'raw') {
      setCanvasMode('chat');
      return;
    }
    if (rawPage == null) await loadRawFrames(rawQuery);
    setCanvasMode('raw');
  };

  if (!effective) {
    return <AcpErrorState reason={t('acp.missingSessionReason')} />;
  }

  const visibleError = visibleSessionError(effective, effectiveEvents);

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-background">
      <ACPSessionHeader session={effective} rawActive={canvasMode === 'raw'} rawLoading={rawLoading} onToggleRaw={toggleRawFrames} />
      {visibleError ? <AcpErrorBanner reason={visibleError} /> : null}
      <div className="min-h-0 min-w-0 max-w-full flex-1 overflow-hidden">
        {canvasMode === 'raw' ? (
          <div className="h-full overflow-y-auto p-5">
            <RawFrameViewer
              loading={rawLoading}
              page={rawPage}
              query={rawQuery}
              onLayoutChange={preserveScrollPosition}
              onQueryChange={(query) => void loadRawFrames(query)}
            />
          </div>
        ) : (
          <div className="relative h-full min-w-0 overflow-hidden">
            <div
              ref={scrollerElementRef}
              className={cn('h-full min-w-0 overflow-y-auto overflow-x-hidden transition-opacity duration-200 [overflow-anchor:none]', !chatPrimed && 'opacity-0')}
              onScroll={handleChatScroll}
            >
              <div ref={contentElementRef} className="min-w-0 [overflow-anchor:none]">
                {loadingOlder ? <AcpListLoading label={t('acp.loadingOlderEvents')} /> : hasOlderEvents ? <AcpHistoryHint label={t('acp.scrollForHistory')} /> : <div className="h-3" />}
                {timeline.length === 0 && !isSessionActive(effective.status) && !sending ? <div className="p-5"><EmptyAcpState /></div> : null}
                {timeline.map((event) => <div className="px-5 py-2" key={timelineEventKey(event)}><ACPEventRenderer event={event} onLayoutChange={preserveScrollPosition} /></div>)}
                <div className="space-y-4 pb-5">
                  {sendError ? <AcpErrorBanner reason={`${t('acp.sendFailed')}：${sendError}`} /> : null}
                  {cancelError ? <AcpErrorBanner reason={`${t('acp.stopFailed')}：${cancelError}`} /> : null}
                  {manualCheckError ? <AcpErrorBanner reason={`${t('acp.manualCheckSubmitFailed')}：${manualCheckError}`} /> : null}
                  {permissionError ? <AcpErrorBanner reason={permissionError} /> : null}
                  {pendingPermission ? <PermissionRequestCard request={pendingPermission} onSelect={(optionId) => answerPermission(pendingPermission, optionId)} /> : null}
                </div>
              </div>
            </div>
            {!chatPrimed ? <AcpChatSkeleton /> : null}
          </div>
        )}
      </div>
      {canvasMode === 'chat' ? (
        <div className="shrink-0 border-t bg-background/95 p-4 backdrop-blur">
          {composerLocked ? (
            showManualCheckActions ? (
              <AcpManualCheckPanel
                submitting={manualCheckSubmitting}
                onSuccess={() => void submitManualDecision('success')}
                onFailure={() => void submitManualDecision('failure')}
              />
            ) : (
              <AcpPermissionComposerLock />
            )
          ) : (
            <PromptInput
              value={prompt}
              onValueChange={setPrompt}
              onSubmit={send}
              isLoading={sending}
              className="rounded-2xl bg-card/80 shadow-sm shadow-background/30 transition-colors focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/10"
            >
              {showComposerStatus ? (
                <AcpComposerStatus
                  kind={composerProcessingKind}
                  active={composerStatusActive}
                  startAt={composerStatusStartAt}
                  sessionSeconds={composerSessionSeconds}
                />
              ) : null}
              <PromptInputTextarea
                className="min-h-16 text-sm leading-6 text-foreground placeholder:text-muted-foreground"
                placeholder={composerPlaceholder}
                readOnly={activePromptLocked && !planInterventionOption}
              />
              <div className="mt-2 flex items-center justify-between gap-4 px-2 pb-1">
                <span className="text-xs text-muted-foreground">{composerInputHint}</span>
                <PromptInputActions className="shrink-0 pl-2">
                  {canStopSession ? (
                    <PromptInputAction tooltip={t('acp.stopHint')}>
                      <Button className="h-8 gap-1.5 rounded-full px-3" size="sm" variant="secondary" disabled={cancelling} onClick={stopSession}>
                        {cancelling ? <Loader2 className="size-3.5 animate-spin" /> : <CircleStop className="size-3.5" />}
                        {cancelling ? t('acp.stopping') : t('acp.stop')}
                      </Button>
                    </PromptInputAction>
                  ) : null}
                  <PromptInputAction tooltip={t('acp.send')}>
                    <Button className="h-8 gap-1.5 rounded-full px-3" size="sm" disabled={!canSubmitPrompt} onClick={send}>
                      {sendButtonBusy ? <Loader2 className="size-3.5 animate-spin" /> : <Send className="size-3.5" />}
                      {t('acp.send')}
                    </Button>
                  </PromptInputAction>
                </PromptInputActions>
              </div>
              <AcpSessionConfigBar session={effective} />
            </PromptInput>
          )}
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

function AcpListLoading({ label }: { label: string }) {
  return <div className="mx-auto my-3 flex w-fit items-center gap-2 rounded-full border bg-card/80 px-3 py-1.5 text-xs text-muted-foreground"><Loader2 className="size-3 animate-spin" />{label}</div>;
}

function AcpHistoryHint({ label }: { label: string }) {
  return <div className="mx-auto my-3 w-fit select-none rounded-full border border-dashed bg-muted/20 px-3 py-1 text-xs text-muted-foreground">{label}</div>;
}

function AcpPermissionComposerLock() {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-0 items-center gap-2 rounded-2xl border border-primary/15 bg-card/60 px-3 py-2 text-sm text-muted-foreground shadow-sm shadow-background/20">
      <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
        <ShieldQuestion className="size-4" />
      </span>
      <span className="min-w-0 truncate font-medium">{t('acp.permissionPending')}</span>
    </div>
  );
}

function AcpManualCheckPanel({ submitting, onSuccess, onFailure }: { submitting: boolean; onSuccess: () => void; onFailure: () => void }) {
  const { t } = useTranslation();
  return (
    <Card className="rounded-2xl border-primary/20 bg-card/85 shadow-sm shadow-background/30">
      <CardContent className="space-y-3 p-4">
        <div className="space-y-1">
          <div className="text-sm font-semibold text-foreground">{t('acp.manualCheckPending')}</div>
          <p className="text-xs leading-5 text-muted-foreground">{t('acp.manualCheckDescription')}</p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button className="h-9 rounded-full px-4" size="sm" disabled={submitting} onClick={onSuccess}>
            {submitting ? <Loader2 className="size-3.5 animate-spin" /> : null}
            {submitting ? t('acp.manualCheckSubmitting') : t('acp.manualCheckSuccess')}
          </Button>
          <Button className="h-9 rounded-full px-4" size="sm" variant="outline" disabled={submitting} onClick={onFailure}>
            {t('acp.manualCheckFailure')}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function AcpChatSkeleton() {
  return (
    <div className="pointer-events-none absolute inset-0 space-y-4 bg-background px-5 py-6">
      {[0, 1, 2].map((item) => (
        <div className="flex min-w-0 items-start gap-3" key={item}>
          <div className="size-7 shrink-0 animate-pulse rounded-full bg-muted" />
          <div className="min-w-0 flex-1 space-y-2 rounded-2xl border bg-card/60 p-4">
            <div className="h-3 w-2/5 animate-pulse rounded-full bg-muted" />
            <div className="h-3 w-4/5 animate-pulse rounded-full bg-muted" />
            <div className="h-3 w-3/5 animate-pulse rounded-full bg-muted" />
          </div>
        </div>
      ))}
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

function AcpSessionConfigBar({ session }: { session: AcpSessionVm }) {
  const { t } = useTranslation();
  const model = session.config?.currentModelName ?? session.config?.currentModelId;
  const mode = session.config?.currentModeName ?? session.config?.currentModeId;

  if (!model && !mode) return null;

  return (
    <div className="flex flex-wrap items-center gap-2 border-t border-border/50 px-2 py-2 text-xs text-muted-foreground">
      {model ? (
        <Badge variant="outline" className="max-w-full gap-1.5 rounded-full bg-background/50 px-2 py-0.5 font-normal">
          <span className="shrink-0 text-muted-foreground">{t('acp.currentModel')}</span>
          <span className="min-w-0 truncate text-foreground">{model}</span>
        </Badge>
      ) : null}
      {mode ? (
        <Badge variant="outline" className="max-w-full gap-1.5 rounded-full bg-background/50 px-2 py-0.5 font-normal">
          <span className="shrink-0 text-muted-foreground">{t('acp.permissionMode')}</span>
          <span className="min-w-0 truncate text-foreground">{mode}</span>
        </Badge>
      ) : null}
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

export function ACPMessageList({ timeline, sessionStatus, sending, onLayoutChange }: { timeline: AcpTimelineItem[]; sessionStatus: string; sending: boolean; onLayoutChange?: () => void }) {
  const active = isSessionActive(sessionStatus) || sending;

  if (timeline.length === 0) return active ? null : <EmptyAcpState />;

  return (
    <div className="min-w-0 space-y-4">
      {timeline.map((event) => <ACPEventRenderer key={timelineEventKey(event)} event={event} onLayoutChange={onLayoutChange} />)}
    </div>
  );
}

function EmptyAcpState() {
  const { t } = useTranslation();
  return <div className="rounded-2xl border border-dashed bg-muted/10 p-8 text-center text-sm text-muted-foreground">{t('acp.noEvents')}</div>;
}

export function ACPEventRenderer({ event, onLayoutChange }: { event: AcpTimelineItem; onLayoutChange?: () => void }) {
  if (isChildAgentGroup(event)) return <AssistantTimelineRow><ChildAgentGroupCard event={event} onLayoutChange={onLayoutChange} /></AssistantTimelineRow>;
  if (event.kind === 'textDelta' || event.kind === 'userTextDelta') return <MessageBubble event={event} />;
  if (event.kind === 'thoughtDelta') return <AssistantTimelineRow><ThoughtBlock event={event} /></AssistantTimelineRow>;
  if (event.kind === 'toolCall' || event.kind === 'toolCallUpdate') return <AssistantTimelineRow><ToolCallCard event={event} onLayoutChange={onLayoutChange} /></AssistantTimelineRow>;
  if (event.kind === 'plan') return <AssistantTimelineRow><PlanBlock event={event} /></AssistantTimelineRow>;
  return null;
}

function ChildAgentGroupCard({ event, onLayoutChange }: { event: AcpChildAgentGroup; onLayoutChange?: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(!isTerminalToolStatus(event.status));
  const input = agentToolInput(event.toolEvent);
  const details = toolDetails(event.toolEvent);
  const description = input.description ?? details.queryBlocks[0]?.value;
  const statusTone = toolStatusTone(event.status);
  const statusLabel = event.status ? displayStatus(t, event.status) : t('acp.subAgentRunning');
  const promptPreview = input.prompt ? truncateText(input.prompt, 240) : null;
  const output = details.output;

  const statusClass = statusTone === 'danger'
    ? 'bg-destructive/10 text-destructive'
    : statusTone === 'success'
      ? 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
      : statusTone === 'running'
        ? 'bg-primary/10 text-primary'
        : 'bg-muted text-muted-foreground';

  return (
    <div className="min-w-0 max-w-full overflow-hidden rounded-xl border border-primary/20 bg-card/75 shadow-sm shadow-background/30">
      <Collapsible open={open} onOpenChange={(next) => { setOpen(next); onLayoutChange?.(); }}>
        <CollapsibleTrigger asChild>
          <Button variant="ghost" className="h-auto w-full min-w-0 justify-between overflow-hidden rounded-none px-3 py-2 font-normal hover:bg-muted/20">
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary"><UsersRound className="size-4" /></span>
              <span className="min-w-0 flex-1 truncate text-left text-sm">
                <span className="font-semibold text-foreground">{t('acp.subAgent')}</span>
                {input.subagentType ? <span className="ml-2 font-mono text-xs text-muted-foreground">{input.subagentType}</span> : null}
                {description ? <span className="ml-2 text-xs text-muted-foreground">{description}</span> : null}
              </span>
            </div>
            <span className="ml-3 flex shrink-0 items-center gap-2">
              {event.events.length > 0 ? <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">{t('acp.subAgentEvents', { count: event.events.length })}</span> : null}
              <span className={cn('rounded-full px-2 py-0.5 text-xs font-medium', statusClass)}>{statusLabel}</span>
              <ChevronDown className={cn('size-4 shrink-0 text-muted-foreground transition-transform', open && 'rotate-180')} />
            </span>
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent className="min-w-0 max-w-full overflow-hidden border-t border-border data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down">
          {open ? (
            <div className="min-w-0 max-w-full space-y-3 overflow-hidden bg-background/50 p-3">
              {input.subagentType || description || promptPreview ? (
                <div className="grid min-w-0 gap-2 text-xs sm:grid-cols-2">
                  {input.subagentType ? <ChildAgentMeta label={t('acp.subAgentType')} value={input.subagentType} /> : null}
                  {description ? <ChildAgentMeta label={t('acp.subAgentDescription')} value={description} /> : null}
                  {promptPreview ? <ChildAgentMeta className="sm:col-span-2" label={t('acp.subAgentPrompt')} value={promptPreview} /> : null}
                </div>
              ) : null}
              {event.events.length > 0 ? (
                <div className="min-w-0 max-w-full space-y-3 overflow-hidden rounded-lg border border-border/60 bg-muted/10 p-3">
                  {event.events.map((child) => <ACPEventRenderer key={timelineEventKey(child)} event={child} onLayoutChange={onLayoutChange} />)}
                </div>
              ) : null}
              {output ? (
                <div className="min-w-0 max-w-full overflow-hidden rounded-lg border bg-background/70 p-2.5 text-xs">
                  <div className="mb-1 font-medium uppercase tracking-wide text-muted-foreground">{t('acp.subAgentResult')}</div>
                  <pre className="max-h-52 min-w-0 overflow-auto whitespace-pre-wrap break-words font-mono text-foreground [overflow-wrap:anywhere]">{formatToolValue(output)}</pre>
                </div>
              ) : null}
            </div>
          ) : null}
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}

function ChildAgentMeta({ label, value, className }: { label: string; value: string; className?: string }) {
  return (
    <div className={cn('min-w-0 overflow-hidden rounded-lg border bg-background/70 px-2.5 py-1.5', className)}>
      <div className="mb-1 truncate text-muted-foreground">{label}</div>
      <div className="break-words text-foreground [overflow-wrap:anywhere]">{value}</div>
    </div>
  );
}

function AssistantTimelineRow({ children }: { children: React.ReactNode }) {
  return (
    <Message className="min-w-0 items-start justify-start gap-2">
      <div className="size-7 shrink-0" aria-hidden="true" />
      <div className="w-full min-w-0 max-w-[82%] flex-1">{children}</div>
    </Message>
  );
}

function AcpComposerStatus({ kind, active, startAt, sessionSeconds }: { kind: AcpProcessingKind; active: boolean; startAt?: string | null; sessionSeconds?: number | null }) {
  const { t } = useTranslation();
  const [stepStartAt, setStepStartAt] = useState<string | null>(startAt ?? null);
  const previousKind = useRef(kind);

  useEffect(() => {
    if (!active) return;
    if (previousKind.current !== kind || !stepStartAt) {
      previousKind.current = kind;
      setStepStartAt(startAt ?? new Date().toISOString());
    }
  }, [active, kind, startAt, stepStartAt]);

  const stepSeconds = useElapsedSeconds(active && kind !== 'sending', stepStartAt ?? startAt);
  const label = processingLabel(t, kind);
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-2 px-3 pb-1 pt-2 text-xs text-muted-foreground">
      {active ? (
        <>
          <Loader2 className="size-3.5 shrink-0 animate-spin text-primary" />
          <span className="font-medium text-foreground">{label}</span>
          {kind === 'sending' ? <AnimatedEllipsis /> : <span className="rounded-full bg-muted/60 px-2 py-0.5 tabular-nums">{t('acp.stepElapsed', { duration: formatElapsedDuration(stepSeconds) })}</span>}
        </>
      ) : null}
      {sessionSeconds != null ? <span className="rounded-full bg-muted/60 px-2 py-0.5 tabular-nums">{t('acp.sessionElapsed', { duration: formatElapsedDuration(sessionSeconds) })}</span> : null}
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
          'rounded-2xl px-4 py-3 text-sm leading-6 shadow-sm [overflow-wrap:anywhere]',
          isUser ? 'whitespace-pre-wrap rounded-br-md bg-primary text-primary-foreground' : 'rounded-bl-md border bg-card text-card-foreground',
          failed && 'border border-destructive/40 bg-destructive/10 text-destructive',
        )}>
          {isUser ? event.content : <Markdown>{event.content ?? ''}</Markdown>}
        </MessageContent>
        {event.optimistic || failed ? (
          <div className={cn('flex px-1 text-xs text-muted-foreground', isUser && 'justify-end text-right')}>
            {failed ? t('acp.sendFailed') : <span className="inline-flex items-center">{event.status === 'processing' ? t('acp.processing') : t('acp.sending')}<AnimatedEllipsis /></span>}
          </div>
        ) : null}
      </div>
      {isUser ? <MessageAvatar tone="user" /> : null}
    </Message>
  );
}

function AnimatedEllipsis() {
  return (
    <span className="inline-flex w-4 items-center justify-start" aria-hidden="true">
      <span className="animate-pulse">.</span>
      <span className="animate-pulse [animation-delay:150ms]">.</span>
      <span className="animate-pulse [animation-delay:300ms]">.</span>
    </span>
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
    <ChainOfThought className="min-w-0 max-w-full overflow-hidden rounded-xl border border-border/60 bg-muted/15 px-3.5 py-2 shadow-sm shadow-background/20">
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
    summary: toolSummary(details.queryBlocks),
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
  const { t } = useTranslation();
  return (
    <AssistantTimelineRow>
      <div className="w-full max-w-3xl overflow-hidden rounded-xl border border-primary/20 bg-card/80 px-3 py-2 shadow-sm shadow-background/20">
        <div className="flex min-w-0 flex-col gap-2.5">
          <div className="flex min-w-0 items-center gap-2.5">
            <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
              <ShieldQuestion className="size-3.5" />
            </span>
            <div className="min-w-0">
              <div className="truncate text-sm font-semibold text-foreground">{request.title}</div>
              <div className="truncate text-xs text-muted-foreground">{t('acp.permissionPending')}</div>
            </div>
          </div>
          <div className="grid min-w-0 grid-cols-1 gap-1.5 pl-9 sm:grid-cols-2 sm:gap-2">
            {request.options.map((option) => (
              <Button
                key={option.optionId}
                size="sm"
                variant={option.kind.startsWith('allow') ? 'default' : 'outline'}
                className="h-7 max-w-full justify-center rounded-full px-2.5 text-xs"
                onClick={() => onSelect(option.optionId)}
              >
                <span className="min-w-0 truncate">{option.name || option.optionId}</span>
              </Button>
            ))}
          </div>
        </div>
      </div>
    </AssistantTimelineRow>
  );
}

export function RawFrameViewer({ page, query, loading, onQueryChange, onLayoutChange }: { page: AcpRawFramePageVm | null; query: AcpRawFrameQueryInput; loading: boolean; onQueryChange: (query: AcpRawFrameQueryInput) => void; onLayoutChange?: () => void }) {
  const { t } = useTranslation();
  const [searchInput, setSearchInput] = useState(query.search ?? '');

  useEffect(() => {
    setSearchInput(query.search ?? '');
  }, [query.search]);

  const pageSize = page?.pageSize ?? query.pageSize ?? 100;
  const applyQuery = (next: AcpRawFrameQueryInput) => onQueryChange({ ...query, ...next });
  const applySearch = () => applyQuery({ page: 0, search: searchInput.trim() || undefined });
  const clearSearch = () => {
    setSearchInput('');
    onQueryChange({ page: 0, pageSize, direction: undefined, search: undefined, kind: undefined });
  };

  if (loading && !page) {
    return <div className="flex items-center gap-2 rounded-2xl border bg-card/70 p-4 text-sm text-muted-foreground"><Loader2 className="size-4 animate-spin" />{t('acp.loadingRawFrames')}</div>;
  }

  return (
    <div className="w-full min-w-0 max-w-full space-y-3 overflow-hidden">
      <div className="rounded-2xl border border-border/60 bg-card/50 p-3 shadow-sm shadow-background/20">
        <div className="flex min-w-0 flex-col gap-3">
          <div className="flex min-w-0 flex-col gap-2 lg:flex-row">
            <div className="relative min-w-0 flex-1">
              <Search className="pointer-events-none absolute left-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <input
                className="h-9 w-full rounded-md border border-input bg-background/70 pl-8 pr-3 text-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-primary/50 focus-visible:ring-2 focus-visible:ring-primary/10"
                value={searchInput}
                placeholder={t('acp.rawSearchPlaceholder')}
                onChange={(event) => setSearchInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') applySearch();
                }}
              />
            </div>
            <Select value={query.kind ?? 'all'} onValueChange={(value) => applyQuery({ page: 0, kind: value === 'all' ? undefined : value })}>
              <SelectTrigger className="h-9 lg:w-44"><SelectValue placeholder={t('acp.rawKindPlaceholder')} /></SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('acp.rawKindAll')}</SelectItem>
                {rawKindOptions(t).map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}
              </SelectContent>
            </Select>
            <Select value={query.direction ?? 'all'} onValueChange={(value) => applyQuery({ page: 0, direction: value === 'all' ? undefined : value })}>
              <SelectTrigger className="h-9 lg:w-36"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t('acp.rawDirectionAll')}</SelectItem>
                <SelectItem value="inbound">{t('acp.rawInbound')}</SelectItem>
                <SelectItem value="outbound">{t('acp.rawOutbound')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="flex min-w-0 flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
            <span className="min-w-0 truncate">{rawFramePageSummary(t, page)}</span>
            <div className="flex flex-wrap items-center gap-2">
              {loading ? <Loader2 className="size-3.5 animate-spin text-primary" /> : null}
              <Select value={String(pageSize)} onValueChange={(value) => applyQuery({ page: 0, pageSize: Number(value) })}>
                <SelectTrigger className="h-8 w-24"><SelectValue /></SelectTrigger>
                <SelectContent>
                  <SelectItem value="50">50</SelectItem>
                  <SelectItem value="100">100</SelectItem>
                  <SelectItem value="200">200</SelectItem>
                </SelectContent>
              </Select>
              <Button size="sm" variant="outline" className="h-8 rounded-full px-3" disabled={loading} onClick={applySearch}>{t('acp.rawSearch')}</Button>
              <Button size="sm" variant="ghost" className="h-8 rounded-full px-3" disabled={loading} onClick={clearSearch}>{t('acp.rawClear')}</Button>
              <Button size="sm" variant="outline" className="h-8 rounded-full px-3" disabled={loading || !page || page.page === 0} onClick={() => applyQuery({ page: 0 })}>{t('acp.rawLatest')}</Button>
              <Button size="sm" variant="outline" className="h-8 rounded-full px-3" disabled={loading || !page?.hasPrevious} onClick={() => applyQuery({ page: Math.max(0, (page?.page ?? 0) - 1) })}>{t('acp.rawNewer')}</Button>
              <Button size="sm" variant="outline" className="h-8 rounded-full px-3" disabled={loading || !page?.hasNext} onClick={() => applyQuery({ page: (page?.page ?? 0) + 1 })}>{t('acp.rawOlder')}</Button>
            </div>
          </div>
        </div>
      </div>

      {page && page.items.length > 0 ? page.items.map((frame) => <RawFrameRow key={frame.id} frame={frame} onLayoutChange={onLayoutChange} />) : (
        <div className="rounded-2xl border border-dashed bg-muted/10 p-8 text-center text-sm text-muted-foreground">{t('acp.rawNoFrames')}</div>
      )}
    </div>
  );
}

function RawFrameRow({ frame, onLayoutChange }: { frame: AcpRawFrameVm; onLayoutChange?: () => void }) {
  const { t } = useTranslation();
  const display = rawFrameDisplay(frame.content);
  const scrollable = isLongRawFrame(display.expanded);
  return (
    <details onToggle={onLayoutChange} className="group w-full min-w-0 max-w-full overflow-hidden rounded-xl border border-border/60 bg-card/50 font-mono text-[11px] leading-5 shadow-sm shadow-background/20 open:border-primary/20 open:bg-card/70 open:ring-1 open:ring-primary/10">
      <summary className="flex w-full min-w-0 cursor-pointer list-none items-center gap-2 overflow-hidden px-3 py-2 text-muted-foreground outline-none transition-colors marker:hidden hover:bg-muted/20 focus-visible:bg-muted/20">
        <span className="shrink-0 select-none tabular-nums text-muted-foreground/80">#{frame.lineNumber}</span>
        {frame.timestamp ? <span className="hidden shrink-0 tabular-nums text-muted-foreground/70 sm:inline">{frame.timestamp}</span> : null}
        {frame.direction ? <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">{displayRawDirection(t, frame.direction)}</span> : null}
        <span className="shrink-0 rounded-full bg-primary/10 px-2 py-0.5 text-[10px] text-primary">{displayRawKind(t, frame.kind)}</span>
        <code className="block min-w-0 flex-1 truncate text-foreground/75">{truncateFrameLine(display.compact)}</code>
        {frame.contentTruncated ? <span className="shrink-0 text-[10px] text-amber-600 dark:text-amber-300">truncated</span> : null}
      </summary>
      <pre className={cn('block w-full min-w-0 max-w-full overflow-x-hidden whitespace-pre-wrap break-all border-t border-border/50 bg-background/40 px-4 py-3 text-foreground/75 outline-none [overflow-wrap:anywhere]', scrollable ? 'max-h-[38rem] overflow-y-auto [scrollbar-color:hsl(var(--muted-foreground)/0.35)_transparent] [scrollbar-width:thin] [&::-webkit-scrollbar]:w-2 [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-muted-foreground/30 [&::-webkit-scrollbar-thumb]:hover:bg-muted-foreground/45 [&::-webkit-scrollbar-track]:bg-transparent' : 'overflow-y-visible')}>{display.expanded}</pre>
    </details>
  );
}

function useElapsedSeconds(active: boolean, startAt?: string | null, endAt?: string | null) {
  const fallbackStart = useRef(Date.now());
  const startMs = parseAcpTimestamp(startAt) ?? fallbackStart.current;
  const endMs = parseAcpTimestamp(endAt) ?? Date.now();
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active, startMs]);

  return Math.max(0, Math.floor(((active ? now : endMs) - startMs) / 1000));
}

function firstResponseTimestampAfter(events: AcpUiEventVm[], start: number, before?: number | null) {
  for (const event of events) {
    if (!isResponseTimingEvent(event)) continue;
    const timestamp = parseAcpTimestamp(event.timestamp);
    if (timestamp != null && timestamp >= start && (before == null || timestamp < before)) return timestamp;
  }
  return null;
}

function promptIdFromEvent(event?: AcpUiEventVm | null) {
  return stringValue(rawObject(event?.raw)?.promptId) ?? null;
}

function isGoldBandUserPrompt(event: AcpUiEventVm) {
  return event.kind === 'userTextDelta' && rawObject(event.raw)?.source === 'goldBandPrompt';
}

function isGoldBandManagedPrompt(event: AcpUiEventVm) {
  return event.kind === 'userTextDelta' && (isGoldBandUserPrompt(event) || isOptimisticEvent(event));
}

function shouldMergeUserPromptEvents(previous: AcpUiEventVm | undefined, event: AcpUiEventVm) {
  if (!previous || previous.kind !== 'userTextDelta' || event.kind !== 'userTextDelta') return false;
  if (!sameText(previous.content, event.content)) return false;
  const previousPromptId = promptIdFromEvent(previous);
  const promptId = promptIdFromEvent(event);
  if (previousPromptId || promptId) return previousPromptId != null && previousPromptId === promptId;
  return isGoldBandManagedPrompt(previous) !== isGoldBandManagedPrompt(event);
}

function isChildAgentGroup(event: AcpTimelineItem): event is AcpChildAgentGroup {
  return event.kind === 'childAgentGroup';
}

function isAgentToolCall(event: AcpTimelineEvent) {
  if (event.kind !== 'toolCall' && event.kind !== 'toolCallUpdate') return false;
  const name = toolDetails(event).name?.trim().toLowerCase();
  if (name === 'agent') return true;
  if (name !== 'task') return false;
  const input = agentToolInput(event);
  return Boolean(input.prompt || input.description || input.subagentType);
}

function isTerminalToolStatus(status?: string | null) {
  return ['completed', 'success', 'succeeded', 'failed', 'error', 'cancelled', 'canceled'].includes(status?.toLowerCase() ?? '');
}

function agentToolInput(event: AcpTimelineEvent) {
  const raw = rawObject(event.raw);
  const toolCall = rawObject(raw?.toolCall) ?? rawObject(raw?.content) ?? raw;
  const rawInput = rawObject(toolCall?.rawInput) ?? rawObject(raw?.rawInput);
  return {
    subagentType: stringValue(rawInput?.subagent_type) ?? stringValue(rawInput?.subagentType),
    description: stringValue(rawInput?.description),
    prompt: stringValue(rawInput?.prompt),
  };
}

function parentToolUseId(event: AcpTimelineEvent) {
  const raw = rawObject(event.raw);
  const meta = rawObject(raw?._meta);
  const claudeCode = rawObject(meta?.claudeCode);
  return stringValue(claudeCode?.parentToolUseId);
}

function isResponseTimingEvent(event: AcpUiEventVm) {
  return event.kind !== 'userTextDelta';
}

function hasResponseAfterTurn(events: AcpUiEventVm[], turnStartedAt?: string | null) {
  const start = parseAcpTimestamp(turnStartedAt);
  return start != null && firstResponseTimestampAfter(events, start) != null;
}

function isSessionActive(status?: string | null) {
  return ['pending', 'running', 'in_progress', 'sending', 'cancelling', 'cancel_requested'].includes(status?.toLowerCase() ?? '');
}

function isRuntimeActiveStatus(status?: string | null) {
  return ['pending', 'running', 'in_progress', 'active'].includes(status?.toLowerCase() ?? '');
}

function processingKindFromTimeline(event: AcpTimelineItem | null, sending: boolean): AcpProcessingKind {
  if (sending) return 'sending';
  if (!event) return 'launching';
  if (isChildAgentGroup(event)) return processingKindFromTimeline(event.events.at(-1) ?? event.toolEvent, sending);
  if (event.kind === 'thoughtDelta') return 'thinking';
  if (event.kind === 'toolCall' || event.kind === 'toolCallUpdate') return 'tool';
  if (event.kind === 'textDelta') return 'responding';
  return 'processing';
}

function processingLabel(t: ReturnType<typeof useTranslation>['t'], kind: AcpProcessingKind) {
  if (kind === 'sending') return t('acp.sending');
  if (kind === 'stopping') return t('acp.stopping');
  if (kind === 'launching') return t('acp.launchingClaude');
  if (kind === 'thinking') return t('acp.thinkingNow');
  if (kind === 'tool') return t('acp.toolRunning');
  if (kind === 'responding') return t('acp.responding');
  return t('acp.processing');
}

function findPlanInterventionOption(request: AcpPermissionRequestVm) {
  return request.options.find((option) => {
    const label = `${option.optionId} ${option.name} ${option.kind}`.toLowerCase();
    return label.includes('keep planning') || label.includes('继续规划') || label.includes('keep-planning');
  }) ?? null;
}

function visibleSessionError(session: AcpSessionVm, events: AcpUiEventVm[]) {
  const message = session.diagnostics.lastError;
  if (!message) return null;
  const errorAt = parseAcpTimestamp(session.diagnostics.lastErrorTimestamp);
  if (errorAt == null) return message;
  return events.some((event) => isNormalResponseAfterError(event, errorAt)) ? null : message;
}

function isNormalResponseAfterError(event: AcpUiEventVm, errorAt: number) {
  const timestamp = parseAcpTimestamp(event.timestamp);
  if (timestamp == null || timestamp <= errorAt) return false;
  if (!['textDelta', 'thoughtDelta', 'toolCall', 'toolCallUpdate', 'plan'].includes(event.kind)) return false;
  return toolStatusTone(event.status) !== 'danger';
}

function buildAcpTimeline(events: AcpUiEventVm[]): AcpTimelineItem[] {
  return groupChildAgentTimeline(buildFlatAcpTimeline(events));
}

function buildFlatAcpTimeline(events: AcpUiEventVm[]) {
  const timeline: AcpTimelineEvent[] = [];
  const toolIndex = new Map<string, AcpTimelineEvent>();
  for (const event of events) {
    if (!isRenderableEvent(event)) continue;
    const previous = timeline[timeline.length - 1];
    if (shouldMergeUserPromptEvents(previous, event)) {
      previous.seq = event.seq;
      previous.endedSeq = event.seq;
      previous.endedAt = event.timestamp;
      previous.status = event.status ?? previous.status;
      previous.raw = mergeRaw(previous.raw, event.raw);
      previous.optimistic = previous.optimistic || isOptimisticEvent(event);
      continue;
    }
    if (previous && previous.kind === event.kind && isMergeableDelta(event.kind)) {
      if (isSameDeltaStream(previous, event)) {
        previous.content = event.content ?? previous.content;
        previous.seq = event.seq;
        previous.endedSeq = event.seq;
        previous.endedAt = event.timestamp;
        previous.status = event.status ?? previous.status;
        previous.raw = mergeRaw(previous.raw, event.raw);
        previous.optimistic = previous.optimistic || isOptimisticEvent(event);
        continue;
      }
      previous.content = `${previous.content ?? ''}${event.content ?? ''}`;
      previous.seq = event.seq;
      previous.endedSeq = event.seq;
      previous.endedAt = event.timestamp;
      previous.raw = event.raw;
      continue;
    }
    if ((event.kind === 'toolCall' || event.kind === 'toolCallUpdate') && event.toolCallId) {
      const existing = toolIndex.get(event.toolCallId);
      if (existing) {
        existing.kind = 'toolCall';
        existing.seq = event.seq;
        existing.endedSeq = event.seq;
        existing.endedAt = event.timestamp;
        existing.title = event.title ?? existing.title;
        existing.status = event.status ?? existing.status;
        existing.content = event.content ?? existing.content;
        existing.raw = mergeRaw(existing.raw, event.raw);
        continue;
      }
      const copy: AcpTimelineEvent = { ...event, kind: 'toolCall', startedAt: event.timestamp, endedAt: event.timestamp, startedSeq: event.seq, endedSeq: event.seq };
      toolIndex.set(event.toolCallId, copy);
      timeline.push(copy);
      continue;
    }
    if (event.kind === 'thoughtDelta' && !event.content?.trim()) continue;
    timeline.push({ ...event, startedAt: event.timestamp, endedAt: event.timestamp, startedSeq: event.seq, endedSeq: event.seq, optimistic: isOptimisticEvent(event) });
  }
  let nextTimestamp: number | null = null;
  for (let index = timeline.length - 1; index >= 0; index -= 1) {
    const event = timeline[index];
    const currentTimestamp = parseAcpTimestamp(event.timestamp);
    if (event.kind === 'thoughtDelta') {
      const start = parseAcpTimestamp(event.startedAt ?? event.timestamp);
      const end = nextTimestamp ?? parseAcpTimestamp(event.endedAt) ?? start;
      if (start != null && end != null && end >= start) {
        timeline[index] = { ...event, durationMs: Math.max(0, end - start) };
      }
    }
    if (currentTimestamp != null) nextTimestamp = currentTimestamp;
  }
  return timeline;
}

function groupChildAgentTimeline(events: AcpTimelineEvent[]): AcpTimelineItem[] {
  const grouped: AcpTimelineItem[] = [];
  const agentToolCallIds = new Set(events.filter(isAgentToolCall).map((event) => event.toolCallId).filter(Boolean));
  const ownedChildKeys = new Set<string>();
  const childrenByParent = new Map<string, AcpTimelineEvent[]>();

  for (const event of events) {
    const parentId = parentToolUseId(event);
    if (!parentId || !agentToolCallIds.has(parentId)) continue;
    const children = childrenByParent.get(parentId) ?? [];
    children.push(event);
    childrenByParent.set(parentId, children);
    ownedChildKeys.add(timelineEventKey(event));
  }

  for (let index = 0; index < events.length; index += 1) {
    const event = events[index];
    if (ownedChildKeys.has(timelineEventKey(event))) continue;
    if (!isAgentToolCall(event)) {
      grouped.push(event);
      continue;
    }

    const startSeq = event.startedSeq ?? event.seq;
    const terminal = isTerminalToolStatus(event.status);
    const endSeq = terminal ? event.endedSeq ?? event.seq : Number.POSITIVE_INFINITY;
    const ownedChildren = event.toolCallId ? childrenByParent.get(event.toolCallId) ?? [] : [];
    const children: AcpTimelineEvent[] = [...ownedChildren];
    let cursor = index + 1;
    let usedSequenceFallback = false;

    if (children.length === 0) {
      usedSequenceFallback = true;
      while (cursor < events.length) {
        const candidate = events[cursor];
        const candidateStartSeq = candidate.startedSeq ?? candidate.seq;
        if (ownedChildKeys.has(timelineEventKey(candidate))) break;
        if (isGoldBandUserPrompt(candidate)) break;
        if (candidateStartSeq <= startSeq) break;
        if (candidateStartSeq >= endSeq) break;
        if (isAgentToolCall(candidate)) break;
        children.push(candidate);
        cursor += 1;
      }
    }

    grouped.push({
      kind: 'childAgentGroup',
      id: `child-agent-${event.toolCallId ?? event.id}-${startSeq}`,
      seq: startSeq,
      timestamp: event.startedAt ?? event.timestamp,
      startedSeq: startSeq,
      endedSeq: terminal ? endSeq : undefined,
      startedAt: event.startedAt ?? event.timestamp,
      endedAt: terminal ? event.endedAt : undefined,
      status: event.status,
      title: event.title,
      toolCallId: event.toolCallId,
      toolEvent: event,
      events: groupChildAgentTimeline(children),
    });
    if (usedSequenceFallback) index = cursor - 1;
  }
  return grouped;
}

function isRenderableEvent(event: AcpUiEventVm) {
  if (hiddenEventKinds.has(event.kind)) return false;
  const sessionUpdate = rawObject(event.raw)?.sessionUpdate;
  return typeof sessionUpdate !== 'string' || !hiddenSessionUpdates.has(sessionUpdate);
}

function isMergeableDelta(kind: string) {
  return kind === 'textDelta' || kind === 'thoughtDelta';
}

function isSameDeltaStream(previous: AcpUiEventVm, event: AcpUiEventVm) {
  return isStableDeltaEvent(previous) && isStableDeltaEvent(event) && previous.kind === event.kind && previous.id === event.id;
}

function isStableDeltaEvent(event: AcpUiEventVm) {
  if (event.kind === 'userTextDelta' && isOptimisticEvent(event)) return false;
  return isMergeableDelta(event.kind);
}

function rawObject(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function arrayValue(value: unknown): unknown[] | null {
  return Array.isArray(value) ? value : null;
}

function mergeRaw(previous: unknown, next: unknown) {
  const previousObject = rawObject(previous);
  const nextObject = rawObject(next);
  if (!previousObject || !nextObject) return next ?? previous;
  const previousMeta = rawObject(previousObject._meta);
  const nextMeta = rawObject(nextObject._meta);
  const previousClaudeCode = rawObject(previousMeta?.claudeCode);
  const nextClaudeCode = rawObject(nextMeta?.claudeCode);
  const merged = { ...previousObject, ...nextObject };
  if (previousMeta || nextMeta) {
    merged._meta = { ...previousMeta, ...nextMeta };
    if (previousClaudeCode || nextClaudeCode) {
      (merged._meta as Record<string, unknown>).claudeCode = { ...previousClaudeCode, ...nextClaudeCode };
    }
  }
  return merged;
}

function mergeAcpEvents(previous: AcpUiEventVm[], next: AcpUiEventVm[]) {
  const byKey = new Map<string, AcpUiEventVm>();
  for (const event of previous) byKey.set(acpEventKey(event), event);
  for (const event of next) byKey.set(acpEventKey(event), event);
  return [...byKey.values()].sort((left, right) => left.seq - right.seq);
}

function limitAcpEvents(events: AcpUiEventVm[], trim: 'start' | 'end') {
  if (events.length <= EVENT_WINDOW_LIMIT) return events;
  return trim === 'start' ? events.slice(events.length - EVENT_WINDOW_LIMIT) : events.slice(0, EVENT_WINDOW_LIMIT);
}

function acpEventKey(event: AcpUiEventVm) {
  if (isStableDeltaEvent(event)) return `${event.sessionId ?? ''}:${event.kind}:${event.id}`;
  return `${event.seq}:${event.id}:${event.kind}`;
}

function mergeOptimisticSession(session: AcpSessionVm | null | undefined, optimisticEvents: AcpUiEventVm[]) {
  if (!session || optimisticEvents.length === 0) return session ?? null;
  const pending = optimisticEvents.filter((event) => !hasMatchingUserPrompt(session.events, event));
  if (pending.length === 0) return session;
  return { ...session, events: [...session.events, ...pending] };
}

export function createAcpPromptId() {
  return `acp-prompt-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function optimisticUserEvent(content: string, promptId = createAcpPromptId()): AcpUiEventVm {
  const createdAt = Math.floor(Date.now() / 1000);
  return {
    id: `optimistic-user-${createdAt}-${Math.random().toString(36).slice(2)}`,
    seq: Number.MAX_SAFE_INTEGER - createdAt,
    timestamp: `${createdAt}Z`,
    kind: 'userTextDelta',
    content,
    status: 'sending',
    raw: { source: 'goldBandPrompt', optimistic: true, promptId },
  };
}

function isOptimisticEvent(event: AcpUiEventVm) {
  return rawObject(event.raw)?.optimistic === true;
}

function hasMatchingUserPrompt(events: AcpUiEventVm[], candidate: AcpUiEventVm) {
  if (candidate.kind !== 'userTextDelta') return false;
  return Boolean(findMatchingGoldBandUserPrompt(events, candidate.content, promptIdFromEvent(candidate)));
}

function findMatchingGoldBandUserPrompt(events: AcpUiEventVm[], content?: string | null, promptId?: string | null) {
  if (promptId) {
    return events.find((event) => isGoldBandUserPrompt(event) && promptIdFromEvent(event) === promptId) ?? null;
  }
  return events.find((event) => isGoldBandUserPrompt(event) && sameText(event.content, content)) ?? null;
}

function sameText(left?: string | null, right?: string | null) {
  return Boolean(left?.trim()) && left?.trim() === right?.trim();
}

function toolDetails(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const toolCall = rawObject(raw?.toolCall) ?? rawObject(raw?.content) ?? raw;
  const fields = rawObject(toolCall?.fields);
  const rawInput = rawObject(toolCall?.rawInput) ?? rawObject(raw?.rawInput);
  const locations = arrayValue(toolCall?.locations) ?? arrayValue(raw?.locations);
  const meta = rawObject(raw?._meta);
  const claudeCode = rawObject(meta?.claudeCode);
  const title = stringValue(toolCall?.title) ?? event.title;
  const claudeToolName = stringValue(claudeCode?.toolName);
  const name = claudeToolName ?? parseToolTitle(title).name ?? stringValue(toolCall?.name) ?? title;
  const output = cleanToolOutput(toolCall?.output ?? raw?.output ?? fields?.output ?? raw?.content);
  return {
    name,
    output,
    queryBlocks: queryBlocksFromTool(title, rawInput, locations),
  };
}

function queryBlocksFromTool(title: string | null | undefined, rawInput?: Record<string, unknown> | null, locations?: unknown[] | null) {
  const parsedTitle = parseToolTitle(title);
  const blocks: Array<{ labelKey: string; value: string }> = [];
  const push = (labelKey: string, value?: string | null) => {
    const normalized = value?.trim();
    if (!normalized || blocks.some((block) => block.value === normalized)) return;
    blocks.push({ labelKey, value: normalized });
  };

  push('acp.toolPath', parsedTitle.scope);
  push('acp.toolQuery', parsedTitle.query);
  push('acp.toolPath', stringValue(rawInput?.file_path));
  push('acp.toolPath', stringValue(rawInput?.path));
  push('acp.toolPath', stringValue(rawInput?.cwd));
  push('acp.toolQuery', stringValue(rawInput?.pattern));
  push('acp.toolQuery', stringValue(rawInput?.query));
  push('acp.toolQuery', stringValue(rawInput?.glob));
  push('acp.toolQuery', stringValue(rawInput?.command));
  push('acp.toolPath', firstLocationPath(locations));
  return blocks;
}

function toolSummary(blocks: Array<{ value: string }>) {
  const values = blocks.map((block) => block.value.trim()).filter(Boolean);
  return values.length > 0 ? values.join(' · ') : undefined;
}

function firstLocationPath(locations?: unknown[] | null) {
  if (!locations) return null;
  for (const location of locations) {
    const path = stringValue(rawObject(location)?.path);
    if (path) return path;
  }
  return null;
}

function parseToolTitle(title: string | null | undefined) {
  if (!title) return { name: null, scope: null, query: null };
  const [name] = title.split(' ');
  const quoted = [...title.matchAll(/`([^`]+)`/g)].map((match) => match[1]);
  const rest = title.slice(name.length).trim();
  const plainScope = rest && rest.toLowerCase() !== 'file' ? rest : null;
  return {
    name: name || title,
    scope: quoted[0] ?? plainScope,
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

function formatToolValue(value: unknown) {
  if (value === null) return 'null';
  if (value === undefined) return 'undefined';
  if (typeof value === 'string') return value;
  if (typeof value === 'object') return JSON.stringify(value, null, 2);
  return String(value);
}

function truncateText(value: string, maxLength: number) {
  return value.length > maxLength ? `${value.slice(0, maxLength)}…` : value;
}

function displayRawDirection(t: ReturnType<typeof useTranslation>['t'], direction?: string | null) {
  if (direction === 'inbound') return t('acp.rawInboundFrame');
  if (direction === 'outbound') return t('acp.rawOutboundFrame');
  return direction ?? t('common.unknown');
}

function rawKindOptions(t: ReturnType<typeof useTranslation>['t']) {
  return [
    { value: 'agent_message_chunk', label: t('acp.rawKindAgentMessage') },
    { value: 'agent_thought_chunk', label: t('acp.rawKindThought') },
    { value: 'tool_call', label: t('acp.rawKindToolCall') },
    { value: 'tool_call_update', label: t('acp.rawKindToolUpdate') },
    { value: 'usage_update', label: t('acp.rawKindUsage') },
    { value: 'available_commands_update', label: t('acp.rawKindCommands') },
    { value: 'session/prompt', label: t('acp.rawKindSessionPrompt') },
    { value: 'session/new', label: t('acp.rawKindSessionNew') },
    { value: 'session/load', label: t('acp.rawKindSessionLoad') },
    { value: 'result', label: t('acp.rawKindResult') },
    { value: 'error', label: t('acp.rawKindError') },
    { value: 'parse-error', label: t('acp.rawKindParseError') },
  ];
}

function displayRawKind(t: ReturnType<typeof useTranslation>['t'], kind: string) {
  const labels: Record<string, string> = {
    initialize: t('acp.rawKindInitialize'),
    'session/new': t('acp.rawKindSessionNew'),
    'session/load': t('acp.rawKindSessionLoad'),
    'session/prompt': t('acp.rawKindSessionPrompt'),
    agent_message_chunk: t('acp.rawKindAgentMessage'),
    agent_thought_chunk: t('acp.rawKindThought'),
    user_message_chunk: t('acp.rawKindUserMessage'),
    tool_call: t('acp.rawKindToolCall'),
    tool_call_update: t('acp.rawKindToolUpdate'),
    usage_update: t('acp.rawKindUsage'),
    available_commands_update: t('acp.rawKindCommands'),
    result: t('acp.rawKindResult'),
    error: t('acp.rawKindError'),
    'parse-error': t('acp.rawKindParseError'),
  };
  return labels[kind] ?? kind;
}

function rawFrameDisplay(content: string) {
  try {
    const value = JSON.parse(content);
    return {
      compact: JSON.stringify(value),
      expanded: wrapLongSegments(JSON.stringify(value, null, 2)),
    };
  } catch {
    return {
      compact: content,
      expanded: wrapLongSegments(content),
    };
  }
}

function rawFramePageSummary(t: ReturnType<typeof useTranslation>['t'], page: AcpRawFramePageVm | null) {
  if (!page || page.total === 0) return t('acp.rawMatchCount', { total: 0 });
  const firstLine = page.items[0]?.lineNumber ?? 0;
  const lastLine = page.items.at(-1)?.lineNumber ?? firstLine;
  return t('acp.rawPageSummary', { start: firstLine, end: lastLine, total: page.total, page: page.page + 1 });
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

function formatElapsedDuration(totalSeconds: number) {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  if (seconds < 60) return `${seconds} 秒`;
  const minutes = Math.floor(seconds / 60);
  const restSeconds = seconds % 60;
  if (minutes < 60) return restSeconds ? `${minutes} 分 ${restSeconds} 秒` : `${minutes} 分`;
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  if (hours < 24) return restMinutes ? `${hours} 时 ${restMinutes} 分` : `${hours} 时`;
  const days = Math.floor(hours / 24);
  const restHours = hours % 24;
  return restHours ? `${days} 天 ${restHours} 时` : `${days} 天`;
}
