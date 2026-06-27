import {
  forwardRef,
  memo,
  startTransition,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  CircleStop,
  Clock,
  FileText,
  Image as ImageIcon,
  ListTodo,
  Loader2,
  Paperclip,
  Search,
  Send,
  ShieldQuestion,
  Terminal,
  UsersRound,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ChainOfThought,
  ChainOfThoughtContent,
  ChainOfThoughtItem,
  ChainOfThoughtStep,
  ChainOfThoughtTrigger,
} from "@/components/prompt-kit/chain-of-thought";
import { Markdown } from "@/components/prompt-kit/markdown";
import { Message, MessageContent } from "@/components/prompt-kit/message";
import {
  PromptInput,
  PromptInputActions,
  PromptInputAction,
  PromptInputTextarea,
} from "@/components/prompt-kit/prompt-input";
import {
  Tool,
  type ToolLabels,
  type ToolParam,
  type ToolPart,
} from "@/components/prompt-kit/tool";
import { cn } from "@/lib/utils";
import { goldThemedScrollbarClassName } from "@/lib/themed-scrollbar";
import {
  decideAcpLiveEventFlush,
  isAcpLiveToolEvent,
  isAcpTextStreamEventKind,
  isCoalescableAcpLiveEvent,
  mergeAcpLiveStreamEvent,
  mergeAcpLiveToolEvent,
  shouldAutoScrollAfterAcpTimelineUpdate,
} from "@/lib/acp-live-flush";
import {
  createAcpSessionConfigViewModel,
  findAcpConfigOption,
  type AcpSessionConfigViewModel,
} from "@/lib/acp-session-config";
import {
  imageSrcFromContent,
  isImageMessageAttachment,
  type MessageAttachmentPreview,
} from "@/lib/asset-preview";
import {
  createAcpSessionAssetPanelViewModel,
  type AcpSessionAssetPanelItem,
} from "@/lib/acp-session-assets-panel";
import { useAttachmentPicker, useWindowDragGuard } from "@/lib/attachment-service";
import { AttachmentChipsList, AttachmentPreviewDialogs } from "@/components/shared/AttachmentComponents";
import { AcpAvatarWithTime } from "@/components/acp/AcpAvatarWithTime";
import { AcpUsagePanel } from "@/components/acp/AcpUsagePanel";
import { HiddenPromptMessageContent } from "@/components/acp/HiddenPromptMessageContent";
import {
  ElicitationCard,
  type ElicitationSchema,
} from "@/components/acp/ElicitationCard";
import {
  attemptIdFromAcpEvent,
  isAcpAttemptSeparator,
  normalizeAcpEventForAttempt,
  normalizeAcpSessionForAttempt,
  originalSeqFromAcpEvent,
} from "@/lib/acp-event-normalization";
import {
  deriveAcpRuntimeComposerState,
  isRuntimeActiveStatus,
  isSessionActiveStatus,
  isSessionCompletedStatus,
  isSessionTerminalStatus,
} from "@/lib/acp-runtime-composer-state";
import {
  hasAcpSessionMetadata,
  missingAcpSessionRetryDelay,
  resolveAcpSessionShellState,
  shouldCreateLiveAcpSessionShell,
} from "@/lib/acp-session-shell";
import { formatLocalDateTime } from "@/lib/datetime";
import {
  getAcpRawFrames,
  getAcpSession,
  respondAcpPermission,
  respondElicitation,
  submitConversationPrompt,
  setAcpSessionModel,
  setAcpSessionPermissionMode,
  showArtifact,
  showAttachment,
  showConversationAttachment,
  stopActiveSession,
  submitManualCheck,
} from "@/api";
import { subscribeAcpSessionUpdates } from "@/api";
import { getRuntimeApi } from "@/api/client";
import { isTauriRuntime } from "@/api/shared";
import { displayAppError, displayStatus } from "@/i18n";
import type {
  AcpPermissionRequestVm,
  AcpRawFramePageVm,
  AcpRawFrameQueryInput,
  AcpRawFrameVm,
  AcpSessionVm,
  AcpUiEventVm,
  AcpUsageVm,
  AssetItemVm,
  ContentVm,
  ConversationAttemptLifecycleVm,
} from "@/types";

export type AcpLifecycleSnapshot = {
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  session?: AcpSessionVm | null;
  lifecycle: ConversationAttemptLifecycleVm;
};

export type AcpRuntimeComposerContext = {
  lifecycle?: ConversationAttemptLifecycleVm | null;
  runtimeStatus?: string | null;
  workflowValid: boolean;
  workflowError?: string | null;
  pauseMessage?: string | null;
  runtimeError?: string | null;
  onRepair?: () => void;
};

export interface ACPChatDialogHandle {
  openArtifactsDialog: (asset?: AssetItemVm) => void;
}

interface ACPChatDialogProps {
  session?: AcpSessionVm | null;
  projectId: string;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  runtimeComposerContext?: AcpRuntimeComposerContext;
  manualCheckPending?: boolean;
  systemPromptOptions?: Array<{ attemptId: string; prompt?: string | null }>;
  eventIdPrefix?: string;
  eventPageSize?: number;
  liveUpdatesPaused?: boolean;
  optimisticEvents?: AcpUiEventVm[];
  onOptimisticEventsChange?: (events: AcpUiEventVm[]) => void;
  onManualCheckSubmitted?: () => void;
  onSessionStopped?: () => void;
  onLifecycleSnapshot?: (snapshot: AcpLifecycleSnapshot) => void;
  onAtBottomChange?: (atBottom: boolean) => void;
  allowEventOnlySessionShell?: boolean;
  artifacts?: AssetItemVm[];
  attachments?: AssetItemVm[];
  allArtifacts?: AssetItemVm[];
  allAttachments?: AssetItemVm[];
  usageCompact?: boolean;
}

type AcpCanvasMode = "chat" | "raw";

type ToolTone = "muted" | "pending" | "running" | "success" | "danger";
type AcpProcessingKind =
  | "sending"
  | "launching"
  | "processing"
  | "thinking"
  | "tool"
  | "responding"
  | "stopping"
  | "launching-next-node";
type AcpTimelineEvent = AcpUiEventVm & {
  startedAt?: string;
  endedAt?: string;
  startedSeq?: number;
  endedSeq?: number;
  durationMs?: number;
  optimistic?: boolean;
};

type AcpChildAgentGroup = {
  kind: "childAgentGroup";
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
type AcpExpandedItems = Record<string, boolean>;
type AcpExpansionControls = {
  expandedItems: AcpExpandedItems;
  onOpenChange: (key: string, open: boolean) => void;
};

const DEFAULT_EVENT_PAGE_SIZE = 360;
const DEFAULT_LOADED_EVENT_BUFFER_LIMIT = 360;
const MIN_LOADED_EVENT_BUFFER_LIMIT = 30;
const HISTORY_LOAD_THRESHOLD_PX = 240;
const BOTTOM_STICK_THRESHOLD_PX = 48;
const LIVE_EVENT_FLUSH_MS = 125;
const LIVE_EVENT_INTERACTION_QUIET_MS = 180;

export const ACP_SESSION_SCROLL_AREA_CLASS_NAME = goldThemedScrollbarClassName(
  "h-full min-w-0 overflow-y-auto",
);
export const ACP_RAW_SCROLL_AREA_CLASS_NAME = goldThemedScrollbarClassName(
  "h-full overflow-y-auto p-5",
);

function timelineEventKey(event: AcpTimelineItem) {
  if (isChildAgentGroup(event)) return event.id;
  if (
    (event.kind === "toolCall" || event.kind === "toolCallUpdate") &&
    event.toolCallId
  )
    return `tool-${event.toolCallId}`;
  return `${event.kind}-${event.id}`;
}

const hiddenSessionUpdates = new Set([
  "available_commands_update",
  "usage_update",
  "session_info_update",
  "current_mode_update",
  "config_option_update",
]);

const hiddenEventKinds = new Set([
  "availableCommands",
  "usageUpdate",
  "sessionInfo",
  "modeUpdate",
  "configUpdate",
  "permissionRequest",
  "elicitationRequest",
  "elicitationResponse",
  "rawDiagnostic",
  "runtimeError",
]);

const optimisticEventStore = new Map<string, AcpUiEventVm[]>();
const optimisticEventListeners = new Map<
  string,
  Set<(events: AcpUiEventVm[]) => void>
>();

function readStoredOptimisticEvents(sessionKey: string) {
  return optimisticEventStore.get(sessionKey) ?? [];
}

function updateStoredOptimisticEvents(
  sessionKey: string,
  updater: (current: AcpUiEventVm[]) => AcpUiEventVm[],
) {
  const next = updater(readStoredOptimisticEvents(sessionKey));
  if (next.length === 0) optimisticEventStore.delete(sessionKey);
  else optimisticEventStore.set(sessionKey, next);
  optimisticEventListeners
    .get(sessionKey)
    ?.forEach((listener) => listener(next));
  return next;
}

export function updateAcpOptimisticEvents(
  sessionKey: string,
  updater: (current: AcpUiEventVm[]) => AcpUiEventVm[],
) {
  return updateStoredOptimisticEvents(sessionKey, updater);
}

function subscribeStoredOptimisticEvents(
  sessionKey: string,
  listener: (events: AcpUiEventVm[]) => void,
) {
  const listeners =
    optimisticEventListeners.get(sessionKey) ??
    new Set<(events: AcpUiEventVm[]) => void>();
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
    if (event.kind === "userTextDelta" && event.status === "sending")
      return event;
  }
  return null;
}

const ACP_EVENT_STORE_MAX_KEYS = 12;

const acpLoadedEventStore = new Map<string, AcpUiEventVm[]>();
const acpEventStoreAccessOrder: string[] = [];

function touchAcpEventStoreKey(sessionKey: string) {
  const idx = acpEventStoreAccessOrder.indexOf(sessionKey);
  if (idx !== -1) acpEventStoreAccessOrder.splice(idx, 1);
  acpEventStoreAccessOrder.push(sessionKey);
}

function evictAcpEventStoreIfNeeded() {
  while (acpEventStoreAccessOrder.length > ACP_EVENT_STORE_MAX_KEYS) {
    const oldest = acpEventStoreAccessOrder.shift();
    if (oldest !== undefined) acpLoadedEventStore.delete(oldest);
  }
}

export function restoreAcpLoadedEvents(
  sessionKey: string,
  events: AcpUiEventVm[],
  eventPageSize: number,
) {
  const stored = acpLoadedEventStore.get(sessionKey) ?? [];
  if (stored.length > 0) touchAcpEventStoreKey(sessionKey);
  return limitAcpEvents(
    stored.length > 0 ? mergeAcpEvents(stored, events) : events,
    "start",
    eventPageSize,
  );
}

export function storeAcpLoadedEvents(
  sessionKey: string,
  events: AcpUiEventVm[],
  eventPageSize: number,
) {
  if (events.length === 0) {
    acpLoadedEventStore.delete(sessionKey);
    const idx = acpEventStoreAccessOrder.indexOf(sessionKey);
    if (idx !== -1) acpEventStoreAccessOrder.splice(idx, 1);
  } else {
    if (!acpLoadedEventStore.has(sessionKey)) touchAcpEventStoreKey(sessionKey);
    acpLoadedEventStore.set(
      sessionKey,
      limitAcpEvents(events, "start", eventPageSize),
    );
    evictAcpEventStoreIfNeeded();
  }
}

function normalizeEventPageSize(value?: number) {
  return Number.isFinite(value) && value && value > 0
    ? Math.floor(value)
    : DEFAULT_EVENT_PAGE_SIZE;
}

function loadedEventBufferLimit(eventPageSize: number) {
  return Math.max(
    MIN_LOADED_EVENT_BUFFER_LIMIT,
    Math.min(DEFAULT_LOADED_EVENT_BUFFER_LIMIT, eventPageSize * 3),
  );
}

export const ACPChatDialog = forwardRef<
  ACPChatDialogHandle,
  ACPChatDialogProps
>(function ACPChatDialog(
  {
    session,
    projectId,
    taskId,
    runId,
    roundId,
    nodeId,
    attemptId,
    outerNodeId,
    outerAttemptId,
    runtimeComposerContext,
    manualCheckPending = false,
    systemPromptOptions,
    eventIdPrefix,
    eventPageSize,
    liveUpdatesPaused: externalLiveUpdatesPaused = false,
    optimisticEvents: controlledOptimisticEvents,
    onOptimisticEventsChange,
    onManualCheckSubmitted,
    onSessionStopped,
    onLifecycleSnapshot,
    onAtBottomChange,
    allowEventOnlySessionShell = true,
    artifacts = [],
    attachments = [],
    allArtifacts,
    allAttachments,
    usageCompact,
  },
  ref,
) {
  const { t } = useTranslation();
  const effectiveEventPageSize = normalizeEventPageSize(eventPageSize);
  const effectiveLoadedEventBufferLimit = loadedEventBufferLimit(
    effectiveEventPageSize,
  );
  const sessionKey = `${taskId}:${runId}:${roundId}:${nodeId}:${attemptId}`;
  const eventWindowKey = `${sessionKey}:${outerNodeId ?? ""}:${outerAttemptId ?? ""}:${eventIdPrefix ?? ""}`;
  const sessionIdentity = eventWindowKey;
  const restoredOptimisticEvents =
    controlledOptimisticEvents ?? readStoredOptimisticEvents(sessionKey);
  const restoredLoadedEvents = restoreAcpLoadedEvents(
    eventWindowKey,
    session?.events ?? [],
    effectiveLoadedEventBufferLimit,
  );
  const restoredPromptEvent = latestSendingOptimisticEvent(
    restoredOptimisticEvents,
  );
  const restoredPrompt = restoredPromptEvent?.content?.trim() || null;
  const restoredPromptId = promptIdFromEvent(restoredPromptEvent);
  const [currentSession, setCurrentSession] = useState<AcpSessionVm | null>(
    session ?? null,
  );
  const [loadedEvents, setLoadedEvents] = useState<AcpUiEventVm[]>(
    () => restoredLoadedEvents,
  );
  const loadedEventsRef = useRef<AcpUiEventVm[]>(restoredLoadedEvents);
  const [optimisticEvents, setOptimisticEvents] = useState<AcpUiEventVm[]>(
    () => restoredOptimisticEvents,
  );
  const [prompt, setPrompt] = useState("");
  const [sending, setSending] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [awaitingResponse, setAwaitingResponse] = useState(
    Boolean(restoredPromptEvent),
  );
  const [activeTurnPrompt, setActiveTurnPrompt] = useState<string | null>(
    restoredPrompt,
  );
  const [activeTurnPromptId, setActiveTurnPromptId] = useState<string | null>(
    restoredPromptId,
  );
  const [activeTurnStartedAt, setActiveTurnStartedAt] = useState<string | null>(
    null,
  );
  const [sendError, setSendError] = useState<string | null>(null);
  const [cancelError, setCancelError] = useState<string | null>(null);
  const [manualCheckError, setManualCheckError] = useState<string | null>(null);
  const [manualCheckSubmitting, setManualCheckSubmitting] = useState(false);
  const [manualCheckResolved, setManualCheckResolved] = useState(false);
  const [canvasMode, setCanvasMode] = useState<AcpCanvasMode>("chat");
  const [expandedItems, setExpandedItems] = useState<AcpExpandedItems>({});
  const [systemPromptOpen, setSystemPromptOpen] = useState(false);
  const [rawPage, setRawPage] = useState<AcpRawFramePageVm | null>(null);
  const [rawQuery, setRawQuery] = useState<AcpRawFrameQueryInput>({
    page: 0,
    pageSize: 100,
  });
  const [rawLoading, setRawLoading] = useState(false);
  const [loadingInitialSession, setLoadingInitialSession] = useState(!session && isTauriRuntime());
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [hasOlderEvents, setHasOlderEvents] = useState(
    () => session?.eventPage.hasOlder ?? false,
  );
  const [hasNewerEvents, setHasNewerEvents] = useState(
    () => session?.eventPage.hasNewer ?? false,
  );
  const [isAtBottom, setIsAtBottom] = useState(true);
  const [dismissedPermissionIds, setDismissedPermissionIds] = useState<
    Set<string>
  >(() => new Set());
  const [permissionError, setPermissionError] = useState<string | null>(null);
  const [answeredElicitations, setAnsweredElicitations] = useState<
    Map<string, Record<string, unknown>>
  >(() => new Map());
  const [queuedInterventionPrompt, setQueuedInterventionPrompt] = useState<
    string | null
  >(null);
  const [artifactsDialogOpen, setArtifactsDialogOpen] = useState(false);
  const [selectedArtifact, setSelectedArtifact] = useState<AssetItemVm | null>(
    null,
  );
  const [artifactContent, setArtifactContent] = useState<ContentVm | null>(
    null,
  );
  const [artifactLoading, setArtifactLoading] = useState(false);
  const [messageImagePreview, setMessageImagePreview] = useState<{
    name: string;
    src: string;
  } | null>(null);
  const {
    attachments: pendingAttachments,
    fileError,
    fileInputRef,
    pickFiles,
    handleFilesFromInput,
    removeAttachment,
    clearAttachments,
    resolveAttachmentPaths,
    dropZoneHandlers,
    extractPasteFiles,
    previewImage,
    setPreviewImage,
    textPreview,
    setTextPreview,
    handlePreviewAttachment,
  } = useAttachmentPicker();
  useWindowDragGuard();
  const loadingOlderRef = useRef(false);
  const loadingNewerRef = useRef(false);
  const preservingScrollRef = useRef(false);
  const programmaticScrollRef = useRef(false);
  const pinToBottomRef = useRef(true);
  const cancelRequestedRef = useRef(false);
  const awaitTerminalStopRef = useRef(false);
  const terminalSessionNotifiedRef = useRef(false);
  const [stopCommandPending, setStopCommandPending] = useState(false);
  const [stopOverlayPending, setStopOverlayPending] = useState(false);
  const [runtimeStopAccepted, setRuntimeStopAccepted] = useState(false);
  const [localRuntimeLifecycle, setLocalRuntimeLifecycle] = useState<ConversationAttemptLifecycleVm | null>(null);
  const latestSessionRef = useRef<AcpSessionVm | null>(session ?? null);
  const sessionRefreshSeqRef = useRef(0);
  const configGenerationRef = useRef(0);
  const scrollerElementRef = useRef<HTMLDivElement | null>(null);
  const prependAnchorRef = useRef<{ key: string; top: number } | null>(null);
  const pendingLiveEventsRef = useRef<Map<string, AcpUiEventVm>>(new Map());
  const liveEventFlushTimerRef = useRef<number | null>(null);
  const liveUpdatesDeferredUntilRef = useRef(0);
  const liveUpdatesPausedRef = useRef(false);
  const hydratedSessionKeysRef = useRef(new Set<string>());
  const hydrationInflightRef = useRef(false);
  const liveUpdatesPaused = Boolean(
    externalLiveUpdatesPaused || systemPromptOpen || artifactsDialogOpen,
  );
  liveUpdatesPausedRef.current = liveUpdatesPaused;

  const updateOptimisticEvents = (
    updater: (current: AcpUiEventVm[]) => AcpUiEventVm[],
  ) => {
    const next = updateStoredOptimisticEvents(sessionKey, updater);
    setOptimisticEvents(next);
    onOptimisticEventsChange?.(next);
  };

  useEffect(() => {
    if (controlledOptimisticEvents)
      setOptimisticEvents(controlledOptimisticEvents);
  }, [controlledOptimisticEvents]);

  useEffect(
    () => subscribeStoredOptimisticEvents(sessionKey, setOptimisticEvents),
    [sessionKey],
  );

  useEffect(() => {
    setManualCheckResolved(false);
    setManualCheckSubmitting(false);
    setManualCheckError(null);
    setLocalRuntimeLifecycle(null);
  }, [attemptId, manualCheckPending, nodeId, roundId, runId, taskId]);

  useEffect(() => {
    if (runtimeComposerContext?.lifecycle) setLocalRuntimeLifecycle(null);
  }, [runtimeComposerContext?.lifecycle]);

  useEffect(() => {
    setCurrentSession(session ?? null);
    if (session) setLoadingInitialSession(false);
    if (!session) {
      const restored = restoreAcpLoadedEvents(
        eventWindowKey,
        [],
        effectiveLoadedEventBufferLimit,
      );
      loadedEventsRef.current = restored;
      setLoadedEvents(restored);
      setHasOlderEvents(false);
      setHasNewerEvents(false);
      return;
    }
    setLoadedEvents((events) => {
      const currentEvents =
        events.length === 0
          ? restoreAcpLoadedEvents(
              eventWindowKey,
              session.events,
              effectiveLoadedEventBufferLimit,
            )
          : events;
      const merged = mergeAcpEvents(currentEvents, session.events);
      const limited = limitAcpEvents(
        merged,
        "start",
        effectiveLoadedEventBufferLimit,
      );
      loadedEventsRef.current = limited;
      return limited;
    });
    setHasOlderEvents((current) => current || session.eventPage.hasOlder);
    setHasNewerEvents((current) => current || session.eventPage.hasNewer);
  }, [effectiveLoadedEventBufferLimit, eventWindowKey, session]);

  useEffect(() => {
    const storedOptimisticEvents =
      controlledOptimisticEvents ?? readStoredOptimisticEvents(sessionKey);
    const storedLoadedEvents = restoreAcpLoadedEvents(
      eventWindowKey,
      session?.events ?? [],
      effectiveLoadedEventBufferLimit,
    );
    const storedPromptEvent = latestSendingOptimisticEvent(
      storedOptimisticEvents,
    );
    setCurrentSession(session ?? null);
    setLoadingInitialSession(!session && isTauriRuntime());
    loadedEventsRef.current = storedLoadedEvents;
    setLoadedEvents(storedLoadedEvents);
    setOptimisticEvents(storedOptimisticEvents);
    setDismissedPermissionIds(new Set());
    setPermissionError(null);
    setSendError(null);
    setCancelError(null);
    setCancelling(false);
    setStopCommandPending(false);
    setStopOverlayPending(false);
    setRuntimeStopAccepted(false);
    setAwaitingResponse(Boolean(storedPromptEvent));
    setActiveTurnPrompt(storedPromptEvent?.content?.trim() || null);
    setActiveTurnPromptId(promptIdFromEvent(storedPromptEvent));
    setActiveTurnStartedAt(null);
    setRawPage(null);
    setRawQuery({ page: 0, pageSize: 100 });
    setLoadingOlder(false);
    setExpandedItems({});
    setHasOlderEvents(session?.eventPage.hasOlder ?? false);
    setHasNewerEvents(session?.eventPage.hasNewer ?? false);
    setIsAtBottom(true);
    loadingOlderRef.current = false;
    loadingNewerRef.current = false;
    preservingScrollRef.current = false;
    programmaticScrollRef.current = false;
    prependAnchorRef.current = null;
    liveUpdatesDeferredUntilRef.current = 0;
    pinToBottomRef.current = true;
    cancelRequestedRef.current = false;
    awaitTerminalStopRef.current = false;
    terminalSessionNotifiedRef.current = false;
    latestSessionRef.current = session ?? null;
    sessionRefreshSeqRef.current += 1;
    hydratedSessionKeysRef.current.clear();
    hydrationInflightRef.current = false;
    setCanvasMode("chat");
  }, [effectiveLoadedEventBufferLimit, sessionIdentity]);

  useEffect(() => {
    loadedEventsRef.current = loadedEvents;
    storeAcpLoadedEvents(
      eventWindowKey,
      loadedEvents,
      effectiveLoadedEventBufferLimit,
    );
  }, [effectiveLoadedEventBufferLimit, eventWindowKey, loadedEvents]);

  useEffect(() => {
    onAtBottomChange?.(isAtBottom);
  }, [isAtBottom, onAtBottomChange]);

  const baseSession = currentSession ?? session;
  const runtimeActiveFromContext = !runtimeStopAccepted && (runtimeComposerContext?.lifecycle?.runtime.active ?? isRuntimeActiveStatus(runtimeComposerContext?.runtimeStatus));
  const liveSessionShell = useMemo(
    () =>
      shouldCreateLiveAcpSessionShell({
        runtimeActive: runtimeActiveFromContext,
        allowEventOnlySessionShell,
        loadedEventCount: loadedEvents.length,
      })
        ? createLiveAcpSessionShell(loadedEvents, "running")
        : null,
    [allowEventOnlySessionShell, loadedEvents, runtimeActiveFromContext],
  );
  const visibleSession = useMemo(
    () =>
      baseSession ? { ...baseSession, events: loadedEvents } : liveSessionShell,
    [baseSession, liveSessionShell, loadedEvents],
  );
  const pendingOptimisticPrompt = latestSendingOptimisticEvent(
    optimisticEvents.filter(
      (event) => !hasMatchingUserPrompt(loadedEvents, event),
    ),
  );
  const effective = useMemo(
    () => mergeOptimisticSession(visibleSession, optimisticEvents),
    [visibleSession, optimisticEvents],
  );
  const sessionConfigViewModel = useMemo(
    () => createAcpSessionConfigViewModel(effective?.config),
    [effective?.config],
  );
  const effectiveEvents = effective?.events ?? [];
  const effectiveSessionTerminal = isSessionTerminalStatus(effective?.status);
  const hasResponseAfterActiveTurn = hasResponseAfterTurn(effectiveEvents, activeTurnStartedAt);
  const localTurnInFlight = sending || Boolean(pendingOptimisticPrompt) || (awaitingResponse && Boolean(activeTurnPrompt || activeTurnPromptId));
  const activeAwaitingResponse = awaitingResponse && (!effectiveSessionTerminal || localTurnInFlight);
  const waitingForOptimisticPrompt =
    Boolean(pendingOptimisticPrompt) &&
    !hasResponseAfterActiveTurn;
  const localSubmissionPending = sending || waitingForOptimisticPrompt;
  const runtimeActive = runtimeActiveFromContext && !(isSessionCompletedStatus(effective?.status ?? baseSession?.status) && !localSubmissionPending);
  const pendingPermission =
    effective?.pendingPermissions?.find(
      (request) => !dismissedPermissionIds.has(request.requestId),
    ) ?? pendingPermissionFromEvents(effectiveEvents, dismissedPermissionIds);
  const waitingForPermission = Boolean(pendingPermission);
  const pendingElicitation = pendingElicitationFromEvents(
    effectiveEvents,
    answeredElicitations,
  );
  const planInterventionOption = pendingPermission
    ? findPlanInterventionOption(pendingPermission)
    : null;
  const todoEntries = useMemo(() => {
    const planEvents = effectiveEvents.filter(
      (e) => e.kind === "plan" && isTopLevelPlanEvent(e),
    );
    if (planEvents.length === 0) return [];
    const lastPlan = planEvents[planEvents.length - 1];
    const raw = rawObject(lastPlan.raw);
    const entries = (raw?.entries ?? []) as Array<{
      content?: string;
      status?: string;
      priority?: string;
    }>;
    return entries.filter((e) => e.content);
  }, [effectiveEvents]);
  const rebuiltTimeline = useMemo(
    () => buildAcpTimeline(effectiveEvents),
    [effectiveEvents],
  );
  const timeline = useStableAcpTimeline(rebuiltTimeline);
  const acpSessionActive = isSessionActiveStatus(effective?.status);
  const sessionActive = acpSessionActive || runtimeActive;
  const streamingTextItemKey = sessionActive
    ? latestStreamingTextItemKey(timeline)
    : null;
  const handleTimelineItemOpenChange = useCallback(
    (key: string, open: boolean) => {
      setExpandedItems((current) =>
        current[key] === open ? current : { ...current, [key]: open },
      );
    },
    [],
  );
  const expansionControls = useMemo<AcpExpansionControls>(
    () => ({
      expandedItems,
      onOpenChange: handleTimelineItemOpenChange,
    }),
    [expandedItems, handleTimelineItemOpenChange],
  );

  const handleOpenArtifactDetail = useCallback(
    async (asset: AssetItemVm) => {
      setArtifactsDialogOpen(true);
      setSelectedArtifact(asset);
      setArtifactContent(null);
      setArtifactLoading(true);
      try {
        let content: ContentVm;
        if (asset.kind === "input-attachment") {
          content = await showConversationAttachment(projectId, taskId, asset.name);
        } else {
          const loader =
            asset.kind === "attachment" ? showAttachment : showArtifact;
          const assetOuterNodeId =
            outerNodeId && outerAttemptId ? outerNodeId : undefined;
          const assetOuterAttemptId =
            outerNodeId && outerAttemptId ? outerAttemptId : undefined;
          content = await loader(
            projectId,
            taskId,
            runId,
            asset.roundId || roundId,
            asset.nodeId,
            asset.attemptId,
            asset.name,
            assetOuterNodeId,
            assetOuterAttemptId,
          );
        }
        setArtifactContent(content);
      } catch {
        setArtifactContent(null);
      } finally {
        setArtifactLoading(false);
      }
    },
    [projectId, taskId, runId, roundId],
  );

  const handleArtifactsDialogOpenChange = useCallback((open: boolean) => {
    setArtifactsDialogOpen(open);
    if (!open) {
      setSelectedArtifact(null);
      setArtifactContent(null);
      setArtifactLoading(false);
    }
  }, []);

  const handleOpenMessageAttachment = useCallback(
    async (attachment: MessageAttachmentPreview) => {
      if (isImageMessageAttachment(attachment)) {
        try {
          const content = await showConversationAttachment(projectId, taskId, attachment.name);
          const src = imageSrcFromContent(content);
          if (src) {
            setMessageImagePreview({ name: attachment.name, src });
            return;
          }
        } catch {
          // Fall through to the attachment detail dialog.
        }
      }
      await handleOpenArtifactDetail({
        kind: 'input-attachment',
        name: attachment.name,
        title: attachment.name,
        tone: 'neutral',
        preview: '',
        roundId: '',
        nodeId: '',
        attemptId: '',
      });
    },
    [handleOpenArtifactDetail, taskId],
  );

  useImperativeHandle(
    ref,
    () => ({
      openArtifactsDialog: (asset?: AssetItemVm) => {
        if (asset) {
          handleOpenArtifactDetail(asset);
        } else {
          setArtifactsDialogOpen(true);
        }
      },
    }),
    [handleOpenArtifactDetail],
  );

  useEffect(() => {
    const keys = new Set(timeline.map(timelineEventKey));
    setExpandedItems((current) => {
      let changed = false;
      const next: AcpExpandedItems = {};
      for (const [key, open] of Object.entries(current)) {
        if (!keys.has(key)) {
          changed = true;
          continue;
        }
        next[key] = open;
      }
      return changed ? next : current;
    });
  }, [timeline]);

  const showManualCheckActions = manualCheckPending && !manualCheckResolved;
  const localLifecycle = localRuntimeLifecycle
    ?? (runtimeStopAccepted && runtimeComposerContext?.lifecycle
      ? {
          ...runtimeComposerContext.lifecycle,
          runtime: {
            ...runtimeComposerContext.lifecycle.runtime,
            status: "paused",
            pauseReason: "process-interrupted",
            resumable: true,
            active: false,
            continuable: true,
            phase: "paused",
          },
          displayStatus: "paused",
          continueKind: "input",
          composer: {
            mode: "interrupted-input",
            submitTarget: "runtime-continue",
            processingKind: "processing",
            statusKey: null,
            canStop: false,
            lockInput: false,
          },
        }
      : runtimeComposerContext?.lifecycle);
  const composerLatestEvent = timeline.at(-1) ?? null;
  const turnAccepted = Boolean(activeTurnStartedAt);
  const hasTurnResponse = hasResponseAfterActiveTurn;
  const composerState = deriveAcpRuntimeComposerState({
    lifecycle: localLifecycle,
    workflowValid: runtimeComposerContext?.workflowValid ?? true,
    workflowInvalidMessage: runtimeComposerContext?.workflowError,
    pauseMessage: runtimeComposerContext?.pauseMessage,
    runtimeErrorMessage: runtimeComposerContext?.runtimeError,
    acpStatus: effective?.status,
    prompt,
    waitingForPermission,
    hasPlanIntervention: Boolean(planInterventionOption),
    sending,
    awaitingResponse: activeAwaitingResponse,
    waitingForOptimisticPrompt,
    localTurnInFlight,
    cancelling,
    stopCommandPending,
    turnAccepted,
    hasResponseAfterTurn: hasTurnResponse,
    hasTimelineItems: timeline.length > 0,
    hasEffectiveEvents: effectiveEvents.length > 0,
    timelineProcessingKind: processingKindFromTimeline(composerLatestEvent, false),
  });
  const stopInProgress = composerState.stopInProgress;
  const composerLocked = composerState.composerLocked;
  const composerInputDisabled = composerState.inputDisabled;
  const composerStatusActive = composerState.statusActive;
  const composerSessionSeconds = effective?.sessionElapsedSeconds ?? null;
  const composerProcessingKind: AcpProcessingKind = composerState.processingKind;
  const showComposerStatus = composerState.showStatus || composerSessionSeconds != null;
  const composerStatusStartAt =
    composerState.mode === "submitting" ||
    composerState.mode === "stopping" ||
    (activeAwaitingResponse && turnAccepted && !hasTurnResponse)
      ? activeTurnStartedAt
      : (composerLatestEvent?.startedAt ??
        composerLatestEvent?.timestamp ??
        activeTurnStartedAt);
  const usageStepSeconds = useElapsedSeconds(
    composerStatusActive && composerProcessingKind !== "sending",
    composerStatusStartAt,
  );
  const composerStatusLabel = processingLabel(t, composerProcessingKind);
  const composerInputHint = composerHintText(
    composerState,
    composerStatusLabel,
    t,
  );
  const composerPlaceholder = composerPlaceholderText(composerState, t);
  const canSubmitPrompt = composerState.canSubmit;
  const canStopSession = composerState.canStop;
  const sendButtonBusy =
    (sending || waitingForOptimisticPrompt) && !planInterventionOption;
  const lastEvent = effectiveEvents.at(-1);

  const normalizeSessionUpdate = useCallback(
    (updated: AcpSessionVm | null) =>
      eventIdPrefix
        ? normalizeAcpSessionForAttempt(updated, eventIdPrefix)
        : updated,
    [eventIdPrefix],
  );
  const normalizeEventUpdate = useCallback(
    (event: AcpUiEventVm | null | undefined) =>
      event && eventIdPrefix
        ? normalizeAcpEventForAttempt(event, eventIdPrefix)
        : (event ?? null),
    [eventIdPrefix],
  );

  const applySessionUpdate = useCallback((updated: AcpSessionVm | null) => {
    const normalized = normalizeSessionUpdate(updated);
    const previous = latestSessionRef.current;
    const equivalent = sessionsEquivalent(previous, normalized);
    if (equivalent) return;
    latestSessionRef.current = normalized;
    setCurrentSession(normalized);
    if (!normalized) return;
    setLoadedEvents((events) => {
      setHasNewerEvents(normalized.eventPage.hasNewer);
      const merged = mergeAcpEvents(events, normalized.events);
      const limited = limitAcpEvents(
        merged,
        "start",
        effectiveLoadedEventBufferLimit,
      );
      setHasOlderEvents(
        normalized.eventPage.hasOlder || limited.length < merged.length,
      );
      loadedEventsRef.current = limited;
      return limited;
    });
  }, [effectiveLoadedEventBufferLimit, normalizeSessionUpdate]);

  const emitLifecycleSnapshot = useCallback((lifecycle: ConversationAttemptLifecycleVm | null | undefined, sessionSnapshot?: AcpSessionVm | null) => {
    if (!lifecycle) return;
    onLifecycleSnapshot?.({
      taskId,
      runId,
      roundId,
      nodeId,
      attemptId,
      outerNodeId,
      outerAttemptId,
      session: normalizeSessionUpdate(sessionSnapshot ?? latestSessionRef.current),
      lifecycle,
    });
  }, [
    attemptId,
    nodeId,
    normalizeSessionUpdate,
    onLifecycleSnapshot,
    outerAttemptId,
    outerNodeId,
    roundId,
    runId,
    taskId,
  ]);

  const patchSessionConfig = useCallback((patch: Partial<NonNullable<AcpSessionVm["config"]>>) => {
    const base = latestSessionRef.current;
    if (!base) return;
    const updated: AcpSessionVm = {
      ...base,
      config: {
        ...(base.config ?? {}),
        ...patch,
      },
    };
    configGenerationRef.current += 1;
    latestSessionRef.current = updated;
    setCurrentSession(updated);
  }, []);

  const handleAcpSessionModelChange = useCallback((modelId: string) => {
    const config = latestSessionRef.current?.config;
    const selected = findAcpConfigOption(
      config?.models,
      config?.configOptions,
      "model",
      modelId,
    );
    patchSessionConfig({
      currentModelId: modelId,
      currentModelName: selected.name,
    });
    setAcpSessionModel(
      projectId,
      taskId,
      runId,
      roundId,
      nodeId,
      attemptId,
      modelId,
      outerNodeId,
      outerAttemptId,
    )
      .then((updated) => {
        if (updated) {
          configGenerationRef.current = Math.max(0, configGenerationRef.current - 1);
          applySessionUpdate(updated);
        }
      })
      .catch((error) => {
        configGenerationRef.current = Math.max(0, configGenerationRef.current - 1);
        console.error("Failed to set ACP session model:", error);
      });
  }, [
    applySessionUpdate,
    attemptId,
    nodeId,
    outerAttemptId,
    outerNodeId,
    patchSessionConfig,
    roundId,
    runId,
    taskId,
  ]);

  const handleAcpSessionPermissionModeChange = useCallback((permissionModeId: string) => {
    const config = latestSessionRef.current?.config;
    const selected = findAcpConfigOption(
      config?.modes,
      config?.configOptions,
      "mode",
      permissionModeId,
    );
    patchSessionConfig({
      currentModeId: permissionModeId,
      currentModeName: selected.name,
    });
    setAcpSessionPermissionMode(
      projectId,
      taskId,
      runId,
      roundId,
      nodeId,
      attemptId,
      permissionModeId,
      outerNodeId,
      outerAttemptId,
    )
      .then((updated) => {
        if (updated) {
          configGenerationRef.current = Math.max(0, configGenerationRef.current - 1);
          applySessionUpdate(updated);
        }
      })
      .catch((error) => {
        configGenerationRef.current = Math.max(0, configGenerationRef.current - 1);
        console.error("Failed to set ACP session permission mode:", error);
      });
  }, [
    applySessionUpdate,
    attemptId,
    nodeId,
    outerAttemptId,
    outerNodeId,
    patchSessionConfig,
    roundId,
    runId,
    taskId,
  ]);

  const applyEventUpdates = useCallback((updates: AcpUiEventVm[]) => {
    const normalizedUpdates = updates
      .map((event) => normalizeEventUpdate(event))
      .filter((event): event is AcpUiEventVm => {
        if (!event) return false;
        return (
          isRenderableEvent(event) ||
          event.kind === "permissionRequest" ||
          event.kind === "elicitationRequest" ||
          event.kind === "elicitationResponse"
        );
      });
    if (normalizedUpdates.length === 0) return;
    setLoadedEvents((events) => {
      setHasNewerEvents(false);
      const merged = mergeAcpEvents(events, normalizedUpdates);
      const limited = limitAcpEvents(
        merged,
        "start",
        effectiveLoadedEventBufferLimit,
      );
      setHasOlderEvents((current) => current || limited.length < merged.length);
      loadedEventsRef.current = limited;
      return limited;
    });
  }, [effectiveLoadedEventBufferLimit, normalizeEventUpdate]);

  const applyEventUpdate = useCallback((event: AcpUiEventVm | null | undefined) => {
    if (!event) return;
    applyEventUpdates([event]);
  }, [applyEventUpdates]);

  const flushPendingLiveEvents = useCallback((priority: "sync" | "transition" = "transition") => {
    if (liveEventFlushTimerRef.current !== null) {
      window.clearTimeout(liveEventFlushTimerRef.current);
      liveEventFlushTimerRef.current = null;
    }
    const updates = [...pendingLiveEventsRef.current.values()];
    pendingLiveEventsRef.current.clear();
    if (updates.length === 0) return;
    if (priority === "sync") {
      applyEventUpdates(updates);
      return;
    }
    startTransition(() => applyEventUpdates(updates));
  }, [applyEventUpdates]);

  const liveFlushDeferRemainingMs = useCallback(() => (
    Math.max(0, liveUpdatesDeferredUntilRef.current - performance.now())
  ), []);

  const schedulePendingLiveFlush = useCallback((delayMs: number) => {
    if (liveEventFlushTimerRef.current !== null) {
      window.clearTimeout(liveEventFlushTimerRef.current);
      liveEventFlushTimerRef.current = null;
    }

    const schedule = (nextDelayMs: number) => {
      liveEventFlushTimerRef.current = window.setTimeout(() => {
        liveEventFlushTimerRef.current = null;
        if (liveUpdatesPausedRef.current || pendingLiveEventsRef.current.size === 0) return;
        const deferRemainingMs = liveFlushDeferRemainingMs();
        if (deferRemainingMs > 0) {
          schedule(deferRemainingMs);
          return;
        }
        flushPendingLiveEvents();
      }, Math.max(0, Math.ceil(nextDelayMs)));
    };

    schedule(delayMs);
  }, [flushPendingLiveEvents, liveFlushDeferRemainingMs]);

  const flushOrSchedulePendingLiveEvents = useCallback((priority: "sync" | "transition" = "transition") => {
    if (pendingLiveEventsRef.current.size === 0 || liveUpdatesPausedRef.current) return;
    if (priority === "sync") {
      flushPendingLiveEvents("sync");
      return;
    }
    const deferRemainingMs = liveFlushDeferRemainingMs();
    if (deferRemainingMs > 0) {
      schedulePendingLiveFlush(deferRemainingMs);
      return;
    }
    flushPendingLiveEvents(priority);
  }, [flushPendingLiveEvents, liveFlushDeferRemainingMs, schedulePendingLiveFlush]);

  const deferPendingLiveFlush = useCallback((durationMs = LIVE_EVENT_INTERACTION_QUIET_MS) => {
    const nextDeferredUntil = performance.now() + durationMs;
    liveUpdatesDeferredUntilRef.current = Math.max(
      liveUpdatesDeferredUntilRef.current,
      nextDeferredUntil,
    );
    if (liveEventFlushTimerRef.current !== null) {
      window.clearTimeout(liveEventFlushTimerRef.current);
      liveEventFlushTimerRef.current = null;
    }
    flushOrSchedulePendingLiveEvents();
  }, [flushOrSchedulePendingLiveEvents]);

  const handleLiveStreamUserInteraction = useCallback(() => {
    deferPendingLiveFlush();
  }, [deferPendingLiveFlush]);

  const enqueueLiveEventUpdate = useCallback(
    (event: AcpUiEventVm) => {
      const decision = decideAcpLiveEventFlush({
        coalescable: isCoalescableAcpLiveEvent(event),
        paused: liveUpdatesPausedRef.current,
        deferRemainingMs: liveFlushDeferRemainingMs(),
        flushDelayMs: LIVE_EVENT_FLUSH_MS,
        hasScheduledFlush: liveEventFlushTimerRef.current !== null,
      });

      if (decision.applyImmediately) {
        const bufferedToolKey = liveToolEventBufferKey(event);
        const pendingToolEvent = bufferedToolKey
          ? pendingLiveEventsRef.current.get(bufferedToolKey)
          : null;
        const eventToApply = pendingToolEvent
          ? mergeAcpLiveToolEvent(pendingToolEvent, event, mergeRaw)
          : event;
        if (bufferedToolKey) pendingLiveEventsRef.current.delete(bufferedToolKey);
        if (decision.flushPendingBeforeApply) flushPendingLiveEvents("sync");
        applyEventUpdate(eventToApply);
        return;
      }

      if (!decision.buffer) return;
      const bufferKey = liveEventBufferKey(event);
      pendingLiveEventsRef.current.set(
        bufferKey,
        mergeBufferedLiveEvent(pendingLiveEventsRef.current.get(bufferKey), event),
      );
      if (decision.scheduleDelayMs !== null) {
        schedulePendingLiveFlush(decision.scheduleDelayMs);
      }
    },
    [applyEventUpdate, flushPendingLiveEvents, liveFlushDeferRemainingMs, schedulePendingLiveFlush],
  );

  useEffect(() => {
    if (liveUpdatesPaused) {
      if (liveEventFlushTimerRef.current !== null) {
        window.clearTimeout(liveEventFlushTimerRef.current);
        liveEventFlushTimerRef.current = null;
      }
      return;
    }
    flushOrSchedulePendingLiveEvents();
  }, [flushOrSchedulePendingLiveEvents, liveUpdatesPaused]);

  useEffect(() => {
    latestSessionRef.current = effective ?? currentSession ?? session ?? null;
  }, [currentSession, effective, session]);

  useEffect(() => {
    const scroller = scrollerElementRef.current;
    if (!scroller) return;
    const anchor = prependAnchorRef.current;
    if (anchor) {
      prependAnchorRef.current = null;
      const element = findAcpItemElement(scroller, anchor.key);
      if (element) {
        preservingScrollRef.current = true;
        const delta = element.getBoundingClientRect().top - anchor.top;
        requestAnimationFrame(() => {
          const el = scrollerElementRef.current;
          if (el) el.scrollTop += delta;
          preservingScrollRef.current = false;
        });
      }
      return;
    }
    if (shouldAutoScrollAfterAcpTimelineUpdate({
      pinned: pinToBottomRef.current,
      deferRemainingMs: liveFlushDeferRemainingMs(),
    })) {
      requestAnimationFrame(() => {
        const el = scrollerElementRef.current;
        if (el && pinToBottomRef.current) {
          programmaticScrollRef.current = true;
          el.scrollTop = el.scrollHeight;
          requestAnimationFrame(() => {
            programmaticScrollRef.current = false;
          });
        }
      });
    }
  }, [liveFlushDeferRemainingMs, timeline]);

  useEffect(() => {
    if (!isTauriRuntime()) {
      setLoadingInitialSession(false);
      return;
    }
    const runtimeApi = getRuntimeApi();
    if (!runtimeApi.subscribeAcpSessionUpdates) {
      setLoadingInitialSession(false);
      return;
    }
    let active = true;
    let stopListening: (() => void) | null = null;
    const subscribe = runtimeApi.subscribeAcpSessionUpdates;
    if (!subscribe) return;
    const refreshSeq = sessionRefreshSeqRef.current + 1;
    sessionRefreshSeqRef.current = refreshSeq;
    const matchesSessionEvent = (event: {
      taskId: string;
      runId: string;
      roundId: string;
      nodeId: string;
      attemptId: string;
      outerNodeId?: string | null;
      outerAttemptId?: string | null;
    }) =>
      event.taskId === taskId &&
      event.runId === runId &&
      event.roundId === roundId &&
      event.nodeId === nodeId &&
      event.attemptId === attemptId &&
      (event.outerNodeId ?? null) === (outerNodeId ?? null) &&
      (event.outerAttemptId ?? null) === (outerAttemptId ?? null);
    void (async () => {
      const sessionKey = `${taskId}/${runId}/${roundId}/${nodeId}/${attemptId}/${outerNodeId ?? ""}/${outerAttemptId ?? ""}`;
      stopListening = await subscribe((event) => {
        if (!active || !matchesSessionEvent(event)) return;
        if (event.event) {
          enqueueLiveEventUpdate(event.event);
          const latest = latestSessionRef.current;
          const missingMetadata = !hasAcpSessionMetadata(
            latest
              ? {
                  systemPromptAppend: latest.systemPromptAppend,
                  config: latest.config,
                }
              : null,
          );
          if (
            missingMetadata &&
            !hydratedSessionKeysRef.current.has(sessionKey) &&
            !hydrationInflightRef.current
          ) {
            hydrationInflightRef.current = true;
            getAcpSession(
              projectId,
              taskId,
              runId,
              roundId,
              nodeId,
              attemptId,
              {
                pageSize: effectiveEventPageSize,
                eventLimit: effectiveEventPageSize,
              },
              latestSessionRef.current,
              outerNodeId,
              outerAttemptId,
            )
              .then((updated) => {
                if (active && sessionRefreshSeqRef.current === refreshSeq) {
                  applySessionUpdate(updated);
                  if (updated && isAcpInitialSessionReady(updated)) {
                    hydratedSessionKeysRef.current.add(sessionKey);
                  }
                }
              })
              .catch(() => {})
              .finally(() => {
                hydrationInflightRef.current = false;
              });
          }
        } else {
          flushOrSchedulePendingLiveEvents("sync");
          // Guard against subscription refresh overwriting a pending user config change
          const incoming = event.session;
          if (incoming && configGenerationRef.current > 0 && latestSessionRef.current?.config) {
            const cfg = latestSessionRef.current.config;
            if (incoming.config) {
              incoming.config = { ...incoming.config, currentModelId: cfg.currentModelId, currentModelName: cfg.currentModelName, currentModeId: cfg.currentModeId, currentModeName: cfg.currentModeName };
            }
          }
          applySessionUpdate(incoming ?? null);
        }
      });
      let retryAttempt = 0;
      while (true) {
        try {
          const updated = await getAcpSession(
            projectId,
            taskId,
            runId,
            roundId,
            nodeId,
            attemptId,
            {
              pageSize: effectiveEventPageSize,
              eventLimit: effectiveEventPageSize,
            },
            latestSessionRef.current,
            outerNodeId,
            outerAttemptId,
          );
          if (updated && active && sessionRefreshSeqRef.current === refreshSeq) {
            applySessionUpdate(updated);
            if (isAcpInitialSessionReady(updated) || isSessionTerminalStatus(updated.status)) {
              break;
            }
          }
        } catch {
          // provider resolution / IO error — retry may not help but we try once more
        }
        const delay = missingAcpSessionRetryDelay(retryAttempt);
        if (delay === null) break;
        retryAttempt += 1;
        await new Promise((resolve) => setTimeout(resolve, delay));
      }
      if (active && sessionRefreshSeqRef.current === refreshSeq)
        setLoadingInitialSession(false);
    })();
    return () => {
      active = false;
      stopListening?.();
      if (liveEventFlushTimerRef.current !== null) {
        window.clearTimeout(liveEventFlushTimerRef.current);
        liveEventFlushTimerRef.current = null;
      }
      pendingLiveEventsRef.current.clear();
    };
  }, [
    applySessionUpdate,
    attemptId,
    enqueueLiveEventUpdate,
    eventWindowKey,
    effectiveEventPageSize,
    flushOrSchedulePendingLiveEvents,
    nodeId,
    outerAttemptId,
    outerNodeId,
    roundId,
    runId,
    taskId,
  ]);

  useEffect(() => {
    if (runtimeStopAccepted && !(runtimeComposerContext?.lifecycle?.runtime.active ?? isRuntimeActiveStatus(runtimeComposerContext?.runtimeStatus))) {
      setRuntimeStopAccepted(false);
    }
  }, [runtimeComposerContext?.lifecycle?.runtime.active, runtimeComposerContext?.runtimeStatus, runtimeStopAccepted]);

  useEffect(() => {
    const terminalSession = isSessionTerminalStatus(effective?.status);
    if (stopCommandPending || sending || waitingForOptimisticPrompt) {
      return;
    }
    if (!awaitingResponse && !cancelling) {
      return;
    }
    if (!terminalSession && cancelling && awaitTerminalStopRef.current && acpSessionActive) {
      return;
    }
    if (!terminalSession && cancelling && acpSessionActive) {
      return;
    }
    if (!terminalSession && !cancelling && sessionActive) {
      return;
    }
    setAwaitingResponse(false);
    setCancelling(false);
    awaitTerminalStopRef.current = false;
    const shouldNotifyStopped = cancelRequestedRef.current;
    cancelRequestedRef.current = false;
    if (shouldNotifyStopped) onSessionStopped?.();
  }, [
    acpSessionActive,
    awaitingResponse,
    cancelling,
    effective?.status,
    onSessionStopped,
    sending,
    sessionActive,
    stopCommandPending,
    waitingForOptimisticPrompt,
  ]);

  useEffect(() => {
    if (terminalSessionNotifiedRef.current) return;
    if (!isSessionCompletedStatus(effective?.status)) return;
    if (!runtimeActiveFromContext && !awaitingResponse && !cancelling) return;
    if (localSubmissionPending) return;
    terminalSessionNotifiedRef.current = true;
    onSessionStopped?.();
  }, [awaitingResponse, cancelling, effective?.status, localSubmissionPending, onSessionStopped, runtimeActiveFromContext]);

  useEffect(() => {
    const acceptedPrompt = findMatchingGoldBandUserPrompt(
      loadedEvents,
      activeTurnPrompt,
      activeTurnPromptId,
    );
    if (acceptedPrompt && !activeTurnStartedAt)
      setActiveTurnStartedAt(acceptedPrompt.timestamp);
    updateOptimisticEvents((current) => {
      const next = current.filter((event) =>
        shouldMergeOptimisticEvent(loadedEvents, event),
      );
      return next.length === current.length ? current : next;
    });
  }, [activeTurnPrompt, activeTurnPromptId, activeTurnStartedAt, loadedEvents]);

  const preserveScrollPosition = useCallback(() => {}, []);

  const loadOlderEvents = async () => {
    const previousEvents = loadedEventsRef.current;
    if (
      loadingOlderRef.current ||
      !hasOlderEvents ||
      previousEvents.length === 0
    )
      return;
    const oldestSeq = originalSeqFromAcpEvent(previousEvents[0]);
    const beforeCursor = formatTimelineCursor(oldestSeq);
    const scroller = scrollerElementRef.current;
    prependAnchorRef.current = scroller
      ? captureVisibleAcpAnchor(scroller)
      : null;
    loadingOlderRef.current = true;
    pinToBottomRef.current = false;
    setIsAtBottom(false);
    setLoadingOlder(true);
    try {
      const updated = normalizeSessionUpdate(
        await getAcpSession(
          projectId,
          taskId,
          runId,
          roundId,
          nodeId,
          attemptId,
          {
            beforeCursor,
            beforeSeq: oldestSeq,
            pageSize: effectiveEventPageSize,
            eventLimit: effectiveEventPageSize,
          },
          baseSession,
          outerNodeId,
          outerAttemptId,
        ),
      );
      if (!updated) {
        prependAnchorRef.current = null;
        return;
      }
      const merged = mergeAcpEvents(updated.events, previousEvents);
      const limited = limitAcpEvents(
        merged,
        "end",
        effectiveLoadedEventBufferLimit,
      );
      setCurrentSession(updated);
      setHasOlderEvents(updated.eventPage.hasOlder);
      setHasNewerEvents(
        updated.eventPage.hasNewer || limited.length < merged.length,
      );
      loadedEventsRef.current = limited;
      setLoadedEvents(limited);
    } finally {
      loadingOlderRef.current = false;
      setLoadingOlder(false);
    }
  };

  const loadNewerEvents = async () => {
    const previousEvents = loadedEventsRef.current;
    if (
      loadingNewerRef.current ||
      !hasNewerEvents ||
      previousEvents.length === 0
    )
      return;
    const newestSeq = originalSeqFromAcpEvent(
      previousEvents[previousEvents.length - 1],
    );
    const afterCursor = formatTimelineCursor(newestSeq);
    loadingNewerRef.current = true;
    try {
      const updated = normalizeSessionUpdate(
        await getAcpSession(
          projectId,
          taskId,
          runId,
          roundId,
          nodeId,
          attemptId,
          {
            afterCursor,
            afterSeq: newestSeq,
            pageSize: effectiveEventPageSize,
            eventLimit: effectiveEventPageSize,
          },
          baseSession,
          outerNodeId,
          outerAttemptId,
        ),
      );
      if (!updated) return;
      setCurrentSession(updated);
      setHasNewerEvents(updated.eventPage.hasNewer);
      setLoadedEvents((events) => {
        const merged = mergeAcpEvents(events, updated.events);
        const limited = limitAcpEvents(
          merged,
          "start",
          effectiveLoadedEventBufferLimit,
        );
        setHasOlderEvents(
          updated.eventPage.hasOlder || limited.length < merged.length,
        );
        loadedEventsRef.current = limited;
        return limited;
      });
    } finally {
      loadingNewerRef.current = false;
    }
  };

  const submitPrompt = async (trimmed: string) => {
    const submittingRuntimeContinue = composerState.submitTarget === "runtime-continue";
    if (sending || activeAwaitingResponse || (!submittingRuntimeContinue && sessionActive) || cancelling) return;
    setSending(true);
    setSendError(null);
    let attPaths: string[];
    try {
      attPaths = await resolveAttachmentPaths();
    } catch {
      setSending(false);
      return;
    }
    const optimisticEvent = optimisticUserEvent(trimmed);
    const promptId = promptIdFromEvent(optimisticEvent);
    const effectivePrompt = trimmed;
    setPrompt("");
    clearAttachments();
    setSendError(null);
    pinToBottomRef.current = true;
    setActiveTurnPrompt(effectivePrompt);
    setActiveTurnPromptId(promptId);
    setActiveTurnStartedAt(null);
    setAwaitingResponse(true);
    updateOptimisticEvents((current) => [...current, optimisticEvent]);
    try {
      const result = await submitConversationPrompt(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        effectivePrompt,
        promptId,
        effective ?? null,
        outerNodeId,
        outerAttemptId,
        attPaths.length > 0 ? attPaths : undefined,
      );
      const updated = result.session ?? null;
      if (updated) applySessionUpdate(updated);
      if (result.lifecycle) {
        setLocalRuntimeLifecycle(result.lifecycle);
        emitLifecycleSnapshot(result.lifecycle, result.session ?? null);
      }
      if (result.kind === "runtime-continue-started") {
        setAwaitingResponse(false);
        setActiveTurnStartedAt(optimisticEvent.timestamp);
        updateOptimisticEvents((current) =>
          current.map((event) =>
            event.id === optimisticEvent.id
              ? { ...event, status: "completed" }
              : event,
          ),
        );
        onSessionStopped?.();
      } else if (result.kind === "rejected") {
        setSendError(t("errors.app.unexpected", { message: "" }));
        setAwaitingResponse(false);
        setActiveTurnPrompt(null);
        setActiveTurnPromptId(null);
        setActiveTurnStartedAt(null);
        updateOptimisticEvents((current) =>
          current.map((event) =>
            event.id === optimisticEvent.id
              ? { ...event, status: "failed" }
              : event,
          ),
        );
      } else if (updated) {
        const acceptedEvents = mergeAcpEvents(loadedEventsRef.current, updated.events);
        const acceptedPrompt = findMatchingGoldBandUserPrompt(
          acceptedEvents,
          effectivePrompt,
          promptId,
          optimisticEvent.timestamp,
        );
        if (acceptedPrompt) {
          setActiveTurnStartedAt(acceptedPrompt.timestamp);
          if (isSessionTerminalStatus(updated.status)) setAwaitingResponse(false);
          updateOptimisticEvents((current) =>
            current.filter(
              (event) => !hasMatchingUserPrompt(acceptedEvents, event),
            ),
          );
        } else {
          setAwaitingResponse(false);
          setActiveTurnPrompt(null);
          setActiveTurnPromptId(null);
          setActiveTurnStartedAt(null);
          updateOptimisticEvents((current) =>
            current.map((event) =>
              event.id === optimisticEvent.id
                ? { ...event, status: "failed" }
                : event,
            ),
          );
        }
      } else {
        setAwaitingResponse(false);
        setActiveTurnPrompt(null);
        setActiveTurnPromptId(null);
        setActiveTurnStartedAt(null);
        updateOptimisticEvents((current) =>
          current.map((event) =>
            event.id === optimisticEvent.id
              ? { ...event, status: "failed" }
              : event,
          ),
        );
      }
    } catch (error) {
      if (cancelRequestedRef.current) {
        setAwaitingResponse(true);
        setActiveTurnPrompt(null);
        setActiveTurnPromptId(null);
        setActiveTurnStartedAt(null);
        updateOptimisticEvents((current) =>
          current.filter((event) => event.id !== optimisticEvent.id),
        );
        return;
      }
      setSendError(displayAppError(t, error));
      setAwaitingResponse(false);
      setActiveTurnPrompt(null);
      setActiveTurnPromptId(null);
      setActiveTurnStartedAt(null);
      updateOptimisticEvents((current) =>
        current.map((event) =>
          event.id === optimisticEvent.id
            ? { ...event, status: "failed" }
            : event,
        ),
      );
    } finally {
      setSending(false);
    }
  };

  const send = async () => {
    const trimmed = prompt.trim();
    if (!trimmed) return;
    if (pendingPermission && planInterventionOption) {
      setPrompt("");
      setQueuedInterventionPrompt(trimmed);
      setAwaitingResponse(true);
      await answerPermission(
        pendingPermission,
        planInterventionOption.optionId,
      );
      return;
    }
    if (composerState.submitTarget !== "none") {
      await submitPrompt(trimmed);
    }
  };

  const stopSession = async () => {
    if (!canStopSession || stopInProgress) return;
    cancelRequestedRef.current = true;
    setCancelling(true);
    setStopCommandPending(true);
    setStopOverlayPending(true);
    setCancelError(null);
    setAwaitingResponse(true);
    try {
      const result = await stopActiveSession(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        effective ?? null,
        outerNodeId,
        outerAttemptId,
      );
      awaitTerminalStopRef.current = Boolean(result.session?.sessionId);
      setRuntimeStopAccepted(Boolean(result.run));
      if (result.lifecycle) {
        setLocalRuntimeLifecycle(result.lifecycle);
        emitLifecycleSnapshot(result.lifecycle, result.session ?? null);
      }
      applySessionUpdate(result.session ?? null);
      flushPendingLiveEvents("sync");
      const finalSession = await getAcpSession(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        {
          pageSize: effectiveEventPageSize,
          eventLimit: effectiveEventPageSize,
        },
        result.session ?? effective ?? null,
        outerNodeId,
        outerAttemptId,
      );
      applySessionUpdate(finalSession);
      setStopCommandPending(false);
      setSending(false);
      setActiveTurnPrompt(null);
      setActiveTurnPromptId(null);
      setActiveTurnStartedAt(null);
    } catch (error) {
      setCancelError(displayAppError(t, error));
      setCancelling(false);
      setStopCommandPending(false);
      cancelRequestedRef.current = false;
    } finally {
      setStopOverlayPending(false);
    }
  };

  const submitManualDecision = async (outcome: "success" | "failure") => {
    if (!showManualCheckActions || manualCheckSubmitting) return;
    setManualCheckError(null);
    setManualCheckSubmitting(true);
    try {
      await submitManualCheck(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        outcome,
      );
      setManualCheckResolved(true);
      onManualCheckSubmitted?.();
    } catch (error) {
      setManualCheckError(displayAppError(t, error));
    } finally {
      setManualCheckSubmitting(false);
    }
  };

  const answerPermission = async (
    request: AcpPermissionRequestVm,
    optionId: string,
  ) => {
    setPermissionError(null);
    setDismissedPermissionIds((current) =>
      new Set(current).add(request.requestId),
    );
    try {
      const updated = await respondAcpPermission(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        request.requestId,
        optionId,
        effective,
        outerNodeId,
        outerAttemptId,
      );
      applySessionUpdate(updated);
    } catch (error) {
      setDismissedPermissionIds((current) => {
        const next = new Set(current);
        next.delete(request.requestId);
        return next;
      });
      setQueuedInterventionPrompt(null);
      setPermissionError(displayAppError(t, error));
    }
  };

  const answerElicitation = async (
    elicitationId: string,
    content?: Record<string, unknown>,
  ) => {
    setAnsweredElicitations((current) => {
      const next = new Map(current);
      next.set(elicitationId, content ?? {});
      return next;
    });
    try {
      await respondElicitation(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        elicitationId,
        "accept",
        content ?? null,
        outerNodeId,
        outerAttemptId,
      );
    } catch {
      setAnsweredElicitations((current) => {
        const next = new Map(current);
        next.delete(elicitationId);
        return next;
      });
    }
  };

  useEffect(() => {
    if (
      !queuedInterventionPrompt ||
      sending ||
      pendingPermission ||
      sessionActive ||
      activeAwaitingResponse ||
      cancelling
    )
      return;
    const queued = queuedInterventionPrompt;
    setQueuedInterventionPrompt(null);
    void submitPrompt(queued);
  }, [
    activeAwaitingResponse,
    cancelling,
    pendingPermission,
    queuedInterventionPrompt,
    sending,
    sessionActive,
  ]);

  const loadRawFrames = async (query: AcpRawFrameQueryInput) => {
    setRawLoading(true);
    try {
      const next = await getAcpRawFrames(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        query,
        outerNodeId,
        outerAttemptId,
      );
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
    if (canvasMode === "raw") {
      setCanvasMode("chat");
      return;
    }
    if (rawPage == null) await loadRawFrames(rawQuery);
    setCanvasMode("raw");
  };

  const scrollFrameRef = useRef<number | null>(null);

  const handleScrollRef = useRef<(() => void) | null>(null);
  handleScrollRef.current = () => {
    if (preservingScrollRef.current) return;
    const scroller = scrollerElementRef.current;
    if (!scroller) return;
    if (!programmaticScrollRef.current) handleLiveStreamUserInteraction();
    const scrollTop = scroller.scrollTop;
    if (scrollTop < HISTORY_LOAD_THRESHOLD_PX) void loadOlderEvents();
    const distanceFromBottom =
      scroller.scrollHeight - scrollTop - scroller.clientHeight;
    if (!programmaticScrollRef.current && distanceFromBottom > BOTTOM_STICK_THRESHOLD_PX) {
      pinToBottomRef.current = false;
    }
    if (distanceFromBottom < BOTTOM_STICK_THRESHOLD_PX) {
      pinToBottomRef.current = true;
    }
    const atBottom =
      distanceFromBottom < BOTTOM_STICK_THRESHOLD_PX;
    setIsAtBottom((current) => (current === atBottom ? current : atBottom));
    if (atBottom && hasNewerEvents) void loadNewerEvents();
  };
  const handleScroll = useCallback(() => {
    if (scrollFrameRef.current != null) return;
    scrollFrameRef.current = requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      handleScrollRef.current?.();
    });
  }, []);

  const sessionShellState = resolveAcpSessionShellState({
    hasBaseSession: Boolean(baseSession),
    hasLiveSessionShell: Boolean(liveSessionShell),
    initialSessionLoading: loadingInitialSession,
  });

  if (sessionShellState === 'loading') {
    return <AcpLoadingState label={t("common.loading")} />;
  }

  if (!effective) {
    if (runtimeActiveFromContext) {
      return <AcpLoadingState label={t("conversation.runtime.runtimeActive")} />;
    }
    return <AcpErrorState reason={t("acp.missingSessionReason")} />;
  }

  const visibleError = visibleSessionError(effective, effectiveEvents);

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-background">
      <ACPSessionHeader
        session={effective}
        rawActive={canvasMode === "raw"}
        rawLoading={rawLoading}
        systemPromptAvailable={
          Boolean(effective.systemPromptAppend?.trim()) ||
          Boolean(systemPromptOptions?.some((option) => option.prompt?.trim()))
        }
        onToggleRaw={toggleRawFrames}
        onOpenSystemPrompt={() => setSystemPromptOpen(true)}
      />
      <SystemPromptDialog
        open={systemPromptOpen}
        prompt={effective.systemPromptAppend}
        options={systemPromptOptions}
        onOpenChange={setSystemPromptOpen}
      />
      <ACPArtifactsDialog
        open={artifactsDialogOpen}
        artifacts={artifacts}
        attachments={attachments}
        selectedArtifact={selectedArtifact}
        artifactContent={artifactContent}
        artifactLoading={artifactLoading}
        onOpenChange={handleArtifactsDialogOpenChange}
        onOpenDetail={handleOpenArtifactDetail}
        onBack={() => {
          setSelectedArtifact(null);
          setArtifactContent(null);
        }}
      />
      <MessageImagePreviewDialog
        preview={messageImagePreview}
        onClose={() => setMessageImagePreview(null)}
      />
      {visibleError ? <AcpErrorBanner reason={visibleError} /> : null}
      <div className="relative min-h-0 min-w-0 max-w-full flex-1 overflow-hidden">
        {canvasMode === "raw" ? (
          <div className={ACP_RAW_SCROLL_AREA_CLASS_NAME}>
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
              className={ACP_SESSION_SCROLL_AREA_CLASS_NAME}
              onScroll={handleScroll}
              onWheel={handleLiveStreamUserInteraction}
            >
              {loadingOlder ? (
                <AcpListLoading label={t("acp.loadingOlderEvents")} />
              ) : hasOlderEvents ? (
                <AcpHistoryHint label={t("acp.scrollForHistory")} />
              ) : (
                <div className="h-3" />
              )}
              {timeline.length === 0 &&
              !isSessionActiveStatus(effective.status) &&
              !sending ? (
                <div className="p-5">
                  <EmptyAcpState />
                </div>
              ) : timeline.length === 0 ? (
                <div className="p-5">
                  <AcpPendingTimelineState label={composerStatusLabel} />
                </div>
              ) : (
                <div className="space-y-3 px-5 py-3">
                  {timeline.map((item) => (
                    <div
                      key={timelineEventKey(item)}
                      data-acp-item-key={timelineEventKey(item)}
                    >
                      <ACPTimelineItemRenderer
                        event={item}
                        expansionControls={expansionControls}
                        streamingTextItemKey={streamingTextItemKey}
                        projectId={projectId}
                        taskId={taskId}
                        onMessageAttachmentClick={handleOpenMessageAttachment}
                      />
                    </div>
                  ))}
                </div>
              )}
              <div className="space-y-4 px-5 pb-5">
                {sendError ? (
                  <AcpErrorBanner
                    reason={`${t("acp.sendFailed")}：${sendError}`}
                  />
                ) : null}
                {cancelError ? (
                  <AcpErrorBanner
                    reason={`${t("acp.stopFailed")}：${cancelError}`}
                  />
                ) : null}
                {manualCheckError ? (
                  <AcpErrorBanner
                    reason={`${t("acp.manualCheckSubmitFailed")}：${manualCheckError}`}
                  />
                ) : null}
                {permissionError ? (
                  <AcpErrorBanner reason={permissionError} />
                ) : null}
                {pendingPermission ? (
                  <PermissionRequestCard
                    request={pendingPermission}
                    onSelect={(optionId) =>
                      answerPermission(pendingPermission, optionId)
                    }
                  />
                ) : null}
                {pendingElicitation ? (
                  <ElicitationCard
                    key={pendingElicitation.elicitationId}
                    elicitationId={pendingElicitation.elicitationId}
                    message={pendingElicitation.message}
                    schema={pendingElicitation.requestedSchema}
                    confirmedContent={pendingElicitation.confirmedContent}
                    onRespond={(content) =>
                      answerElicitation(
                        pendingElicitation.elicitationId,
                        content,
                      )
                    }
                  />
                ) : null}
              </div>
            </div>
          </div>
        )}
        {stopOverlayPending ? <AcpStopOverlay /> : null}
      </div>
      {canvasMode === "chat" ? (
        <div className="shrink-0 bg-background/95 backdrop-blur">
          <AcpSessionAssetsPanel
            artifacts={allArtifacts ?? artifacts}
            attachments={allAttachments ?? attachments}
            onOpenDetail={handleOpenArtifactDetail}
          />
          {todoEntries.length > 0 ? (
            <div className="px-4">
              <AcpTodoPanel entries={todoEntries} />
            </div>
          ) : null}
          <div className="border-t px-4 pt-1.5 pb-1.5">
            <AcpUsagePanel
              usage={effective?.usage}
              isRunning={sessionActive || composerStatusActive}
              compact={usageCompact}
              processingLabel={
                usageCompact && composerStatusActive ? composerStatusLabel : null
              }
              stepSeconds={
                usageCompact
                  ? composerStatusActive
                    ? usageStepSeconds
                    : null
                  : null
              }
              sessionSeconds={usageCompact ? composerSessionSeconds : null}
              className="mb-1"
            />
            {showManualCheckActions ? (
              <AcpManualCheckPanel
                submitting={manualCheckSubmitting}
                onSuccess={() => void submitManualDecision("success")}
                onFailure={() => void submitManualDecision("failure")}
              />
            ) : null}
            {composerState.externalKind ? (
              <AcpExternalComposerState
                kind={composerState.externalKind}
                message={composerState.externalMessage ?? ""}
                onAction={
                  composerState.externalKind === "invalid-workflow"
                    ? runtimeComposerContext?.onRepair
                    : undefined
                }
              />
            ) : (
              <>
                {composerLocked ? (
                  <AcpPermissionComposerLock />
                ) : (
                  <div
                    data-attachment-dropzone="true"
                    onDragEnter={dropZoneHandlers.onDragEnter}
                    onDragOver={dropZoneHandlers.onDragOver}
                    onDrop={dropZoneHandlers.onDrop}
                  >
                    {/* Attachment chips */}
                    <div className="mb-2">
                      <AttachmentChipsList
                        attachments={pendingAttachments}
                        compact
                        onRemove={removeAttachment}
                        onPreview={handlePreviewAttachment}
                        onClear={clearAttachments}
                        clearLabel={t("common.clear") ?? "Clear"}
                      />
                    </div>
                    {/* File error */}
                    {fileError ? (
                      <div className="mb-2 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                        {fileError}
                      </div>
                    ) : null}
                    <PromptInput
                      value={prompt}
                      onValueChange={setPrompt}
                      onSubmit={send}
                      isLoading={sending}
                      className="rounded-2xl bg-card/80 shadow-sm shadow-background/30 transition-colors focus-within:border-primary/40 focus-within:ring-2 focus-within:ring-primary/10"
                    >
                      {showComposerStatus && !usageCompact ? (
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
                        textareaDisabled={composerInputDisabled}
                        onDragEnter={dropZoneHandlers.onDragEnter}
                        onDragOver={dropZoneHandlers.onDragOver}
                        onDrop={dropZoneHandlers.onDrop}
                        onPaste={extractPasteFiles}
                      />
                      <div className="mt-1.5 flex items-center justify-between gap-4 px-2 pb-1">
                        <div className="flex items-center gap-2">
                          <input
                            ref={fileInputRef}
                            type="file"
                            multiple
                            className="hidden"
                            onChange={handleFilesFromInput}
                          />
                          <PromptInputAction
                            tooltip={t("acp.attachHint") ?? "Attach files"}
                          >
                            <Button
                              className="size-7 rounded-full"
                              size="icon"
                              variant="ghost"
                              disabled={composerInputDisabled}
                              onClick={() => { void pickFiles(); }}
                            >
                              <Paperclip className="size-3.5" />
                            </Button>
                          </PromptInputAction>
                          <span className="text-xs text-muted-foreground">
                            {composerInputHint}
                          </span>
                        </div>
                        <PromptInputActions className="shrink-0 pl-2">
                          {canStopSession ? (
                            <PromptInputAction tooltip={t("acp.stopHint")}>
                              <Button
                                className="h-8 gap-1.5 rounded-full px-3"
                                size="sm"
                                variant="secondary"
                                disabled={stopInProgress}
                                onClick={() => { void stopSession(); }}
                              >
                                {stopInProgress ? (
                                  <Loader2
                                    className="size-3.5 animate-spin"
                                    style={{ willChange: "transform" }}
                                  />
                                ) : (
                                  <CircleStop className="size-3.5" />
                                )}
                                {stopInProgress ? t("acp.stopping") : t("acp.stop")}
                              </Button>
                            </PromptInputAction>
                          ) : null}
                          <PromptInputAction tooltip={t("acp.send")}>
                            <Button
                              className="h-8 gap-1.5 rounded-full px-3"
                              size="sm"
                              disabled={!canSubmitPrompt}
                              onClick={send}
                            >
                              {sendButtonBusy ? (
                                <Loader2
                                  className="size-3.5 animate-spin"
                                  style={{ willChange: "transform" }}
                                />
                              ) : (
                                <Send className="size-3.5" />
                              )}
                              {t("acp.send")}
                            </Button>
                          </PromptInputAction>
                        </PromptInputActions>
                      </div>
                      <AcpSessionConfigBar
                        scopeKey={sessionIdentity}
                        viewModel={sessionConfigViewModel}
                        onModelChange={handleAcpSessionModelChange}
                        onPermissionModeChange={handleAcpSessionPermissionModeChange}
                      />
                    </PromptInput>
                  </div>
                )}
              </>
            )}
          </div>
        </div>
      ) : null}
      <AttachmentPreviewDialogs
        previewImage={previewImage}
        textPreview={textPreview}
        onCloseImage={() => setPreviewImage(null)}
        onCloseText={() => setTextPreview(null)}
      />
    </div>
  );
});

function AcpErrorState({ reason }: { reason: string }) {
  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <AcpErrorBanner reason={reason} />
      <div className="flex-1" />
    </div>
  );
}

function AcpLoadingState({ label }: { label: string }) {
  return (
    <div className="flex h-full min-h-0 items-center justify-center bg-background text-sm text-muted-foreground">
      <div className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className="size-3.5 shrink-0 animate-spin rounded-full border-2 border-primary/25 border-t-primary [animation-duration:900ms]"
        />
        <span>{label}</span>
      </div>
    </div>
  );
}

function AcpListLoading({ label }: { label: string }) {
  return (
    <div className="mx-auto my-3 flex w-fit items-center gap-2 rounded-full border bg-card/80 px-3 py-1.5 text-xs text-muted-foreground">
      <Loader2 className="size-3 animate-spin" />
      {label}
    </div>
  );
}

function AcpHistoryHint({ label }: { label: string }) {
  return (
    <div className="mx-auto my-3 w-fit select-none rounded-full border border-dashed bg-muted/20 px-3 py-1 text-xs text-muted-foreground">
      {label}
    </div>
  );
}

function captureVisibleAcpAnchor(scroller: HTMLElement) {
  const scrollerTop = scroller.getBoundingClientRect().top;
  const items = Array.from(
    scroller.querySelectorAll<HTMLElement>("[data-acp-item-key]"),
  );
  const item =
    items.find(
      (element) => element.getBoundingClientRect().bottom > scrollerTop,
    ) ?? items[0];
  const key = item?.dataset.acpItemKey;
  return item && key ? { key, top: item.getBoundingClientRect().top } : null;
}

function findAcpItemElement(scroller: HTMLElement, key: string) {
  return (
    Array.from(
      scroller.querySelectorAll<HTMLElement>("[data-acp-item-key]"),
    ).find((element) => element.dataset.acpItemKey === key) ?? null
  );
}

function AcpPermissionComposerLock() {
  const { t } = useTranslation();
  return (
    <div className="flex min-w-0 items-center gap-2 rounded-2xl border border-primary/15 bg-card/60 px-3 py-2 text-sm text-muted-foreground shadow-sm shadow-background/20">
      <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
        <ShieldQuestion className="size-4" />
      </span>
      <span className="min-w-0 truncate font-medium">
        {t("acp.permissionPending")}
      </span>
    </div>
  );
}

function AcpExternalComposerState({
  kind,
  message,
  onAction,
}: {
  kind: "invalid-workflow" | "runtime-error";
  message: string;
  onAction?: () => void;
}) {
  const { t } = useTranslation();
  const isError = kind === "runtime-error";
  return (
    <div
      className={cn(
        "flex min-w-0 items-center gap-3 rounded-2xl border px-5 py-4 shadow-sm shadow-background/20",
        isError
          ? "border-destructive/20 bg-destructive/5"
          : "border-amber-500/20 bg-amber-500/5",
      )}
    >
      <span
        className={cn(
          "flex size-9 shrink-0 items-center justify-center rounded-lg",
          isError
            ? "bg-destructive/10 text-destructive"
            : "bg-amber-500/10 text-amber-500",
        )}
      >
        {isError ? (
          <CircleStop className="size-4" />
        ) : (
          <ShieldQuestion className="size-4" />
        )}
      </span>
      <span className="min-w-0 flex-1 text-sm font-medium text-foreground">
        {message}
      </span>
      {onAction ? (
        <Button
          size="default"
          className="h-9 shrink-0 rounded-full px-4 text-sm"
          onClick={onAction}
        >
          {isError
            ? t("conversation.runtime.repairAction")
            : t("conversation.runtime.repairWorkflow")}
        </Button>
      ) : null}
    </div>
  );
}

function AcpManualCheckPanel({
  submitting,
  onSuccess,
  onFailure,
}: {
  submitting: boolean;
  onSuccess: () => void;
  onFailure: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="mb-3 flex min-w-0 items-center gap-3 rounded-2xl border border-primary/20 bg-card/60 px-4 py-2.5 shadow-sm shadow-background/20">
      <div className="min-w-0 flex-1">
        <span className="text-sm font-semibold text-foreground">
          {t("acp.manualCheckPending")}
        </span>
        <span className="ml-2 text-xs text-muted-foreground">
          {t("acp.manualCheckDescription")}
        </span>
      </div>
      <div className="flex shrink-0 gap-2">
        <Button
          className="h-8 rounded-full px-3"
          size="sm"
          disabled={submitting}
          onClick={onSuccess}
        >
          {submitting ? <Loader2 className="size-3.5 animate-spin" /> : null}
          {submitting
            ? t("acp.manualCheckSubmitting")
            : t("acp.manualCheckSuccess")}
        </Button>
        <Button
          className="h-8 rounded-full px-3"
          size="sm"
          variant="outline"
          disabled={submitting}
          onClick={onFailure}
        >
          {t("acp.manualCheckFailure")}
        </Button>
      </div>
    </div>
  );
}

function AcpTodoPanel({
  entries,
}: {
  entries: Array<{ content?: string; status?: string; priority?: string }>;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  if (entries.length === 0) return null;

  const completedCount = entries.filter(
    (e) => e.status === "completed" || e.status === "complete",
  ).length;
  const inProgressEntry = entries.find(
    (e) => e.status === "in_progress" || e.status === "running",
  );
  const summary = inProgressEntry
    ? `${completedCount}/${entries.length} · ${inProgressEntry.content}`
    : `${completedCount}/${entries.length}`;

  return (
    <Collapsible
      open={open}
      onOpenChange={setOpen}
      className="w-full border border-b-0 border-border/60 bg-card/60"
    >
      <CollapsibleTrigger asChild>
        <Button
          variant="ghost"
          className="h-auto w-full justify-between rounded-none border-0 px-3 py-2 font-normal shadow-none hover:bg-transparent focus-visible:border-transparent focus-visible:ring-0"
        >
          <span className="flex min-w-0 items-center gap-2 text-xs">
            <ListTodo className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="text-muted-foreground">{t("acp.todo")}</span>
            <span className="truncate font-medium text-foreground">
              {summary}
            </span>
          </span>
          <ChevronDown
            className={cn(
              "size-3.5 shrink-0 text-muted-foreground transition-transform",
              open && "rotate-180",
            )}
          />
        </Button>
      </CollapsibleTrigger>
      <CollapsibleContent className="data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down overflow-hidden">
        <div className="space-y-1 border-t border-border/40 px-3 pb-2 pt-2">
          {entries.map((entry, index) => (
            <div className="flex min-w-0 items-start gap-2 text-xs" key={index}>
              <Badge variant="secondary" className="shrink-0">
                {entry.status
                  ? displayStatus(t, entry.status)
                  : (entry.priority ?? index + 1)}
              </Badge>
              <span className="min-w-0 break-words [overflow-wrap:anywhere]">
                {entry.content}
              </span>
            </div>
          ))}
        </div>
      </CollapsibleContent>
    </Collapsible>
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

function AcpStopOverlay() {
  const { t } = useTranslation();
  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-background/70 backdrop-blur-sm">
      <div className="flex items-center gap-2 rounded-full border bg-card/90 px-4 py-2 text-sm font-medium text-foreground shadow-lg shadow-background/30">
        <Loader2 className="size-4 animate-spin text-primary" />
        {t("acp.stopping")}
      </div>
    </div>
  );
}

function AcpErrorBanner({ reason }: { reason: string }) {
  const { t } = useTranslation();
  return (
    <div className="shrink-0 border-b border-destructive/20 bg-destructive/5 px-5 py-3 text-sm">
      <span className="font-semibold text-destructive">
        {t("acp.sessionFailed")}
      </span>
      <span className="ml-2 text-muted-foreground">{reason}</span>
    </div>
  );
}

type AcpSessionConfigBarProps = {
  scopeKey: string;
  viewModel: AcpSessionConfigViewModel;
  onModelChange?: (modelId: string) => void;
  onPermissionModeChange?: (permissionModeId: string) => void;
};

const AcpSessionConfigBar = memo(function AcpSessionConfigBar({
  viewModel,
  onModelChange,
  onPermissionModeChange,
}: AcpSessionConfigBarProps) {
  const { t } = useTranslation();
  const {
    currentModelId,
    currentModelName,
    currentModeId,
    currentModeName,
    modeLabel,
    availableModels,
    availablePermissionModes,
  } = viewModel;

  const handleModelSelect = useCallback(
    (modelId: string) => {
      onModelChange?.(modelId);
    },
    [onModelChange],
  );

  const handlePermissionModeSelect = useCallback(
    (permissionModeId: string) => {
      onPermissionModeChange?.(permissionModeId);
    },
    [onPermissionModeChange],
  );

  const modelLabel = currentModelName ?? currentModelId ?? t('conversation.home.selectModel');
  const permissionModeLabel = modeLabel ?? currentModeId ?? t('acp.permissionMode');
  const showModels = availableModels.length > 0 || Boolean(currentModelId);
  const showPermissionModes = availablePermissionModes.length > 0 || Boolean(modeLabel);

  if (!showModels && !showPermissionModes) return null;

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5 border-t border-border/50 px-2 py-1.5 text-xs text-muted-foreground">
      {availableModels.length > 1 ? (
        <Select value={currentModelId ?? ''} onValueChange={handleModelSelect}>
          <SelectTrigger className="h-7 min-w-0 max-w-[min(22rem,100%)] gap-1.5 rounded-full border-border/60 bg-background/50 px-2.5 text-xs font-normal text-foreground shadow-none hover:bg-background/70 focus-visible:border-primary/30 focus-visible:ring-2 focus-visible:ring-primary/10">
            <span className="shrink-0 text-muted-foreground">
              {t('acp.currentModel')}
            </span>
            <span className="min-w-0 flex-1 truncate text-left">{modelLabel}</span>
          </SelectTrigger>
          <SelectContent
            side="top"
            sideOffset={8}
            position="popper"
            align="start"
            className="w-[min(22rem,calc(100vw-2rem))] max-w-[calc(100vw-2rem)]"
          >
            {availableModels.map((m) => (
              <SelectItem value={m.id} key={m.id} className="items-start py-2">
                <span className="block min-w-0">
                  <span className="block truncate font-medium">{m.name}</span>
                  {m.description ? (
                    <span className="mt-0.5 block whitespace-normal break-words text-[11px] leading-4 text-muted-foreground">{m.description}</span>
                  ) : null}
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : showModels ? (
        <Badge variant="outline" className="max-w-full gap-1.5 rounded-full bg-background/50 px-2 py-0.5 font-normal">
          <span className="shrink-0 text-muted-foreground">{t('acp.currentModel')}</span>
          <span className="min-w-0 truncate text-foreground">{modelLabel}</span>
        </Badge>
      ) : null}
      {showPermissionModes ? (
        availablePermissionModes.length > 1 ? (
          <Select value={currentModeId ?? ''} onValueChange={handlePermissionModeSelect}>
            <SelectTrigger className="h-7 min-w-0 max-w-[min(18rem,100%)] gap-1.5 rounded-full border-border/60 bg-background/50 px-2.5 text-xs font-normal text-foreground shadow-none hover:bg-background/70 focus-visible:border-primary/30 focus-visible:ring-2 focus-visible:ring-primary/10">
              <span className="shrink-0 text-muted-foreground">
                {t('acp.permissionMode')}
              </span>
              <span className="min-w-0 flex-1 truncate text-left">{permissionModeLabel}</span>
            </SelectTrigger>
            <SelectContent
              side="top"
              sideOffset={8}
              position="popper"
              align="start"
              className="w-[min(22rem,calc(100vw-2rem))] max-w-[calc(100vw-2rem)]"
            >
              {availablePermissionModes.map((m) => (
                <SelectItem value={m.id} key={m.id} className="items-start py-2">
                  <span className="block min-w-0">
                    <span className="block truncate font-medium">{m.name}</span>
                    {m.description ? (
                      <span className="mt-0.5 block whitespace-normal break-words text-[11px] leading-4 text-muted-foreground">{m.description}</span>
                    ) : null}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <Badge variant="outline" className="max-w-full gap-1.5 rounded-full bg-background/50 px-2 py-0.5 font-normal">
            <span className="shrink-0 text-muted-foreground">{t('acp.permissionMode')}</span>
            <span className="min-w-0 truncate text-foreground">{permissionModeLabel}</span>
          </Badge>
        )
      ) : null}
    </div>
  );
}, areAcpSessionConfigBarPropsEqual);

function areAcpSessionConfigBarPropsEqual(
  previous: AcpSessionConfigBarProps,
  next: AcpSessionConfigBarProps,
) {
  return (
    previous.scopeKey === next.scopeKey &&
    previous.viewModel.signature === next.viewModel.signature &&
    previous.onModelChange === next.onModelChange &&
    previous.onPermissionModeChange === next.onPermissionModeChange
  );
}

export function ACPSessionHeader({
  session,
  rawActive,
  rawLoading,
  systemPromptAvailable,
  onToggleRaw,
  onOpenSystemPrompt,
}: {
  session: AcpSessionVm;
  rawActive: boolean;
  rawLoading: boolean;
  systemPromptAvailable?: boolean;
  onToggleRaw: () => void;
  onOpenSystemPrompt: () => void;
}) {
  const { t } = useTranslation();
  const mode = session.config?.currentModeName ?? session.config?.currentModeId;
  const hasSystemPrompt =
    systemPromptAvailable ?? Boolean(session.systemPromptAppend?.trim());
  return (
    <div className="shrink-0 border-b border-border/60 bg-gold-surface-high/60 px-5 pb-1 pt-0 shadow-[inset_0_-1px_0_color-mix(in_srgb,var(--gold-line-soft)_56%,transparent)]">
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="min-w-0 truncate text-[13px] font-medium leading-5 text-foreground/88">
          {session.adapterDisplayName ?? session.provider}
        </span>
        {mode ? (
          <Badge
            variant="outline"
            className="max-w-full gap-1 rounded-full border-border/60 bg-background/30 px-1.5 py-0 text-[10px] font-normal text-foreground/78"
          >
            <span className="shrink-0 text-muted-foreground">
              {t("acp.permissionMode")}
            </span>
            <span className="min-w-0 truncate text-foreground">{mode}</span>
          </Badge>
        ) : null}
        <span className="truncate text-[10px] text-muted-foreground/82">
          {session.sessionId ?? t("acp.noSessionId")}
        </span>
        <div className="ml-auto flex shrink-0 items-center gap-1.5">
          <Button
            size="sm"
            variant="outline"
            className="h-5.5 gap-1 border-border/60 bg-background/22 px-2 text-[10px] font-normal text-foreground/80 hover:bg-background/38"
            onClick={onOpenSystemPrompt}
            disabled={!hasSystemPrompt}
          >
            <FileText className="size-3" />
            {t("acp.systemPrompt")}
          </Button>
          <Button
            size="sm"
            variant={rawActive ? "default" : "outline"}
            className={cn(
              "h-5.5 gap-1 px-2 text-[10px] font-normal",
              rawActive
                ? "bg-primary/18 text-foreground hover:bg-primary/24"
                : "border-border/60 bg-background/22 text-foreground/80 hover:bg-background/38",
            )}
            onClick={onToggleRaw}
            disabled={rawLoading}
          >
            {rawLoading ? <Loader2 className="size-3 animate-spin" /> : null}
            {t("acp.rawFrames")}
          </Button>
        </div>
      </div>
    </div>
  );
}

const SystemPromptDialog = memo(function SystemPromptDialog({
  open,
  prompt,
  options,
  onOpenChange,
}: {
  open: boolean;
  prompt?: string | null;
  options?: Array<{ attemptId: string; prompt?: string | null }>;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation();
  const availableOptions = useMemo(
    () => (open ? (options?.filter((option) => option.prompt?.trim()) ?? []) : []),
    [open, options],
  );
  const latestAttemptId = availableOptions.at(-1)?.attemptId ?? null;
  const [selectedAttemptId, setSelectedAttemptId] = useState<string | null>(
    latestAttemptId,
  );
  useEffect(() => {
    if (!open) return;
    setSelectedAttemptId(latestAttemptId);
  }, [open, latestAttemptId]);
  const selectedPrompt = availableOptions.find(
    (option) => option.attemptId === selectedAttemptId,
  )?.prompt;
  const content = useMemo(
    () => (open ? ((selectedPrompt ?? prompt)?.trim() || "") : ""),
    [open, prompt, selectedPrompt],
  );
  if (!open) return null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        overlayClassName="bg-black/16 backdrop-blur-md"
        className="max-h-[86vh] max-w-4xl gap-4 overflow-hidden border-border/50 bg-background/68 p-0 shadow-xl shadow-black/10 supports-[backdrop-filter]:bg-background/55"
      >
        <DialogHeader className="border-b px-5 py-4">
          <DialogTitle className="text-base">
            {t("acp.systemPromptTitle")}
          </DialogTitle>
        </DialogHeader>
        <div className="min-h-0 space-y-3 px-5 pb-5">
          {availableOptions.length > 1 ? (
            <Select
              value={selectedAttemptId ?? availableOptions[0]?.attemptId}
              onValueChange={setSelectedAttemptId}
            >
              <SelectTrigger className="h-8 w-[220px]">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {availableOptions.map((option) => (
                  <SelectItem value={option.attemptId} key={option.attemptId}>
                    {option.attemptId}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : null}
          {content ? (
            <pre className="max-h-[64vh] overflow-auto rounded-xl border bg-muted/20 p-4 font-sans text-xs leading-5 text-foreground/85 whitespace-pre-wrap break-words [scrollbar-color:hsl(var(--muted-foreground)/0.35)_transparent] [scrollbar-width:thin] [&::-webkit-scrollbar]:w-2 [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-muted-foreground/30 [&::-webkit-scrollbar-track]:bg-transparent">
              {content}
            </pre>
          ) : (
            <div className="rounded-xl border border-dashed bg-muted/10 p-6 text-sm text-muted-foreground">
              {t("acp.systemPromptEmpty")}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
});

function ACPArtifactsDialog({
  open,
  artifacts,
  attachments,
  selectedArtifact,
  artifactContent,
  artifactLoading,
  onOpenChange,
  onOpenDetail,
  onBack,
}: {
  open: boolean;
  artifacts: AssetItemVm[];
  attachments: AssetItemVm[];
  selectedArtifact: AssetItemVm | null;
  artifactContent: ContentVm | null;
  artifactLoading: boolean;
  onOpenChange: (open: boolean) => void;
  onOpenDetail: (asset: AssetItemVm) => void;
  onBack: () => void;
}) {
  const { t } = useTranslation();
  const allAssets = [
    ...artifacts.map((a) => ({ ...a, kind: "artifact" as const })),
    ...attachments.map((a) => ({ ...a, kind: "attachment" as const })),
  ];

  if (selectedArtifact) {
    const imagePreviewSrc = imageSrcFromContent(artifactContent);
    return (
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent
          overlayClassName="bg-black/16 backdrop-blur-md"
          className="max-h-[86vh] max-w-4xl gap-4 overflow-hidden border-border/50 bg-background/68 p-0 shadow-xl shadow-black/10 supports-[backdrop-filter]:bg-background/55"
        >
          <DialogHeader className="border-b border-border/40 px-5 py-4">
            <div className="flex items-center gap-2">
              <Button
                variant="ghost"
                size="sm"
                className="h-7 gap-1.5 px-2 text-xs"
                onClick={onBack}
              >
                <ChevronDown className="size-3 rotate-90" />
                {t("common.back")}
              </Button>
              <DialogTitle className="truncate text-base">
                {selectedArtifact.title}
              </DialogTitle>
            </div>
          </DialogHeader>
          <div className="min-h-0 flex-1 overflow-auto p-5">
            {artifactLoading ? (
              <div className="flex items-center justify-center py-12 text-sm text-muted-foreground">
                <Loader2 className="mr-2 size-4 animate-spin" />
                {t("common.loading")}
              </div>
            ) : artifactContent ? (
              <div className="space-y-4">
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  <Badge
                    variant="secondary"
                    className="rounded-full px-2.5 text-[11px]"
                  >
                    {selectedArtifact.kind}
                  </Badge>
                  <span>{artifactContent.kind}</span>
                </div>
                {imagePreviewSrc ? (
                  <div className="flex max-h-[64vh] items-center justify-center overflow-hidden rounded-xl border border-border/45 bg-black/5 p-2">
                    <img
                      src={imagePreviewSrc}
                      alt={selectedArtifact.title}
                      draggable={false}
                      className="max-h-[60vh] max-w-full object-contain"
                    />
                  </div>
                ) : (
                  <pre className="max-h-[60vh] overflow-auto rounded-xl border bg-muted/20 p-4 font-sans text-xs leading-5 text-foreground/85 whitespace-pre-wrap break-words">
                    {artifactContent.content}
                  </pre>
                )}
              </div>
            ) : (
              <div className="rounded-xl border border-dashed bg-muted/10 p-6 text-center text-sm text-muted-foreground">
                {t("common.empty")}
              </div>
            )}
          </div>
        </DialogContent>
      </Dialog>
    );
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        overlayClassName="bg-black/16 backdrop-blur-md"
        className="max-h-[86vh] max-w-lg gap-4 overflow-hidden border-border/50 bg-background/68 p-0 shadow-xl shadow-black/10 supports-[backdrop-filter]:bg-background/55"
      >
        <DialogHeader className="border-b px-5 py-4">
          <DialogTitle className="text-base">
            {t("acp.artifactsTitle")}
          </DialogTitle>
        </DialogHeader>
        <div className="min-h-0 space-y-3 overflow-auto px-5 pb-5">
          {attachments.length > 0 ? (
            <section className="space-y-2">
              <div className="flex items-center justify-between gap-3">
                <h3 className="text-sm font-semibold">
                  {t("acp.attachments")}
                </h3>
                <Badge variant="secondary" className="rounded-full px-2.5">
                  {attachments.length}
                </Badge>
              </div>
              <div className="space-y-1.5">
                {attachments.map((item) => (
                  <Button
                    key={`attachment-${item.name}`}
                    variant="outline"
                    className="h-10 w-full justify-start gap-3 rounded-lg border-border/45 bg-background/34 px-3 text-left shadow-none hover:bg-background/42"
                    onClick={() => onOpenDetail(item)}
                  >
                    <Badge
                      variant="secondary"
                      className="shrink-0 rounded-full px-2.5 text-[11px]"
                    >
                      {item.kind}
                    </Badge>
                    <span className="min-w-0 flex-1 truncate text-sm font-medium">
                      {item.title}
                    </span>
                  </Button>
                ))}
              </div>
            </section>
          ) : null}
          {artifacts.length > 0 ? (
            <section className="space-y-2">
              <div className="flex items-center justify-between gap-3">
                <h3 className="text-sm font-semibold">{t("acp.artifacts")}</h3>
                <Badge variant="secondary" className="rounded-full px-2.5">
                  {artifacts.length}
                </Badge>
              </div>
              <div className="space-y-1.5">
                {artifacts.map((item) => (
                  <Button
                    key={`artifact-${item.name}`}
                    variant="outline"
                    className="h-10 w-full justify-start gap-3 rounded-lg border-border/45 bg-background/34 px-3 text-left shadow-none hover:bg-background/42"
                    onClick={() => onOpenDetail(item)}
                  >
                    <Badge
                      variant="secondary"
                      className="shrink-0 rounded-full px-2.5 text-[11px]"
                    >
                      {item.kind}
                    </Badge>
                    <span className="min-w-0 flex-1 truncate text-sm font-medium">
                      {item.title}
                    </span>
                  </Button>
                ))}
              </div>
            </section>
          ) : null}
          {allAssets.length === 0 ? (
            <div className="rounded-xl border border-dashed bg-muted/10 p-6 text-center text-sm text-muted-foreground">
              {t("common.empty")}
            </div>
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function AcpSessionAssetsPanel({
  artifacts,
  attachments,
  onOpenDetail,
}: {
  artifacts: AssetItemVm[];
  attachments: AssetItemVm[];
  onOpenDetail: (asset: AssetItemVm) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const vm = createAcpSessionAssetPanelViewModel(artifacts, attachments);

  if (vm.totalCount === 0) return null;

  return (
    <div className="px-4 pt-1.5">
      <Collapsible
        open={open}
        onOpenChange={setOpen}
        className="w-full border border-b-0 border-border/60 bg-card/60"
      >
        <CollapsibleTrigger asChild>
          <Button
            variant="ghost"
            className="h-auto w-full justify-between rounded-none border-0 px-3 py-2 font-normal shadow-none hover:bg-transparent focus-visible:border-transparent focus-visible:ring-0"
          >
            <span className="flex min-w-0 items-center gap-2 text-xs">
              <Paperclip className="size-3.5 shrink-0 text-muted-foreground" />
              <span className="text-muted-foreground">
                {t("acp.artifactsTitle")}
              </span>
              <span className="flex min-w-0 items-center gap-1.5 truncate font-medium text-foreground">
                {vm.summaryParts.map((part) => (
                  <span key={part.kind} className="shrink-0">
                    {part.kind === "artifact"
                      ? t("acp.assetSummaryArtifact", { count: part.count })
                      : t("acp.assetSummaryAttachment", { count: part.count })}
                  </span>
                ))}
              </span>
            </span>
            <ChevronDown
              className={cn(
                "size-3.5 shrink-0 text-muted-foreground transition-transform",
                open && "rotate-180",
              )}
            />
          </Button>
        </CollapsibleTrigger>
        <CollapsibleContent className="data-[state=closed]:animate-collapsible-up data-[state=open]:animate-collapsible-down overflow-hidden">
          <div className="space-y-1 border-t border-border/40 px-3 pb-2 pt-2">
            {vm.items.map((item) => (
              <AcpSessionAssetPanelRow
                key={`${item.kind}-${item.name}`}
                item={item}
                onOpenDetail={onOpenDetail}
              />
            ))}
          </div>
        </CollapsibleContent>
      </Collapsible>
    </div>
  );
}

function AcpSessionAssetPanelRow({
  item,
  onOpenDetail,
}: {
  item: AcpSessionAssetPanelItem;
  onOpenDetail: (asset: AssetItemVm) => void;
}) {
  const { t } = useTranslation();
  const Icon = item.kind === "artifact" ? FileText : Paperclip;

  return (
    <button
      type="button"
      className="flex w-full min-w-0 items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs transition-colors hover:bg-muted/45"
      onClick={() => onOpenDetail(item)}
    >
      <Icon className="size-3.5 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 truncate font-medium text-foreground">
        {item.title || item.name}
      </span>
      <span className="shrink-0 text-muted-foreground">
        {item.kind === "artifact" ? t("acp.artifacts") : t("acp.attachments")}
      </span>
    </button>
  );
}

export function ACPMessageList({
  timeline,
  sessionStatus,
  sending,
}: {
  timeline: AcpTimelineItem[];
  sessionStatus: string;
  sending: boolean;
  onLayoutChange?: () => void;
}) {
  const active = isSessionActiveStatus(sessionStatus) || sending;
  const streamingTextItemKey = active ? latestStreamingTextItemKey(timeline) : null;
  const expansionControls = useMemo<AcpExpansionControls>(
    () => ({
      expandedItems: {},
      onOpenChange: () => {},
    }),
    [],
  );

  if (timeline.length === 0) return active ? null : <EmptyAcpState />;

  return (
    <div className="min-w-0 space-y-4">
      {timeline.map((item) => (
        <ACPTimelineItemRenderer
          key={timelineEventKey(item)}
          event={item}
          expansionControls={expansionControls}
          streamingTextItemKey={streamingTextItemKey}
        />
      ))}
    </div>
  );
}

function EmptyAcpState() {
  const { t } = useTranslation();
  return (
    <div className="rounded-2xl border border-dashed bg-muted/10 p-8 text-center text-sm text-muted-foreground">
      {t("acp.noEvents")}
    </div>
  );
}

function AcpPendingTimelineState({ label }: { label: string }) {
  return (
    <div className="flex min-h-[10rem] items-center justify-center text-sm text-muted-foreground">
      <div className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className="size-3.5 shrink-0 animate-spin rounded-full border-2 border-primary/25 border-t-primary [animation-duration:900ms]"
        />
        <span className="font-medium text-foreground">{label}...</span>
      </div>
    </div>
  );
}

function AttemptSeparator({ event }: { event: AcpTimelineEvent }) {
  return (
    <div className="flex items-center gap-3 py-1 text-xs text-muted-foreground">
      <span className="h-px flex-1 bg-border/70" />
      <span className="rounded-full border bg-background/90 px-3 py-1 font-mono text-[10px] uppercase tracking-[0.12em]">
        {event.title ?? event.content ?? "attempt"}
      </span>
      <span className="h-px flex-1 bg-border/70" />
    </div>
  );
}

const ACPTimelineItemRenderer = memo(function ACPTimelineItemRenderer({
  event,
  expansionControls,
  streamingTextItemKey,
  projectId,
  taskId,
  onMessageAttachmentClick,
}: {
  event: AcpTimelineItem;
  expansionControls: AcpExpansionControls;
  streamingTextItemKey?: string | null;
  projectId?: string;
  taskId?: string;
  onMessageAttachmentClick?: (att: MessageAttachmentPreview) => void;
}) {
  if (isChildAgentGroup(event))
    return (
      <AssistantTimelineRow timestamp={event.timestamp ?? event.startedAt}>
        <ChildAgentGroupCard
          event={event}
          expansionControls={expansionControls}
          streamingTextItemKey={streamingTextItemKey}
        />
      </AssistantTimelineRow>
    );
  if (event.kind === "attemptSeparator")
    return <AttemptSeparator event={event} />;
  if (event.kind === "textDelta" || event.kind === "userTextDelta")
    return <MessageBubble event={event} streamingTextItemKey={streamingTextItemKey} projectId={projectId} taskId={taskId} onMessageAttachmentClick={onMessageAttachmentClick} />;
  if (event.kind === "thoughtDelta")
    return <ThoughtBlock event={event} expansionControls={expansionControls} />;
  if (event.kind === "toolCall" || event.kind === "toolCallUpdate")
    return <ToolBlock event={event} expansionControls={expansionControls} />;
  if (event.kind === "plan")
    return (
      <AssistantTimelineRow timestamp={event.timestamp}>
        <PlanBlock event={event} />
      </AssistantTimelineRow>
    );
  return null;
});

const ChildAgentGroupCard = memo(function ChildAgentGroupCard({
  event,
  expansionControls,
  streamingTextItemKey,
  onLayoutChange,
}: {
  event: AcpChildAgentGroup;
  expansionControls: AcpExpansionControls;
  streamingTextItemKey?: string | null;
  onLayoutChange?: () => void;
}) {
  const { t } = useTranslation();
  const itemKey = timelineEventKey(event);
  const open = isTimelineItemOpen(itemKey, expansionControls);
  const input = agentToolInput(event.toolEvent);
  const details = toolDetails(event.toolEvent);
  const description = input.description ?? details.queryBlocks[0]?.value;
  const statusTone = toolStatusTone(event.status);
  const statusLabel = event.status
    ? displayStatus(t, event.status)
    : t("acp.subAgentRunning");
  const promptPreview = input.prompt ? truncateText(input.prompt, 240) : null;
  const output = details.output;

  const statusClass =
    statusTone === "danger"
      ? "bg-destructive/10 text-destructive"
      : statusTone === "success"
        ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300"
        : statusTone === "running"
          ? "bg-primary/10 text-primary"
          : "bg-muted text-muted-foreground";
  return (
    <div className="min-w-0 max-w-full overflow-hidden rounded-xl border border-primary/20 bg-card/75 shadow-sm shadow-background/30">
      <Collapsible
        open={open}
        onOpenChange={(next) => {
          expansionControls.onOpenChange(itemKey, next);
          onLayoutChange?.();
        }}
      >
        <CollapsibleTrigger asChild>
          <Button
            variant="ghost"
            className="h-auto w-full min-w-0 justify-between overflow-hidden rounded-none px-3 py-2 font-normal hover:bg-muted/20"
          >
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                <UsersRound className="size-4" />
              </span>
              <span className="min-w-0 flex-1 truncate text-left text-sm">
                <span className="font-semibold text-foreground">
                  {t("acp.subAgent")}
                </span>
                {input.subagentType ? (
                  <span className="ml-2 text-xs text-muted-foreground">
                    {input.subagentType}
                  </span>
                ) : null}
                {description ? (
                  <span className="ml-2 text-xs text-muted-foreground">
                    {description}
                  </span>
                ) : null}
              </span>
            </div>
            <span className="ml-3 flex shrink-0 items-center gap-2">
              {event.events.length > 0 ? (
                <span className="rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
                  {t("acp.subAgentEvents", { count: event.events.length })}
                </span>
              ) : null}
              <span
                className={cn(
                  "rounded-full px-2 py-0.5 text-xs font-medium",
                  statusClass,
                )}
              >
                {statusLabel}
              </span>
              <ChevronDown
                className={cn(
                  "size-4 shrink-0 text-muted-foreground transition-transform",
                  open && "rotate-180",
                )}
              />
            </span>
          </Button>
        </CollapsibleTrigger>
        {open ? (
          <CollapsibleContent className="min-w-0 max-w-full overflow-hidden border-t border-border">
            <div className="min-w-0 max-w-full space-y-3 overflow-hidden bg-background/50 p-3">
              {input.subagentType || description || promptPreview ? (
                <div className="grid min-w-0 gap-2 text-xs sm:grid-cols-2">
                  {input.subagentType ? (
                    <ChildAgentMeta
                      label={t("acp.subAgentType")}
                      value={input.subagentType}
                    />
                  ) : null}
                  {description ? (
                    <ChildAgentMeta
                      label={t("acp.subAgentDescription")}
                      value={description}
                    />
                  ) : null}
                  {promptPreview ? (
                    <ChildAgentMeta
                      className="sm:col-span-2"
                      label={t("acp.subAgentPrompt")}
                      value={promptPreview}
                    />
                  ) : null}
                </div>
              ) : null}
              {event.events.length > 0 ? (
                <div className="min-w-0 max-w-full space-y-3 overflow-hidden rounded-lg border border-border/60 bg-muted/10 p-3">
                  {event.events.map((child) => (
                    <ACPTimelineItemRenderer
                      key={timelineEventKey(child)}
                      event={child}
                      expansionControls={expansionControls}
                      streamingTextItemKey={streamingTextItemKey}
                    />
                  ))}
                </div>
              ) : null}
              {output ? (
                <div className="min-w-0 max-w-full overflow-hidden rounded-lg border bg-background/70 p-2.5 text-xs">
                  <div className="mb-1 font-medium uppercase tracking-wide text-muted-foreground">
                    {t("acp.subAgentResult")}
                  </div>
                  <pre className="max-h-52 min-w-0 overflow-auto whitespace-pre-wrap break-words font-sans text-foreground [overflow-wrap:anywhere]">
                    {formatToolValue(output)}
                  </pre>
                </div>
              ) : null}
            </div>
          </CollapsibleContent>
        ) : null}
      </Collapsible>
    </div>
  );
});

function ChildAgentMeta({
  label,
  value,
  className,
}: {
  label: string;
  value: string;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "min-w-0 overflow-hidden rounded-lg border bg-background/70 px-2.5 py-1.5",
        className,
      )}
    >
      <div className="mb-1 truncate text-muted-foreground">{label}</div>
      <div className="break-words text-foreground [overflow-wrap:anywhere]">
        {value}
      </div>
    </div>
  );
}

const AssistantTimelineRow = memo(function AssistantTimelineRow({
  children,
  timestamp,
  density = "single",
}: {
  children: React.ReactNode;
  timestamp?: string | null;
  density?: "single" | "start" | "middle" | "end";
}) {
  return (
    <Message
      className={cn(
        "min-w-0 items-start justify-start gap-2",
        density !== "single" && "mb-0",
      )}
    >
      <AcpAvatarWithTime tone="assistant" timestamp={timestamp} />
      <div className="w-full min-w-0 max-w-[82%] flex-1">{children}</div>
    </Message>
  );
});

const AcpComposerStatus = memo(function AcpComposerStatus({
  kind,
  active,
  startAt,
  sessionSeconds,
}: {
  kind: AcpProcessingKind;
  active: boolean;
  startAt?: string | null;
  sessionSeconds?: number | null;
}) {
  const { t } = useTranslation();
  const [stepStartAt, setStepStartAt] = useState<string | null>(
    startAt ?? null,
  );
  const previousKind = useRef(kind);

  useEffect(() => {
    if (!active) return;
    if (previousKind.current !== kind || !stepStartAt) {
      previousKind.current = kind;
      setStepStartAt(startAt ?? new Date().toISOString());
    }
  }, [active, kind, startAt, stepStartAt]);

  const stepSeconds = useElapsedSeconds(
    active && kind !== "sending",
    stepStartAt ?? startAt,
  );
  const label = processingLabel(t, kind);
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-2 px-3 pb-1 pt-2 text-xs text-muted-foreground">
      {active ? (
        <>
          <span
            aria-hidden="true"
            className="size-3.5 shrink-0 animate-spin rounded-full border-2 border-primary/25 border-t-primary [animation-duration:900ms]"
          />
          <span className="font-medium text-foreground">{label}</span>
          {kind === "sending" ? (
            <AnimatedEllipsis />
          ) : (
            <span className="flex items-center gap-1.5">
              <span className="text-muted-foreground/80">
                {t("acp.timingStep")}
              </span>
              <span className="tabular-nums text-foreground/80">
                {formatElapsedDuration(stepSeconds)}
              </span>
            </span>
          )}
        </>
      ) : null}
      {sessionSeconds != null ? (
        <span className="flex items-center gap-1.5">
          <span className="text-muted-foreground/80">
            {t("acp.timingSession")}
          </span>
          <span className="tabular-nums text-foreground/80">
            {formatElapsedDuration(sessionSeconds)}
          </span>
        </span>
      ) : null}
    </div>
  );
});

const MessageBubble = memo(function MessageBubble({
  event,
  streamingTextItemKey,
  projectId,
  taskId,
  onMessageAttachmentClick,
}: {
  event: AcpTimelineEvent;
  streamingTextItemKey?: string | null;
  projectId?: string;
  taskId?: string;
  onMessageAttachmentClick?: (att: MessageAttachmentPreview) => void;
}) {
  const { t } = useTranslation();
  const isUser = event.kind === "userTextDelta";
  const failed = event.status === "failed";
  const streamingDraft =
    !isUser && timelineEventKey(event) === streamingTextItemKey;
  const rawAttachments: MessageAttachmentPreview[] =
    rawObject(event.raw)?.attachments as any ?? [];
  const hasAttachments = isUser && !event.optimistic && rawAttachments.length > 0;
  return (
    <Message
      className={cn(
        "min-w-0 items-start gap-2",
        isUser ? "justify-end" : "justify-start",
      )}
    >
      {!isUser ? (
        <AcpAvatarWithTime tone="assistant" timestamp={event.timestamp} />
      ) : null}
      <div
        className={cn(
          "min-w-0 max-w-[82%] space-y-1",
          isUser && "flex flex-col items-end",
        )}
      >
        <MessageContent
          className={cn(
            "rounded-2xl border px-4 py-3 text-sm leading-6 shadow-sm [overflow-wrap:anywhere]",
            isUser
              ? "w-fit max-w-full rounded-br-md border-[color-mix(in_srgb,var(--primary)_26%,var(--border))] bg-[color-mix(in_srgb,var(--primary)_16%,var(--card))] text-foreground shadow-[0_8px_24px_color-mix(in_srgb,var(--primary)_10%,transparent)]"
              : "rounded-bl-md border-border/70 bg-card text-card-foreground",
            failed &&
              "border border-destructive/40 bg-destructive/10 text-destructive",
          )}
        >
          {isUser ? (
            <HiddenPromptMessageContent content={event.content ?? ""} />
          ) : streamingDraft ? (
            <StreamingTextDraft>{event.content ?? ""}</StreamingTextDraft>
          ) : (
            <Markdown>{event.content ?? ""}</Markdown>
          )}
        </MessageContent>
        {hasAttachments ? (
          <div className={cn("flex flex-wrap gap-1.5 px-1", isUser && "justify-end")}>
            {rawAttachments.map((att) => (
              <MessageAttachmentPreviewButton
                key={att.path}
                attachment={att}
                projectId={projectId}
                taskId={taskId}
                onClick={onMessageAttachmentClick}
              />
            ))}
          </div>
        ) : null}
        {event.optimistic || failed ? (
          <div
            className={cn(
              "flex px-1 text-xs text-muted-foreground",
              isUser && "justify-end text-right",
            )}
          >
            {failed ? (
              t("acp.sendFailed")
            ) : (
              <span className="inline-flex items-center">
                {event.status === "processing"
                  ? t("acp.processing")
                  : t("acp.sending")}
                <AnimatedEllipsis />
              </span>
            )}
          </div>
        ) : null}
      </div>
      {isUser ? (
        <AcpAvatarWithTime tone="user" timestamp={event.timestamp} />
      ) : null}
    </Message>
  );
});

const MessageAttachmentPreviewButton = memo(function MessageAttachmentPreviewButton({
  attachment,
  projectId,
  taskId,
  onClick,
}: {
  attachment: MessageAttachmentPreview;
  projectId?: string;
  taskId?: string;
  onClick?: (attachment: MessageAttachmentPreview) => void;
}) {
  const isImage = isImageMessageAttachment(attachment);
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);

  useEffect(() => {
    if (!isImage || !taskId) {
      setPreviewSrc(null);
      return;
    }
    let cancelled = false;
    setPreviewSrc(null);
    showConversationAttachment(projectId ?? 'default', taskId ?? '', attachment.name)
      .then((content) => {
        if (!cancelled) setPreviewSrc(imageSrcFromContent(content));
      })
      .catch(() => {
        if (!cancelled) setPreviewSrc(null);
      });
    return () => {
      cancelled = true;
    };
  }, [attachment.name, isImage, taskId]);

  if (isImage) {
    return (
      <button
        type="button"
        className="group relative size-[72px] overflow-hidden rounded-lg border border-border/60 bg-card/80 text-muted-foreground shadow-sm transition-colors hover:border-primary/45 hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        title={`${attachment.name} (${formatAttachmentSize(attachment.size)})`}
        aria-label={attachment.name}
        onClick={() => onClick?.(attachment)}
      >
        {previewSrc ? (
          <img
            src={previewSrc}
            alt={attachment.name}
            loading="lazy"
            draggable={false}
            className="size-full object-cover"
          />
        ) : (
          <span className="flex size-full items-center justify-center bg-muted/40">
            <ImageIcon className="size-5 text-blue-400" />
          </span>
        )}
        <span className="absolute inset-x-0 bottom-0 truncate bg-background/78 px-1.5 py-1 text-[10px] font-medium text-foreground/80 opacity-0 backdrop-blur-sm transition-opacity group-hover:opacity-100">
          {attachment.name}
        </span>
      </button>
    );
  }

  return (
    <button
      type="button"
      className="flex items-center gap-1.5 rounded-md border border-border/60 bg-card/80 px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      title={`${attachment.name} (${formatAttachmentSize(attachment.size)})`}
      onClick={() => onClick?.(attachment)}
    >
      <FileText className="size-3 text-muted-foreground" />
      <span className="max-w-[120px] truncate">{attachment.name}</span>
    </button>
  );
});

const MessageImagePreviewDialog = memo(function MessageImagePreviewDialog({
  preview,
  onClose,
}: {
  preview: { name: string; src: string } | null;
  onClose: () => void;
}) {
  return (
    <Dialog open={Boolean(preview)} onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent
        showCloseButton={false}
        overlayClassName="bg-black/70"
        className="!w-auto !max-w-[calc(100vw-4rem)] !gap-0 border-0 bg-transparent p-0 shadow-none sm:!max-w-[calc(100vw-4rem)]"
      >
        <DialogTitle className="sr-only">{preview?.name ?? 'Image Preview'}</DialogTitle>
        {preview ? (
          <img
            src={preview.src}
            alt={preview.name}
            draggable={false}
            className="block max-h-[calc(100vh-4rem)] max-w-[calc(100vw-4rem)] object-contain"
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
});

const StreamingTextDraft = memo(function StreamingTextDraft({
  children,
}: {
  children: string;
}) {
  return (
    <div className="min-w-0 max-w-full whitespace-pre-wrap break-words text-sm leading-6 [overflow-wrap:anywhere]">
      {children}
    </div>
  );
});

const AnimatedEllipsis = memo(function AnimatedEllipsis() {
  return (
    <span
      className="inline-flex w-4 items-center justify-start"
      aria-hidden="true"
    >
      <span className="animate-pulse">.</span>
      <span className="animate-pulse [animation-delay:150ms]">.</span>
      <span className="animate-pulse [animation-delay:300ms]">.</span>
    </span>
  );
});

const ThoughtBlock = memo(function ThoughtBlock({
  event,
  expansionControls,
}: {
  event: AcpTimelineEvent;
  expansionControls: AcpExpansionControls;
}) {
  const { t } = useTranslation();
  if (!event.content?.trim()) return null;
  const itemKey = timelineEventKey(event);
  const open = isTimelineItemOpen(itemKey, expansionControls);
  const duration = formatThinkingDuration(t, event.durationMs);
  return (
    <AssistantTimelineRow timestamp={event.timestamp}>
      <ChainOfThought className="min-w-0 max-w-full overflow-hidden rounded-xl border border-border/60 bg-muted/15 px-3.5 py-2 shadow-sm shadow-background/20">
        <ChainOfThoughtStep
          open={open}
          onOpenChange={(next) => expansionControls.onOpenChange(itemKey, next)}
        >
          <ChainOfThoughtTrigger
            leftIcon={<Clock className="size-4" />}
            className="w-full min-w-0 justify-between"
          >
            <span className="flex min-w-0 flex-wrap items-center gap-2">
              <span className="font-medium">{t("acp.thought")}</span>
              {duration ? (
                <span className="rounded-full bg-muted px-2 py-0.5 text-xs tabular-nums">
                  {duration}
                </span>
              ) : null}
            </span>
          </ChainOfThoughtTrigger>
          <ChainOfThoughtContent animated={false}>
            <ChainOfThoughtItem className="break-words whitespace-pre-wrap text-muted-foreground [overflow-wrap:anywhere]">
              {event.content}
            </ChainOfThoughtItem>
          </ChainOfThoughtContent>
        </ChainOfThoughtStep>
      </ChainOfThought>
    </AssistantTimelineRow>
  );
});

const ToolBlock = memo(function ToolBlock({
  event,
  expansionControls,
}: {
  event: AcpTimelineEvent;
  expansionControls: AcpExpansionControls;
}) {
  const { t } = useTranslation();
  const details = toolDetails(event);
  const ToolIcon = toolIcon(details.name);
  const orderedInput: ToolParam[] = details.queryBlocks.map((block) => ({
    label: t(block.labelKey),
    value: block.value,
  }));
  const toolPart: ToolPart = {
    type: details.name ?? t("acp.toolCall"),
    state: toolState(event.status),
    orderedInput: orderedInput.length > 0 ? orderedInput : undefined,
    rawInput: details.rawInput ?? undefined,
    output: details.output ?? undefined,
    summary: toolSummary(details.queryBlocks),
    toolCallId: event.toolCallId ?? undefined,
    errorText:
      event.status && toolStatusTone(event.status) === "danger"
        ? (event.content ?? undefined)
        : undefined,
  };
  const itemKey = timelineEventKey(event);
  const open = isTimelineItemOpen(itemKey, expansionControls);
  return (
    <AssistantTimelineRow timestamp={event.timestamp}>
      <Tool
        toolPart={toolPart}
        labels={toolLabels(t)}
        icon={<ToolIcon className="size-4" />}
        open={open}
        onOpenChange={(next) => expansionControls.onOpenChange(itemKey, next)}
        animated={false}
      />
    </AssistantTimelineRow>
  );
});

function toolLabels(t: ReturnType<typeof useTranslation>["t"]): ToolLabels {
  return {
    input: t("acp.toolParameters"),
    output: t("acp.toolOutput"),
    error: t("status.error"),
    processing: displayStatus(t, "running"),
    pending: displayStatus(t, "pending"),
    ready: t("acp.toolReady"),
    completed: displayStatus(t, "completed"),
  };
}

export function PlanBlock({ event }: { event: AcpTimelineEvent }) {
  const { t } = useTranslation();
  const entries =
    (
      event.raw as
        | {
            entries?: Array<{
              content?: string;
              status?: string;
              priority?: string;
            }>;
          }
        | undefined
    )?.entries ?? [];
  return (
    <Card className="min-w-0 max-w-full overflow-hidden border-primary/20 bg-primary/5 shadow-none">
      <CardContent className="space-y-2 p-4">
        {entries.map((entry, index) => (
          <div
            className="flex min-w-0 items-start gap-2 text-sm"
            key={`${entry.content ?? index}-${index}`}
          >
            <Badge variant="secondary">
              {entry.status
                ? displayStatus(t, entry.status)
                : (entry.priority ?? index + 1)}
            </Badge>
            <span className="min-w-0 break-words [overflow-wrap:anywhere]">
              {entry.content}
            </span>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}

export function PermissionRequestCard({
  request,
  onSelect,
}: {
  request: AcpPermissionRequestVm;
  onSelect: (optionId: string) => void;
}) {
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
              <div className="truncate text-sm font-semibold text-foreground">
                {request.title}
              </div>
              <div className="truncate text-xs text-muted-foreground">
                {t("acp.permissionPending")}
              </div>
            </div>
          </div>
          <div className="grid min-w-0 grid-cols-1 gap-1.5 pl-9 sm:grid-cols-2 sm:gap-2">
            {request.options.map((option) => (
              <Button
                key={option.optionId}
                size="sm"
                variant={
                  option.kind.startsWith("allow") ? "default" : "outline"
                }
                className="h-7 max-w-full justify-center rounded-full px-2.5 text-xs"
                onClick={() => onSelect(option.optionId)}
              >
                <span className="min-w-0 truncate">
                  {option.name || option.optionId}
                </span>
              </Button>
            ))}
          </div>
        </div>
      </div>
    </AssistantTimelineRow>
  );
}

export function RawFrameViewer({
  page,
  query,
  loading,
  onQueryChange,
  onLayoutChange,
}: {
  page: AcpRawFramePageVm | null;
  query: AcpRawFrameQueryInput;
  loading: boolean;
  onQueryChange: (query: AcpRawFrameQueryInput) => void;
  onLayoutChange?: () => void;
}) {
  const { t } = useTranslation();
  const [searchInput, setSearchInput] = useState(query.search ?? "");

  useEffect(() => {
    setSearchInput(query.search ?? "");
  }, [query.search]);

  const pageSize = page?.pageSize ?? query.pageSize ?? 100;
  const applyQuery = (next: AcpRawFrameQueryInput) =>
    onQueryChange({ ...query, ...next });
  const applySearch = () =>
    applyQuery({ page: 0, search: searchInput.trim() || undefined });
  const clearSearch = () => {
    setSearchInput("");
    onQueryChange({
      page: 0,
      pageSize,
      direction: undefined,
      search: undefined,
      kind: undefined,
    });
  };

  if (loading && !page) {
    return (
      <div className="flex items-center gap-2 rounded-2xl border bg-card/70 p-4 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        {t("acp.loadingRawFrames")}
      </div>
    );
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
                placeholder={t("acp.rawSearchPlaceholder")}
                onChange={(event) => setSearchInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") applySearch();
                }}
              />
            </div>
            <Select
              value={query.kind ?? "all"}
              onValueChange={(value) =>
                applyQuery({
                  page: 0,
                  kind: value === "all" ? undefined : value,
                })
              }
            >
              <SelectTrigger className="h-9 lg:w-44">
                <SelectValue placeholder={t("acp.rawKindPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("acp.rawKindAll")}</SelectItem>
                {rawKindOptions(t).map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select
              value={query.direction ?? "all"}
              onValueChange={(value) =>
                applyQuery({
                  page: 0,
                  direction: value === "all" ? undefined : value,
                })
              }
            >
              <SelectTrigger className="h-9 lg:w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("acp.rawDirectionAll")}</SelectItem>
                <SelectItem value="inbound">{t("acp.rawInbound")}</SelectItem>
                <SelectItem value="outbound">{t("acp.rawOutbound")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div className="flex min-w-0 flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
            <span className="min-w-0 truncate">
              {rawFramePageSummary(t, page)}
            </span>
            <div className="flex flex-wrap items-center gap-2">
              {loading ? (
                <Loader2 className="size-3.5 animate-spin text-primary" />
              ) : null}
              <Select
                value={String(pageSize)}
                onValueChange={(value) =>
                  applyQuery({ page: 0, pageSize: Number(value) })
                }
              >
                <SelectTrigger className="h-8 w-24">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="50">50</SelectItem>
                  <SelectItem value="100">100</SelectItem>
                  <SelectItem value="200">200</SelectItem>
                </SelectContent>
              </Select>
              <Button
                size="sm"
                variant="outline"
                className="h-8 rounded-full px-3"
                disabled={loading}
                onClick={applySearch}
              >
                {t("acp.rawSearch")}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                className="h-8 rounded-full px-3"
                disabled={loading}
                onClick={clearSearch}
              >
                {t("acp.rawClear")}
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="h-8 rounded-full px-3"
                disabled={loading || !page || page.page === 0}
                onClick={() => applyQuery({ page: 0 })}
              >
                {t("acp.rawLatest")}
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="h-8 rounded-full px-3"
                disabled={loading || !page?.hasPrevious}
                onClick={() =>
                  applyQuery({ page: Math.max(0, (page?.page ?? 0) - 1) })
                }
              >
                {t("acp.rawNewer")}
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="h-8 rounded-full px-3"
                disabled={loading || !page?.hasNext}
                onClick={() => applyQuery({ page: (page?.page ?? 0) + 1 })}
              >
                {t("acp.rawOlder")}
              </Button>
            </div>
          </div>
        </div>
      </div>

      {page && page.items.length > 0 ? (
        page.items.map((frame) => (
          <RawFrameRow
            key={frame.id}
            frame={frame}
            onLayoutChange={onLayoutChange}
          />
        ))
      ) : (
        <div className="rounded-2xl border border-dashed bg-muted/10 p-8 text-center text-sm text-muted-foreground">
          {t("acp.rawNoFrames")}
        </div>
      )}
    </div>
  );
}

const RawFrameRow = memo(function RawFrameRow({
  frame,
  onLayoutChange,
}: {
  frame: AcpRawFrameVm;
  onLayoutChange?: () => void;
}) {
  const { t } = useTranslation();
  const [expandedContent, setExpandedContent] = useState<string | null>(null);
  const [isOpen, setIsOpen] = useState(false);

  useEffect(() => {
    setExpandedContent(null);
    setIsOpen(false);
  }, [frame.id, frame.content]);

  const handleToggle = useCallback(
    (e: React.SyntheticEvent<HTMLDetailsElement>) => {
      const open = e.currentTarget.open;
      setIsOpen(open);
      onLayoutChange?.();
      if (open && expandedContent === null) {
        try {
          const value = JSON.parse(frame.content);
          setExpandedContent(wrapLongSegments(JSON.stringify(value, null, 2)));
        } catch {
          setExpandedContent(wrapLongSegments(frame.content));
        }
      }
    },
    [expandedContent, frame.content, onLayoutChange],
  );

  const compact = useMemo(
    () => truncateFrameLine(frame.content.trimStart()),
    [frame.content],
  );
  const displayExpanded = expandedContent ?? t("acp.loadingRawFrames");
  const scrollable =
    expandedContent !== null && isLongRawFrame(expandedContent);

  return (
    <details
      onToggle={handleToggle}
      className="group w-full min-w-0 max-w-full overflow-hidden rounded-xl border border-border/60 bg-card/50 text-[11px] leading-5 shadow-sm shadow-background/20 open:border-primary/20 open:bg-card/70 open:ring-1 open:ring-primary/10"
    >
      <summary className="flex w-full min-w-0 cursor-pointer list-none items-center gap-2 overflow-hidden px-3 py-2 text-muted-foreground outline-none transition-colors marker:hidden hover:bg-muted/20 focus-visible:bg-muted/20">
        <span className="shrink-0 select-none tabular-nums text-muted-foreground/80">
          #{frame.lineNumber}
        </span>
        {frame.timestamp ? (
          <span className="hidden shrink-0 tabular-nums text-muted-foreground/70 sm:inline">
            {formatLocalDateTime(frame.timestamp)}
          </span>
        ) : null}
        {frame.direction ? (
          <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">
            {displayRawDirection(t, frame.direction)}
          </span>
        ) : null}
        <span className="shrink-0 rounded-full bg-primary/10 px-2 py-0.5 text-[10px] text-primary">
          {displayRawKind(t, frame.kind)}
        </span>
        <span className="block min-w-0 flex-1 truncate text-foreground/75">
          {compact}
        </span>
        {frame.contentTruncated ? (
          <span className="shrink-0 text-[10px] text-amber-600 dark:text-amber-300">
            truncated
          </span>
        ) : null}
      </summary>
      {isOpen ? (
        <pre
          className={cn(
            "block w-full min-w-0 max-w-full overflow-x-hidden whitespace-pre-wrap break-all border-t border-border/50 bg-background/40 px-4 py-3 font-sans text-foreground/75 outline-none [overflow-wrap:anywhere]",
            scrollable
              ? "max-h-[38rem] overflow-y-auto [scrollbar-color:hsl(var(--muted-foreground)/0.35)_transparent] [scrollbar-width:thin] [&::-webkit-scrollbar]:w-2 [&::-webkit-scrollbar-thumb]:rounded-full [&::-webkit-scrollbar-thumb]:bg-muted-foreground/30 [&::-webkit-scrollbar-thumb]:hover:bg-muted-foreground/45 [&::-webkit-scrollbar-track]:bg-transparent"
              : "overflow-y-visible",
          )}
        >
          {displayExpanded}
        </pre>
      ) : null}
    </details>
  );
});

function useElapsedSeconds(
  active: boolean,
  startAt?: string | null,
  endAt?: string | null,
) {
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

function firstResponseTimestampAfter(
  events: AcpUiEventVm[],
  start: number,
  before?: number | null,
) {
  for (const event of events) {
    if (!isResponseTimingEvent(event)) continue;
    const timestamp = parseAcpTimestamp(event.timestamp);
    if (
      timestamp != null &&
      timestamp >= start &&
      (before == null || timestamp < before)
    )
      return timestamp;
  }
  return null;
}

function promptIdFromEvent(event?: AcpUiEventVm | null) {
  return stringValue(rawObject(event?.raw)?.promptId) ?? null;
}

function isGoldBandUserPrompt(event: AcpUiEventVm) {
  return (
    event.kind === "userTextDelta" &&
    rawObject(event.raw)?.source === "goldBandPrompt"
  );
}

function isGoldBandManagedPrompt(event: AcpUiEventVm) {
  return (
    event.kind === "userTextDelta" &&
    (isGoldBandUserPrompt(event) || isOptimisticEvent(event))
  );
}

function shouldMergeUserPromptEvents(
  previous: AcpUiEventVm | undefined,
  event: AcpUiEventVm,
) {
  if (
    !previous ||
    previous.kind !== "userTextDelta" ||
    event.kind !== "userTextDelta"
  )
    return false;
  if (!sameText(previous.content, event.content)) return false;
  const previousPromptId = promptIdFromEvent(previous);
  const promptId = promptIdFromEvent(event);
  if (previousPromptId || promptId)
    return previousPromptId != null && previousPromptId === promptId;
  return isGoldBandManagedPrompt(previous) !== isGoldBandManagedPrompt(event);
}

function isChildAgentGroup(
  event: AcpTimelineItem,
): event is AcpChildAgentGroup {
  return event.kind === "childAgentGroup";
}

function isTimelineItemOpen(key: string, controls: AcpExpansionControls) {
  return controls.expandedItems[key] ?? false;
}

function isAgentToolCall(event: AcpTimelineEvent) {
  if (event.kind !== "toolCall" && event.kind !== "toolCallUpdate")
    return false;
  const name = toolDetails(event).name?.trim().toLowerCase();
  if (name === "agent") return true;
  if (name !== "task") return false;
  const input = agentToolInput(event);
  return Boolean(input.prompt || input.description || input.subagentType);
}

function isTerminalToolStatus(status?: string | null) {
  return [
    "completed",
    "success",
    "succeeded",
    "failed",
    "error",
    "cancelled",
    "canceled",
  ].includes(status?.toLowerCase() ?? "");
}

function agentToolInput(event: AcpTimelineEvent) {
  const raw = rawObject(event.raw);
  const toolCall = rawObject(raw?.toolCall) ?? rawObject(raw?.content) ?? raw;
  const rawInput = rawObject(toolCall?.rawInput) ?? rawObject(raw?.rawInput);
  return {
    subagentType:
      stringValue(rawInput?.subagent_type) ??
      stringValue(rawInput?.subagentType),
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

function isTopLevelPlanEvent(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const meta = rawObject(raw?._meta);
  const claudeCode = rawObject(meta?.claudeCode);
  return !stringValue(claudeCode?.parentToolUseId);
}

function isResponseTimingEvent(event: AcpUiEventVm) {
  return event.kind !== "userTextDelta";
}

function hasResponseAfterTurn(
  events: AcpUiEventVm[],
  turnStartedAt?: string | null,
) {
  const start = parseAcpTimestamp(turnStartedAt);
  return start != null && firstResponseTimestampAfter(events, start) != null;
}

function processingKindFromTimeline(
  event: AcpTimelineItem | null,
  sending: boolean,
): AcpProcessingKind {
  if (sending) return "sending";
  if (!event) return "launching";
  if (isChildAgentGroup(event))
    return processingKindFromTimeline(
      event.events.at(-1) ?? event.toolEvent,
      sending,
    );
  if (event.kind === "thoughtDelta") return "thinking";
  if (event.kind === "toolCall" || event.kind === "toolCallUpdate")
    return "tool";
  if (event.kind === "textDelta") return "responding";
  return "processing";
}

function processingLabel(
  t: ReturnType<typeof useTranslation>["t"],
  kind: AcpProcessingKind,
) {
  if (kind === "sending") return t("acp.sending");
  if (kind === "stopping") return t("acp.stopping");
  if (kind === "launching-next-node") return t("conversation.runtime.launchingNextNode");
  if (kind === "launching") return t("acp.launchingClaude");
  if (kind === "thinking") return t("acp.thinkingNow");
  if (kind === "tool") return t("acp.toolRunning");
  if (kind === "responding") return t("acp.responding");
  return t("acp.processing");
}

function composerHintText(
  state: ReturnType<typeof deriveAcpRuntimeComposerState>,
  statusLabel: string,
  t: ReturnType<typeof useTranslation>["t"],
) {
  if (state.hintKind === "permission-pending") return t("acp.permissionPending");
  if (state.hintKind === "stopping") return t("acp.stopping");
  if (state.hintKind === "sending") return t("acp.sending");
  if (state.hintKind === "status") return statusLabel;
  if (state.hintKind === "message") return state.message && state.message !== "runtime-error" ? state.message : t("acp.promptInputHint");
  return t("acp.promptInputHint");
}

function composerPlaceholderText(
  state: ReturnType<typeof deriveAcpRuntimeComposerState>,
  t: ReturnType<typeof useTranslation>["t"],
) {
  if (state.placeholderKind === "plan-intervention") return t("acp.planInterventionHint");
  if (state.placeholderKind === "stopping") return t("conversation.runtime.composerStoppingPlaceholder");
  if (state.placeholderKind === "stopped") return t("conversation.runtime.composerStoppedPlaceholder");
  if (state.placeholderKind === "runtime-controlled") return t("conversation.runtime.composerRuntimeControlledPlaceholder");
  if (state.placeholderKind === "message") return state.message && state.message !== "runtime-error" ? state.message : t("acp.composerPlaceholder");
  return t("acp.composerPlaceholder");
}

function findPlanInterventionOption(request: AcpPermissionRequestVm) {
  return (
    request.options.find((option) => {
      const label =
        `${option.optionId} ${option.name} ${option.kind}`.toLowerCase();
      return (
        label.includes("keep planning") ||
        label.includes("继续规划") ||
        label.includes("keep-planning")
      );
    }) ?? null
  );
}

export function permissionRequestIdFromEvent(event: AcpUiEventVm) {
  return canonicalPermissionRequestId(
    stringValue(rawObject(event.raw)?.requestId) ?? event.id,
  );
}

function canonicalPermissionRequestId(value: string) {
  return value.replace(/^(permission-)+/, "");
}

export function pendingPermissionFromEvents(
  events: AcpUiEventVm[],
  dismissedIds: Set<string>,
) {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event.kind !== "permissionRequest" || event.status !== "pending")
      continue;
    const requestId = permissionRequestIdFromEvent(event);
    if (dismissedIds.has(requestId)) continue;
    const raw: Record<string, unknown> = {
      ...(rawObject(event.raw) ?? {}),
      requestId,
    };
    return {
      requestId,
      title: event.title ?? "Permission required",
      toolCallId: event.toolCallId,
      options:
        arrayValue(raw.options)?.map((option) => {
          const value = rawObject(option);
          return {
            optionId: stringValue(value?.optionId) ?? "",
            name: stringValue(value?.name) ?? "",
            kind: stringValue(value?.kind) ?? "",
          };
        }) ?? [],
      raw,
    } satisfies AcpPermissionRequestVm;
  }
  return null;
}

interface PendingElicitationVm {
  elicitationId: string;
  message: string;
  requestedSchema: ElicitationSchema;
  confirmedContent?: Record<string, unknown> | null;
}

export function pendingElicitationFromEvents(
  events: AcpUiEventVm[],
  answeredElicitations: Map<string, Record<string, unknown>>,
): PendingElicitationVm | null {
  const answeredIds = new Set(answeredElicitations.keys());
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (event.kind === "elicitationResponse") {
      const elicitationId =
        stringValue(rawObject(event.raw)?.elicitationId) ??
        event.id.replace(/-response$/, "");
      answeredIds.add(elicitationId);
      continue;
    }
    if (event.kind !== "elicitationRequest") continue;
    if (answeredIds.has(event.id)) continue;
    if (event.status === "pending") {
      const raw = rawObject(event.raw) ?? {};
      const schema: ElicitationSchema =
        typeof raw === "object" && (raw as Record<string, unknown>).type === "object"
          ? (raw as unknown as ElicitationSchema)
          : { type: "object", properties: {} };
      return {
        elicitationId: event.id,
        message: event.content ?? "",
        requestedSchema: schema,
      };
    }
  }
  return null;
}

function visibleSessionError(session: AcpSessionVm, events: AcpUiEventVm[]) {
  const message = session.diagnostics.lastError;
  if (!message) return null;
  const errorAt = parseAcpTimestamp(session.diagnostics.lastErrorTimestamp);
  if (errorAt == null) return message;
  return events.some((event) => isNormalResponseAfterError(event, errorAt))
    ? null
    : message;
}

function isNormalResponseAfterError(event: AcpUiEventVm, errorAt: number) {
  const timestamp = parseAcpTimestamp(event.timestamp);
  if (timestamp == null || timestamp <= errorAt) return false;
  if (
    ![
      "textDelta",
      "thoughtDelta",
      "toolCall",
      "toolCallUpdate",
      "plan",
    ].includes(event.kind)
  )
    return false;
  return toolStatusTone(event.status) !== "danger";
}

function liveEventBufferKey(event: AcpUiEventVm) {
  return liveToolEventBufferKey(event) ?? acpEventKey(event);
}

function liveToolEventBufferKey(event: AcpUiEventVm) {
  if (!isAcpLiveToolEvent(event) || !event.toolCallId) return null;
  const attemptId = attemptIdFromAcpEvent(event) ?? event.sessionId ?? "";
  return `${attemptId}:tool:${event.toolCallId}`;
}

function mergeBufferedLiveEvent(
  previous: AcpUiEventVm | undefined,
  next: AcpUiEventVm,
) {
  if (isAcpLiveToolEvent(next)) {
    return mergeAcpLiveToolEvent(previous, next, mergeRaw);
  }
  if (isAcpTextStreamEventKind(next.kind)) {
    return mergeAcpLiveStreamEvent(previous, next, mergeRaw);
  }
  return next;
}

function useStableAcpTimeline(timeline: AcpTimelineItem[]) {
  const previousRef = useRef<AcpTimelineItem[]>([]);
  return useMemo(() => {
    const stable = stabilizeTimelineItems(timeline, previousRef.current);
    previousRef.current = stable;
    return stable;
  }, [timeline]);
}

function stabilizeTimelineItems(
  nextItems: AcpTimelineItem[],
  previousItems: AcpTimelineItem[],
): AcpTimelineItem[] {
  if (previousItems.length === 0) return nextItems;
  const previousByKey = new Map(
    previousItems.map((item) => [timelineEventKey(item), item]),
  );
  let changed = nextItems.length !== previousItems.length;
  const stableItems = nextItems.map((item) => {
    const previous = previousByKey.get(timelineEventKey(item));
    const stable = stabilizeTimelineItem(item, previous);
    if (stable !== previous) changed = true;
    return stable;
  });
  return changed ? stableItems : previousItems;
}

function stabilizeTimelineItem(
  next: AcpTimelineItem,
  previous?: AcpTimelineItem,
): AcpTimelineItem {
  if (!previous || isChildAgentGroup(next) !== isChildAgentGroup(previous))
    return next;
  if (isChildAgentGroup(next) && isChildAgentGroup(previous)) {
    const events = stabilizeTimelineItems(next.events, previous.events);
    if (
      events === previous.events &&
      next.seq === previous.seq &&
      next.timestamp === previous.timestamp &&
      next.startedSeq === previous.startedSeq &&
      next.endedSeq === previous.endedSeq &&
      next.startedAt === previous.startedAt &&
      next.endedAt === previous.endedAt &&
      next.status === previous.status &&
      next.title === previous.title &&
      next.toolCallId === previous.toolCallId &&
      stabilizeTimelineItem(next.toolEvent, previous.toolEvent) ===
        previous.toolEvent
    ) {
      return previous;
    }
    return {
      ...next,
      events,
      toolEvent: stabilizeTimelineItem(next.toolEvent, previous.toolEvent) as AcpTimelineEvent,
    };
  }
  return sameTimelineEvent(next as AcpTimelineEvent, previous as AcpTimelineEvent)
    ? previous
    : next;
}

function sameTimelineEvent(left: AcpTimelineEvent, right: AcpTimelineEvent) {
  return (
    left.id === right.id &&
    left.seq === right.seq &&
    left.timestamp === right.timestamp &&
    left.kind === right.kind &&
    left.sessionId === right.sessionId &&
    left.content === right.content &&
    left.title === right.title &&
    left.toolCallId === right.toolCallId &&
    left.status === right.status &&
    left.startedSeq === right.startedSeq &&
    left.endedSeq === right.endedSeq &&
    left.startedAt === right.startedAt &&
    left.endedAt === right.endedAt &&
    left.durationMs === right.durationMs &&
    left.optimistic === right.optimistic &&
    left.raw === right.raw
  );
}

function latestStreamingTextItemKey(items: AcpTimelineItem[]): string | null {
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index];
    if (isChildAgentGroup(item)) {
      const childKey: string | null = latestStreamingTextItemKey(item.events);
      if (childKey) return childKey;
      continue;
    }
    if (item.kind === "textDelta") return timelineEventKey(item);
  }
  return null;
}

function buildAcpTimeline(events: AcpUiEventVm[]): AcpTimelineItem[] {
  return groupChildAgentTimeline(buildFlatAcpTimeline(events));
}

function buildFlatAcpTimeline(events: AcpUiEventVm[]) {
  const timeline: AcpTimelineEvent[] = [];
  const toolIndex = new Map<string, AcpTimelineEvent>();
  const seenUserPrompts = new Set<string>();
  for (const event of events) {
    if (!isRenderableEvent(event)) continue;
    if (event.kind === "userTextDelta") {
      const key = userPromptDedupKey(event);
      if (key && seenUserPrompts.has(key)) continue;
      if (key) seenUserPrompts.add(key);
    }
    const previous = timeline[timeline.length - 1];
    if (shouldMergeUserPromptEvents(previous, event)) {
      previous.seq = event.seq;
      previous.endedSeq = event.endedSeq ?? originalSeqFromAcpEvent(event);
      previous.endedAt = event.endedAt ?? event.timestamp;
      previous.status = event.status ?? previous.status;
      previous.raw = mergeRaw(previous.raw, event.raw);
      previous.optimistic = previous.optimistic || isOptimisticEvent(event);
      continue;
    }
    if (
      previous &&
      previous.kind === event.kind &&
      isMergeableDelta(event.kind) &&
      isSameDeltaStream(previous, event)
    ) {
      const merged = mergeAcpLiveStreamEvent(previous, event, mergeRaw);
      previous.content = merged.content;
      previous.seq = merged.seq ?? event.seq;
      previous.endedSeq = merged.endedSeq ?? event.endedSeq ?? originalSeqFromAcpEvent(event);
      previous.endedAt = merged.endedAt ?? event.endedAt ?? event.timestamp;
      previous.status = event.status ?? previous.status;
      previous.raw = merged.raw;
      previous.optimistic = previous.optimistic || isOptimisticEvent(event);
      continue;
    }
    if (
      (event.kind === "toolCall" || event.kind === "toolCallUpdate") &&
      event.toolCallId
    ) {
      const existing = toolIndex.get(event.toolCallId);
      if (existing) {
        existing.kind = "toolCall";
        existing.seq = event.seq;
        existing.endedSeq = event.endedSeq ?? originalSeqFromAcpEvent(event);
        existing.endedAt = event.endedAt ?? event.timestamp;
        existing.title = event.title ?? existing.title;
        existing.status = event.status ?? existing.status;
        existing.content = event.content ?? existing.content;
        existing.raw = mergeRaw(existing.raw, event.raw);
        continue;
      }
      const copy: AcpTimelineEvent = {
        ...event,
        kind: "toolCall",
        startedAt: event.startedAt ?? event.timestamp,
        endedAt: event.endedAt ?? event.timestamp,
        startedSeq: event.startedSeq ?? originalSeqFromAcpEvent(event),
        endedSeq: event.endedSeq ?? originalSeqFromAcpEvent(event),
      };
      toolIndex.set(event.toolCallId, copy);
      timeline.push(copy);
      continue;
    }
    if (event.kind === "thoughtDelta" && !event.content?.trim()) continue;
    if (event.kind === "plan" && isTopLevelPlanEvent(event)) continue;
    timeline.push({
      ...event,
      startedAt: event.startedAt ?? event.timestamp,
      endedAt: event.endedAt ?? event.timestamp,
      startedSeq: event.startedSeq ?? originalSeqFromAcpEvent(event),
      endedSeq: event.endedSeq ?? originalSeqFromAcpEvent(event),
      optimistic: isOptimisticEvent(event),
    });
  }
  let nextTimestamp: number | null = null;
  for (let index = timeline.length - 1; index >= 0; index -= 1) {
    const event = timeline[index];
    const currentTimestamp = parseAcpTimestamp(event.timestamp);
    if (event.kind === "thoughtDelta") {
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

function groupChildAgentTimeline(
  events: AcpTimelineEvent[],
): AcpTimelineItem[] {
  const grouped: AcpTimelineItem[] = [];
  const agentToolCallIds = new Set(
    events
      .filter(isAgentToolCall)
      .map((event) => event.toolCallId)
      .filter(Boolean),
  );
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
    const endSeq = terminal
      ? (event.endedSeq ?? event.seq)
      : Number.POSITIVE_INFINITY;
    const ownedChildren = event.toolCallId
      ? (childrenByParent.get(event.toolCallId) ?? [])
      : [];
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
      kind: "childAgentGroup",
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
  const raw = rawObject(event.raw);
  if (raw?.hiddenFromChat === true) return false;
  if (hiddenEventKinds.has(event.kind)) return false;
  const sessionUpdate = raw?.sessionUpdate;
  return (
    typeof sessionUpdate !== "string" ||
    !hiddenSessionUpdates.has(sessionUpdate)
  );
}

function userPromptDedupKey(event: AcpUiEventVm) {
  const text = normalizePromptText(event.content);
  if (!text) return null;
  const raw = rawObject(event.raw);
  const attemptId = stringValue(raw?.attemptId) ?? attemptIdFromAcpEvent(event) ?? "current-attempt";
  const promptId = promptIdFromEvent(event);
  if (promptId) return `${attemptId}:prompt:${promptId}`;
  if (isGoldBandManagedPrompt(event)) return `${attemptId}:event:${event.id}`;
  return `${attemptId}:text:${text}`;
}

function isMergeableDelta(kind: string) {
  return kind === "textDelta" || kind === "thoughtDelta";
}

function isSameDeltaStream(previous: AcpUiEventVm, event: AcpUiEventVm) {
  return (
    isStableDeltaEvent(previous) &&
    isStableDeltaEvent(event) &&
    previous.kind === event.kind &&
    previous.id === event.id
  );
}

function isStableDeltaEvent(event: AcpUiEventVm) {
  if (event.kind === "userTextDelta" && isOptimisticEvent(event)) return false;
  return isMergeableDelta(event.kind);
}

function rawObject(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function arrayValue(value: unknown): unknown[] | null {
  return Array.isArray(value) ? value : null;
}

function formatAttachmentSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
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
      (merged._meta as Record<string, unknown>).claudeCode = {
        ...previousClaudeCode,
        ...nextClaudeCode,
      };
    }
  }
  return merged;
}

export function mergeAcpEvents(previous: AcpUiEventVm[], next: AcpUiEventVm[]) {
  if (next.length === 0) return previous;
  const replacementByKey = new Map<string, AcpUiEventVm>();
  for (const event of next) {
    const key = acpEventKey(event);
    const existing = replacementByKey.get(key);
    replacementByKey.set(key, existing ? mergeAcpEventForKey(existing, event) : event);
  }
  let allUpdatesReplaceExistingEvents = replacementByKey.size > 0;
  for (const key of replacementByKey.keys()) {
    if (!previous.some((event) => acpEventKey(event) === key)) {
      allUpdatesReplaceExistingEvents = false;
      break;
    }
  }
  if (allUpdatesReplaceExistingEvents) {
    let changed = false;
    const merged = previous.map((event) => {
      const replacement = replacementByKey.get(acpEventKey(event));
      if (!replacement) return event;
      changed = true;
      return mergeAcpEventForKey(event, replacement);
    });
    return changed ? merged : previous;
  }

  const previousByKey = new Map<string, AcpUiEventVm>();
  const byKey = new Map<string, AcpUiEventVm>();
  for (const event of previous) {
    const key = acpEventKey(event);
    previousByKey.set(key, event);
    byKey.set(key, event);
  }
  for (const event of replacementByKey.values()) {
    const key = acpEventKey(event);
    const existing = previousByKey.get(key);
    byKey.set(
      key,
      existing
        ? mergeAcpEventForKey(existing, event)
        : { ...event, seq: alignAcpDisplaySeq(event, previous) },
    );
  }
  return [...byKey.values()].sort((left, right) => left.seq - right.seq);
}

function mergeAcpEventForKey(existing: AcpUiEventVm, incoming: AcpUiEventVm) {
  if (
    isAcpTextStreamEventKind(existing.kind) &&
    isAcpTextStreamEventKind(incoming.kind) &&
    existing.kind === incoming.kind &&
    existing.id === incoming.id
  ) {
    const merged = mergeAcpLiveStreamEvent(existing, incoming, mergeRaw);
    return { ...merged, seq: existing.seq };
  }
  return { ...incoming, seq: existing.seq };
}

function alignAcpDisplaySeq(event: AcpUiEventVm, previous: AcpUiEventVm[]) {
  const attemptId = attemptIdFromAcpEvent(event);
  if (!attemptId) return event.seq;
  const originalSeq = originalSeqFromAcpEvent(event);
  let offset: number | null = null;
  let separatorSeq: number | null = null;
  for (const candidate of previous) {
    if (attemptIdFromAcpEvent(candidate) !== attemptId) continue;
    if (isAcpAttemptSeparator(candidate)) {
      separatorSeq = Math.max(separatorSeq ?? candidate.seq, candidate.seq);
      continue;
    }
    const candidateOriginalSeq = originalSeqFromAcpEvent(candidate);
    offset = Math.max(
      offset ?? candidate.seq - candidateOriginalSeq,
      candidate.seq - candidateOriginalSeq,
    );
  }
  return originalSeq + (offset ?? separatorSeq ?? 0);
}

export function limitAcpEvents(
  events: AcpUiEventVm[],
  trim: "start" | "end",
  eventPageSize: number,
) {
  if (events.length <= eventPageSize) return events;
  return trim === "start"
    ? events.slice(events.length - eventPageSize)
    : events.slice(0, eventPageSize);
}

function acpEventKey(event: AcpUiEventVm) {
  const attemptId = attemptIdFromAcpEvent(event) ?? event.sessionId ?? "";
  const eventId =
    event.kind === "permissionRequest"
      ? `permission-${permissionRequestIdFromEvent(event)}`
      : event.id;
  return `${attemptId}:${event.kind}:${eventId}`;
}

function createLiveAcpSessionShell(events: AcpUiEventVm[], status: string): AcpSessionVm {
  const first = events[0] ?? null;
  const last = events.at(-1) ?? first;
  return {
    sessionId: last?.sessionId ?? first?.sessionId ?? null,
    provider: "acp",
    status,
    sessionStartedAt: first?.startedAt ?? first?.timestamp ?? null,
    sessionUpdatedAt: last?.endedAt ?? last?.timestamp ?? null,
    restored: false,
    events,
    eventPage: {
      loadedCount: events.length,
      total: events.length,
      oldestSeq: first ? originalSeqFromAcpEvent(first) : null,
      newestSeq: last ? originalSeqFromAcpEvent(last) : null,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: first
        ? formatTimelineCursor(originalSeqFromAcpEvent(first))
        : null,
      newestCursor: last
        ? formatTimelineCursor(originalSeqFromAcpEvent(last))
        : null,
    },
    pendingPermissions: [],
    diagnostics: {
      rawFrameCount: 0,
      eventCount: events.length,
      errorCount: 0,
    },
  };
}

function mergeOptimisticSession(
  session: AcpSessionVm | null | undefined,
  optimisticEvents: AcpUiEventVm[],
): AcpSessionVm | null {
  if (!session || optimisticEvents.length === 0) return session ?? null;
  const pending = optimisticEvents.filter((event) =>
    shouldMergeOptimisticEvent(session.events, event),
  );
  if (pending.length === 0) return session;
  return { ...session, events: [...session.events, ...pending] };
}

function isAcpInitialSessionReady(session: AcpSessionVm) {
  return (
    hasAcpSessionMetadata({
      systemPromptAppend: session.systemPromptAppend,
      config: session.config,
    }) && session.events.some(isGoldBandUserPrompt)
  );
}

function sessionsEquivalent(
  previous: AcpSessionVm | null | undefined,
  next: AcpSessionVm | null | undefined,
) {
  if (!previous || !next) return previous === next;
  if (previous.status !== next.status) return false;
  if (previous.sessionUpdatedAt !== next.sessionUpdatedAt) return false;
  if (previous.systemPromptAppend !== next.systemPromptAppend) return false;
  if (acpSessionMetadataSignature(previous) !== acpSessionMetadataSignature(next)) return false;
  if (previous.events.length !== next.events.length) return false;
  const previousLast = previous.events.at(-1);
  const nextLast = next.events.at(-1);
  if (!previousLast || !nextLast) return previousLast === nextLast;
  return (
    previousLast.id === nextLast.id &&
    previousLast.seq === nextLast.seq &&
    previousLast.status === nextLast.status &&
    previousLast.content === nextLast.content &&
    previous.eventPage.hasOlder === next.eventPage.hasOlder &&
    previous.eventPage.hasNewer === next.eventPage.hasNewer
  );
}

function acpSessionMetadataSignature(session: AcpSessionVm) {
  return JSON.stringify({
    sessionId: session.sessionId ?? null,
    title: session.title ?? null,
    adapterId: session.adapterId ?? null,
    adapterDisplayName: session.adapterDisplayName ?? null,
    systemPromptAppend: session.systemPromptAppend ?? null,
    config: session.config ?? null,
  });
}

export {
  timelineEventKey,
  buildAcpTimeline,
  queryBlocksFromTool,
  isTopLevelPlanEvent,
  hasMatchingUserPrompt,
};

export function createAcpPromptId() {
  return `acp-prompt-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function optimisticUserEvent(
  content: string,
  promptId = createAcpPromptId(),
): AcpUiEventVm {
  const createdAt = Math.floor(Date.now() / 1000);
  return {
    id: `optimistic-user-${createdAt}-${Math.random().toString(36).slice(2)}`,
    seq: Number.MAX_SAFE_INTEGER - createdAt,
    timestamp: `${createdAt}Z`,
    kind: "userTextDelta",
    content,
    status: "sending",
    raw: { source: "goldBandPrompt", optimistic: true, promptId },
  };
}

function isOptimisticEvent(event: AcpUiEventVm) {
  return rawObject(event.raw)?.optimistic === true;
}

function shouldMergeOptimisticEvent(
  events: AcpUiEventVm[],
  event: AcpUiEventVm,
) {
  if (event.kind !== "userTextDelta" || event.status === "failed") return false;
  if (hasMatchingUserPrompt(events, event)) return false;
  if (event.status === "sending") return true;
  return !hasResponseAfterTurn(events, event.timestamp);
}

function hasMatchingUserPrompt(
  events: AcpUiEventVm[],
  candidate: AcpUiEventVm,
) {
  if (candidate.kind !== "userTextDelta") return false;
  return Boolean(
    findMatchingGoldBandUserPrompt(
      events,
      candidate.content,
      promptIdFromEvent(candidate),
      candidate.timestamp,
    ),
  );
}

function findMatchingGoldBandUserPrompt(
  events: AcpUiEventVm[],
  content?: string | null,
  promptId?: string | null,
  candidateTimestamp?: string | null,
) {
  if (promptId) {
    const exact = events.find(
      (event) =>
        isGoldBandUserPrompt(event) && promptIdFromEvent(event) === promptId,
    );
    if (exact) return exact;
    const candidateAt = parseAcpTimestamp(candidateTimestamp);
    if (candidateAt == null) return null;
    return (
      events.find((event) => {
        if (!isGoldBandUserPrompt(event)) return false;
        if (promptIdFromEvent(event)) return false;
        if (!sameText(event.content, content)) return false;
        const eventAt = parseAcpTimestamp(event.timestamp);
        return eventAt != null && eventAt >= candidateAt;
      }) ?? null
    );
  }
  return (
    events.find(
      (event) =>
        isGoldBandUserPrompt(event) && sameText(event.content, content),
    ) ?? null
  );
}

function sameText(left?: string | null, right?: string | null) {
  const normalizedLeft = normalizePromptText(left);
  return (
    Boolean(normalizedLeft) && normalizedLeft === normalizePromptText(right)
  );
}

function normalizePromptText(value?: string | null) {
  return value?.replace(/\r\n/g, "\n").replace(/\r/g, "\n").trim() ?? "";
}

function toolDetails(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const toolCall = rawObject(raw?.toolCall) ?? rawObject(raw?.content) ?? raw;
  const fields = rawObject(toolCall?.fields);
  const rawInput = rawObject(toolCall?.rawInput) ?? rawObject(raw?.rawInput);
  const toolCallInput = rawObject(toolCall?.input);
  const locations =
    arrayValue(toolCall?.locations) ?? arrayValue(raw?.locations);
  const meta = rawObject(raw?._meta);
  const claudeCode = rawObject(meta?.claudeCode);
  const title = stringValue(toolCall?.title) ?? event.title;
  const claudeToolName = stringValue(claudeCode?.toolName);
  const name =
    claudeToolName ??
    parseToolTitle(title).name ??
    stringValue(toolCall?.name) ??
    title;
  const output = cleanToolOutput(
    toolCall?.output ?? raw?.output ?? fields?.output ?? raw?.content,
  );
  const fallbackRawInput = toolCallInput ?? rawInput;
  return {
    name,
    output,
    queryBlocks: queryBlocksFromTool(title, rawInput, locations),
    rawInput: fallbackRawInput,
  };
}

function queryBlocksFromTool(
  title: string | null | undefined,
  rawInput?: Record<string, unknown> | null,
  locations?: unknown[] | null,
) {
  const parsedTitle = parseToolTitle(title);
  const blocks: Array<{ labelKey: string; value: string }> = [];
  const push = (labelKey: string, value?: string | null) => {
    const normalized = value?.trim();
    if (
      !normalized ||
      blocks.some(
        (block) => block.labelKey === labelKey && block.value === normalized,
      )
    )
      return;
    blocks.push({ labelKey, value: normalized });
  };

  push("acp.toolPath", parsedTitle.scope);
  push("acp.toolQuery", parsedTitle.query);
  push("acp.toolPath", stringValue(rawInput?.file_path));
  push("acp.toolPath", stringValue(rawInput?.path));
  push("acp.toolPath", stringValue(rawInput?.cwd));
  push("acp.toolQuery", stringValue(rawInput?.pattern));
  push("acp.toolQuery", stringValue(rawInput?.query));
  push("acp.toolQuery", stringValue(rawInput?.glob));
  push("acp.toolQuery", stringValue(rawInput?.command));
  push("acp.toolPath", firstLocationPath(locations));
  return blocks;
}

function toolSummary(blocks: Array<{ value: string }>) {
  const values = blocks.map((block) => block.value.trim()).filter(Boolean);
  return values.length > 0 ? values.join(" · ") : undefined;
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
  const [name] = title.split(" ");
  const quoted = [...title.matchAll(/`([^`]+)`/g)].map((match) => match[1]);
  const rest = title.slice(name.length).trim();
  const plainScope = rest && rest.toLowerCase() !== "file" ? rest : null;
  return {
    name: name || title,
    scope: quoted[0] ?? plainScope,
    query: quoted[1] ?? null,
  };
}

function toolIcon(name: string | null | undefined) {
  const normalized = name?.toLowerCase();
  if (normalized === "read") return FileText;
  if (normalized === "glob" || normalized === "grep") return Search;
  if (normalized === "bash" || normalized === "powershell") return Terminal;
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
  if (value === null) return "null";
  if (value === undefined) return "undefined";
  if (typeof value === "string") return value;
  if (typeof value === "object") return JSON.stringify(value, null, 2);
  return String(value);
}

function truncateText(value: string, maxLength: number) {
  return value.length > maxLength ? `${value.slice(0, maxLength)}…` : value;
}

function displayRawDirection(
  t: ReturnType<typeof useTranslation>["t"],
  direction?: string | null,
) {
  if (direction === "inbound") return t("acp.rawInboundFrame");
  if (direction === "outbound") return t("acp.rawOutboundFrame");
  return direction ?? t("common.unknown");
}

function rawKindOptions(t: ReturnType<typeof useTranslation>["t"]) {
  return [
    { value: "agent_message_chunk", label: t("acp.rawKindAgentMessage") },
    { value: "agent_thought_chunk", label: t("acp.rawKindThought") },
    { value: "tool_call", label: t("acp.rawKindToolCall") },
    { value: "tool_call_update", label: t("acp.rawKindToolUpdate") },
    { value: "usage_update", label: t("acp.rawKindUsage") },
    { value: "available_commands_update", label: t("acp.rawKindCommands") },
    { value: "session/prompt", label: t("acp.rawKindSessionPrompt") },
    { value: "session/new", label: t("acp.rawKindSessionNew") },
    { value: "session/load", label: t("acp.rawKindSessionLoad") },
    { value: "result", label: t("acp.rawKindResult") },
    { value: "error", label: t("acp.rawKindError") },
    { value: "parse-error", label: t("acp.rawKindParseError") },
  ];
}

function displayRawKind(
  t: ReturnType<typeof useTranslation>["t"],
  kind: string,
) {
  const labels: Record<string, string> = {
    initialize: t("acp.rawKindInitialize"),
    "session/new": t("acp.rawKindSessionNew"),
    "session/load": t("acp.rawKindSessionLoad"),
    "session/prompt": t("acp.rawKindSessionPrompt"),
    agent_message_chunk: t("acp.rawKindAgentMessage"),
    agent_thought_chunk: t("acp.rawKindThought"),
    user_message_chunk: t("acp.rawKindUserMessage"),
    tool_call: t("acp.rawKindToolCall"),
    tool_call_update: t("acp.rawKindToolUpdate"),
    usage_update: t("acp.rawKindUsage"),
    available_commands_update: t("acp.rawKindCommands"),
    result: t("acp.rawKindResult"),
    error: t("acp.rawKindError"),
    "parse-error": t("acp.rawKindParseError"),
  };
  return labels[kind] ?? kind;
}

function rawFramePageSummary(
  t: ReturnType<typeof useTranslation>["t"],
  page: AcpRawFramePageVm | null,
) {
  if (!page || page.total === 0) return t("acp.rawMatchCount", { total: 0 });
  const firstLine = page.items[0]?.lineNumber ?? 0;
  const lastLine = page.items.at(-1)?.lineNumber ?? firstLine;
  return t("acp.rawPageSummary", {
    start: firstLine,
    end: lastLine,
    total: page.total,
    page: page.page + 1,
  });
}

function truncateFrameLine(line: string) {
  return line.length > 300 ? `${line.slice(0, 300)}…` : line;
}

function isLongRawFrame(content: string) {
  return content.split("\n").length > 36 || content.length > 5000;
}

function wrapLongSegments(text: string) {
  return text.replace(
    /\S{120,}/g,
    (segment) => segment.match(/.{1,120}/g)?.join("\n") ?? segment,
  );
}

function stringValue(value: unknown) {
  return typeof value === "string" && value.trim() ? value : null;
}

function toolState(status?: string | null): ToolPart["state"] {
  const tone = toolStatusTone(status);
  if (tone === "running") return "input-streaming";
  if (tone === "danger") return "output-error";
  if (tone === "success") return "output-available";
  return "input-available";
}

function toolStatusTone(status?: string | null): ToolTone {
  if (!status) return "muted";
  if (["pending", "sending"].includes(status)) return "pending";
  if (["running", "in_progress"].includes(status)) return "running";
  if (["completed", "success", "succeeded"].includes(status)) return "success";
  if (["failed", "error", "cancelled"].includes(status)) return "danger";
  return "muted";
}

function formatTimelineCursor(seq: number) {
  return `rev:${seq}`;
}

function parseAcpTimestamp(value?: string | null) {
  if (!value) return null;
  const numeric = value.match(/^(\d+(?:\.\d+)?)Z?$/);
  if (numeric) return Number(numeric[1]) * 1000;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function formatThinkingDuration(
  _t: ReturnType<typeof useTranslation>["t"],
  durationMs?: number,
) {
  if (durationMs == null) return null;
  const seconds = Math.max(1, Math.round(durationMs / 1000));
  return formatElapsedDuration(seconds);
}

function formatElapsedDuration(totalSeconds: number) {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const restSeconds = seconds % 60;
  if (minutes < 60)
    return restSeconds ? `${minutes}m ${restSeconds}s` : `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  if (hours < 24)
    return restMinutes ? `${hours}h ${restMinutes}m` : `${hours}h`;
  const days = Math.floor(hours / 24);
  const restHours = hours % 24;
  return restHours ? `${days}d ${restHours}h` : `${days}d`;
}
