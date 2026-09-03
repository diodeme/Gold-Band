import {
  createContext,
  memo,
  type AnimationEvent,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  Bot,
  Check,
  ChevronDown,
  CircleAlert,
  CircleStop,
  Clock,
  Code2,
  Copy,
  Eye,
  FileText,
  Image as ImageIcon,
  ListTodo,
  Loader2,
  Search,
  ShieldQuestion,
  Terminal,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { BrandLoadingState } from "@/components/BrandLoadingState";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
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
  ChainOfThoughtText,
  ChainOfThoughtTrigger,
} from "@/components/prompt-kit/chain-of-thought";
import {
  alignChatContainerViewportToBottomBeforePaint,
  type ChatContainerContentExpansionToken,
  type ChatContainerContext,
  type ChatContainerFollowIntentCause,
  useOptionalChatContainerContentExpansion,
} from "@/components/prompt-kit/chat-container";
import {
  ConversationViewport,
  ConversationViewportFooter,
} from "@/components/conversation/ConversationViewport";
import { InterventionLayer } from "@/components/conversation/InterventionLayer";
import { ImageActionsContextMenu } from "@/components/shared/ImageActionsContextMenu";
import { Markdown } from "@/components/prompt-kit/markdown";
import {
  Message,
  MessageAction,
  MessageActions,
  MessageContent,
} from "@/components/prompt-kit/message";
import {
  Tool,
  type ToolLabels,
  type ToolParam,
  type ToolPart,
} from "@/components/prompt-kit/tool";
import { cn } from "@/lib/utils";
import {
  acpAttemptWorkspaceResourceKey,
  agentTranscriptResourceKey,
  conversationAssetWorkspaceResourceKey,
  createDraftAttachmentWorkspaceResource,
  createHiddenPromptSectionWorkspaceResource,
  draftAttachmentWorkspaceResourceKey,
  useOptionalRightWorkspaceCommands,
  type AcpAttemptWorkspaceLocator,
  type AgentTranscriptLocator,
} from "@/components/workspace/right-workspace-context";
import { formatTokenCount } from "@/lib/format-token";
import { agentIconClass, agentIconSrc } from "@/lib/agent-icons";
import { EditableConversationTitle } from "@/components/conversation/EditableConversationTitle";
import {
  loadSystemPromptViewMode,
  saveSystemPromptViewMode,
  SYSTEM_PROMPT_VIEW_MODES,
  type SystemPromptViewMode,
} from "@/lib/system-prompt-view-pref";
import { goldThemedScrollbarClassName } from "@/lib/themed-scrollbar";
import { BoundedLruCache } from "@/lib/bounded-lru-cache";
import { useImageActions } from "@/hooks/useImageActions";
import {
  AcpLatestWinsEventBuffer,
  decideAcpLiveEventFlush,
  isAcpLiveToolEvent,
  isAcpTextStreamEventKind,
  isCoalescableAcpLiveEvent,
  mergeAcpLiveStreamEvent,
  mergeAcpLiveToolEvent,
} from "@/lib/acp-live-flush";
import {
  ACP_CHAT_LOADED_EVENT_BUFFER_MAX_MULTIPAGE_ITEMS,
  DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE,
  DEFAULT_ACP_CHAT_EVENT_WINDOW_PAGE_COUNT,
  DEFAULT_ACP_CHAT_LOADED_EVENT_BUFFER_LIMIT,
} from "@/lib/acp-chat-pagination";
import {
  DEFAULT_ACP_RESOURCE_CACHE_SESSION_COUNT,
  normalizeAcpResourceCacheSessionCount,
} from "@/lib/acp-chat-resource-cache";
import {
  createAcpSessionConfigViewModel,
  findAcpConfigOption,
  type AcpProviderConfigCatalog,
  type AcpSessionConfigViewModel,
} from "@/lib/acp-session-config";
import {
  groupMessageAttachmentPreviews,
  imageSrcFromContent,
  isImageMessageAttachment,
  isTaskInputMessageAttachment,
  messageAttachmentPreviewsFromRaw,
  type MessageAttachmentPreview,
} from "@/lib/asset-preview";
import {
  revokeAttachmentPreviewUrls,
  useAttachmentPicker,
  useWindowDragGuard,
  type AttachmentItem,
} from "@/lib/attachment-service";
import {
  queuedPromptToAcpComposerDraft,
  useAcpComposerDraft,
  type AcpComposerDraft,
} from "@/lib/acp-composer-draft";
import {
  addComposerQuote,
  createUserPromptSubmission,
  serializeUserPromptSubmission,
  userPromptQuotesFromRaw,
} from "@/lib/composer-context";
import type { ConversationPromptInput } from "@/types";
import type { AgentMessageSelection } from "@/lib/agent-message-selection";
import { AcpConversationComposer } from "@/components/conversation/AcpConversationComposer";
import { AgentSelectionQuoteButton } from "@/components/conversation/AgentSelectionQuoteButton";
import { ConversationPromptQueue } from "@/components/conversation/ConversationPromptQueue";
import { UserMessageQuotes } from "@/components/conversation/UserMessageQuotes";
import { UserMessageDisclosure } from "@/components/conversation/UserMessageDisclosure";
import { parseCommittedSlashCommand, restoreSlashCommandInputFocus } from "@/lib/slash-command";
import { useAgentCommands } from "@/hooks/useAgentCommands";
import { useSlashCommandController } from "@/hooks/useSlashCommandController";
import { AcpAvatar, AcpAvatarWithTime } from "@/components/acp/AcpAvatarWithTime";
import { AcpUsagePanel, hasAcpUsagePanelContent } from "@/components/acp/AcpUsagePanel";
import {
  HiddenPromptMessageContent,
  type HiddenPromptSectionOpenRequest,
} from "@/components/acp/HiddenPromptMessageContent";
import { AcpProcessingSpinner } from "@/components/acp/AcpProcessingSpinner";
import { WorkspaceFileEditor } from "@/components/workspace/files/WorkspaceFileEditor";
import {
  DEFAULT_TURN_ATTACHMENT_CARD_PREVIEW_LIMIT,
  DEFAULT_TURN_FILE_CARD_PREVIEW_LIMIT,
  TurnAttachmentCardPreviewLimitContext,
  TurnFileCardPreviewLimitContext,
  TurnFileChangesCard,
} from "@/components/acp/TurnFileChangesCard";
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
  acpEventKey,
  acpSessionEventsSignature,
  mergeAcpEventSnapshots,
  mergeAcpToolDetailEnrichment,
  mergeAcpEventWindows,
  mergeAcpEventWindowsForSession,
  mergeRawObject,
  permissionRequestIdFromEvent,
  projectLatestAcpUsageUpdate,
} from "@/lib/acp-event-reducer";
import {
  activityProjectionStatus,
  deriveAcpRuntimeComposerState,
  isAcceptedAcpPromptSubmitKind,
  isAcceptedQueuePromptSubmitKind,
  isTerminalAcpLifecycle,
  isTerminalLifecycleForTurn,
  shouldHidePendingAcpInteractions,
  mergeConversationAttemptLifecycle,
  mergeConversationAttemptLiveControlFacets,
  shouldSettleAcpComposerTransientState,
  shouldSettleRuntimeContinueSubmission,
  isRuntimeActiveStatus,
  isSessionActiveStatus,
  isSessionCompletedStatus,
  isSessionTerminalStatus,
  shouldKeepLocalRuntimeLifecycleOverride,
  shouldTreatAcpRuntimeErrorAsFallback,
} from "@/lib/acp-runtime-composer-state";
import {
  hasAcpSessionMetadata,
  isAcpSessionLoadingSurfaceState,
  isAcpSessionInitializationFailed,
  isAcpSessionInitializationInterrupted,
  missingAcpSessionRetryDelay,
  resolveAcpTimelineSurfaceState,
  resolveAcpSessionShellState,
  shouldCreateCancelledDirectAttemptShell,
  shouldCreateLiveAcpSessionShell,
} from "@/lib/acp-session-shell";
import { formatAgentMessageDetailedTime, formatLocalDateTime } from "@/lib/datetime";
import {
  getAcpActivityDetail,
  getAcpToolDetail,
  getAcpRawFrames,
  getAcpSession,
  deleteConversationQueuedPrompt,
  respondAcpPermission,
  respondElicitation,
  continueConversationRuntime,
  recoverConversationRuntime,
  reorderConversationQueuedPrompts,
  restoreConversationQueuedPrompt,
  statAttachmentFiles,
  submitConversationPrompt,
  useConversationQueuedPrompt,
  setAcpSessionModel,
  setAcpSessionConfigOption,
  setAcpSessionPermissionMode,
  showArtifact,
  showAttachment,
  showConversationAttachment,
  showConversationMessageAttachment,
  stopActiveSession,
  submitManualCheck,
} from "@/api";
import { AcpModelThoughtSelects } from '@/components/acp/AcpModelThoughtSelects';
import { AcpSingleConfigMenu } from '@/components/acp/AcpSingleConfigMenu';
import {
  ACP_SESSION_COMPOSER_BORDER_STYLE,
  ACP_SESSION_COMPOSER_LAYOUT,
} from '@/lib/conversation-composer-layout';
import { getRuntimeApi, type AcpSessionUpdatedEventVm } from "@/api/client";
import { isTauriRuntime } from "@/api/shared";
import type { ConversationPromptSubmitVm } from "@/api/client";
import {
  acknowledgeConversationBranchReplay,
  conversationEventMatchesAttempt,
  ensureConversationEventRouterStarted,
  isValidConversationTimelineGeneration,
  readConversationBranchReplaySnapshot,
  reconcileConversationBranchSession,
  resolveConversationBranchDisplayStatus,
  subscribeConversationAttemptEvents,
  useConversationBranchLiveSnapshot,
} from "@/lib/conversation-event-router";
import {
  acpStreamingLocatorMismatches,
  isAcpStreamingDiagnosticsEnabled,
  recordAcpStreamingDiagnostic,
  summarizeAcpStreamingEvent,
} from "@/lib/acp-streaming-diagnostics";
import {
  startAcpReturnToLatestVisualProbe,
  type AcpReturnToLatestVisualProbe,
} from "@/lib/acp-return-to-latest-visual-probe";
import { displayAppError, displayStatus } from "@/i18n";
import type {
  AcpElicitationRequestVm,
  AcpPermissionRequestVm,
  AcpAgentExecutionVm,
  AcpTimelineProjectionVm,
  AcpRawFramePageVm,
  AcpRawFrameOrder,
  AcpRawFrameQueryInput,
  AcpRawFrameVm,
  AcpSessionTimingVm,
  AcpSessionVm,
  AcpUiEventVm,
  AcpUsageVm,
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
  isOrchestrated: boolean;
  lifecycle?: ConversationAttemptLifecycleVm | null;
  promptQueueEnabled?: boolean;
  runtimeStatus?: string | null;
  workflowValid: boolean;
  workflowError?: string | null;
  pauseMessage?: string | null;
  runtimeError?: string | null;
  runtimeErrorFallback?: string | null;
  onRepair?: () => void;
  supersededSessionNavigation?: {
    href: string;
    onNavigate: () => void;
  };
};

export interface AcpDirectSessionHeaderProps {
  title: string;
  onTitleChange?: (title: string) => void;
}

export type AcpInitialSessionQueryState = "loading" | "success" | "error";
type AcpReturnToLatestVisibilitySource =
  | "session-reset"
  | "at-bottom-change"
  | "branch-view-restore"
  | "canonical-head-rejoin"
  | "viewport-scroll";

function isAcpSessionConfigValueUnavailableError(error: unknown) {
  return Boolean(
    error
    && typeof error === "object"
    && (error as { code?: unknown }).code === "acp.session-config-value-unavailable",
  );
}

interface ACPChatDialogProps {
  session?: AcpSessionVm | null;
  providerCatalog?: AcpProviderConfigCatalog | null;
  sessionEstablished?: boolean;
  sessionReferenceId?: string | null;
  projectId: string;
  taskId: string;
  taskUuid?: string | null;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  branchId?: string;
  readOnly?: boolean;
  runtimeComposerContext?: AcpRuntimeComposerContext;
  manualCheckPending?: boolean;
  systemPromptOptions?: Array<{ attemptId: string; prompt?: string | null }>;
  showSystemPromptAction?: boolean;
  showRawFramesAction?: boolean;
  directSessionHeader?: AcpDirectSessionHeaderProps;
  eventIdPrefix?: string;
  eventPageSize?: number;
  eventWindowPageCount?: number;
  inlineContentMaxBytes?: number;
  liveUpdatesPaused?: boolean;
  onOptimisticEventsChange?: (events: AcpUiEventVm[]) => void;
  onManualCheckSubmitted?: () => void;
  onSessionStopped?: () => void;
  onLifecycleSnapshot?: (snapshot: AcpLifecycleSnapshot) => void;
  onAtBottomChange?: (atBottom: boolean) => void;
  onInitialSessionQueryStateChange?: (state: AcpInitialSessionQueryState) => void;
  allowEventOnlySessionShell?: boolean;
  usageCompact?: boolean;
  cacheNamespace?: string;
  turnFileCardPreviewLimit?: number;
  turnAttachmentCardPreviewLimit?: number;
  wallpaperSurface?: boolean;
  worktreePath?: string | null;
  showBranchControl?: boolean;
  managedWorktreeBranch?: string | null;
}

type AcpCanvasMode = "chat" | "raw";

type ToolTone = "muted" | "pending" | "running" | "success" | "danger";
type AcpProcessingKind =
  | "sending"
  | "launching"
  | "processing"
  | "thinking"
  | "tool"
  | "compacting"
  | "responding"
  | "stopping"
  | "preparing-workspace"
  | "processing-workspace"
  | "launching-next-node";
type AcpTimelineEvent = AcpUiEventVm & {
  startedAt?: string;
  endedAt?: string;
  startedSeq?: number;
  endedSeq?: number;
  durationMs?: number;
  optimistic?: boolean;
};

type RuntimeControlOutputDisplay = {
  artifactName?: string;
  kind?: string;
  jsonText?: string;
  start?: number;
  end?: number;
  jsonStart?: number;
  jsonEnd?: number;
  fenced?: boolean;
  parseStatus?: string;
};

export type RuntimeControlMessageParts = {
  display: RuntimeControlOutputDisplay | null;
  visibleText: string;
};

type AcpAgentLink = {
  kind: "agentLink";
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
  agentExecutionId: string;
  attemptId?: string | null;
  parentAgentExecutionId?: string | null;
  attention: boolean;
  description?: string | null;
  toolEvent: AcpTimelineEvent;
  eventCount: number;
  toolCallCount: number;
  readFileCount: number;
  writtenFileCount: number;
};

const AcpBranchLocatorContext = createContext<AgentTranscriptLocator | null>(null);

type AcpTimelineWindowOwner = {
  eventWindowKey: string;
  sessionId: string | null;
  timelineGeneration: number;
};

const AcpTimelineWindowOwnerContext = createContext<AcpTimelineWindowOwner | null>(null);

type AcpActivityBatch = {
  kind: "activityBatch";
  id: string;
  seq: number;
  timestamp?: string;
  startedSeq: number;
  endedSeq: number;
  startedAt?: string;
  endedAt?: string;
  live: boolean;
  events: AcpTimelineEvent[];
  activityStartSeq: number;
  activityEndSeq: number;
  totalEventCount: number;
  toolCallCount: number;
  thoughtCount: number;
  errorCount: number;
  readFileCount: number;
  writtenFileCount: number;
  detailAvailable: boolean;
  hasMoreEarlier: boolean;
  earlierCursor?: string | null;
  sessionId: string | null;
};

type AcpTodoEntry = {
  content?: string;
  status?: string;
  priority?: string;
};

type AcpTimelineProjection = {
  timeline: AcpTimelineItem[];
  todoEntries: AcpTodoEntry[];
};

type AcpTimelineItem = AcpTimelineEvent | AcpAgentLink | AcpActivityBatch;

const MIN_LOADED_EVENT_BUFFER_LIMIT = 30;
export const ACP_ACTIVITY_DETAIL_PAGE_SIZE = 40;
export const ACP_ACTIVITY_DETAIL_WINDOW_PAGE_COUNT = 3;
export const ACP_ACTIVITY_DETAIL_WINDOW_LIMIT =
  ACP_ACTIVITY_DETAIL_PAGE_SIZE * ACP_ACTIVITY_DETAIL_WINDOW_PAGE_COUNT;
const HISTORY_LOAD_THRESHOLD_PX = 240;
const NEWER_PAGE_LOAD_THRESHOLD_PX = 240;
const RETURN_TO_LATEST_SHOW_DISTANCE_PX = 120;
const LIVE_EVENT_FLUSH_MS = 125;
const LIVE_EVENT_INTERACTION_QUIET_MS = 180;
const LIVE_EVENT_MAX_DEFER_MS = 250;
export const ACP_REPLAY_CATCH_UP_MAX_PAGES = 4;
export const ACP_REPLAY_CATCH_UP_MAX_MS = 2_000;
const ACP_SESSION_LEASE_RETRY_MS = 30_000;

export function shouldShowReturnToLatest(
  currentlyVisible: boolean,
  viewportAtBottom: boolean,
  hasNewerEvents: boolean,
  activationEligible: boolean,
  distanceFromBottom: number,
) {
  if (isAcpConversationAtBottom(viewportAtBottom, hasNewerEvents)) return false;
  if (currentlyVisible) return true;
  return activationEligible
    && distanceFromBottom >= RETURN_TO_LATEST_SHOW_DISTANCE_PX;
}

async function awaitAcpReplayCatchUpRequest<T>(
  request: Promise<T>,
  deadlineAt: number,
): Promise<T | null> {
  const remainingMs = Math.max(0, deadlineAt - performance.now());
  if (remainingMs === 0) return null;
  return new Promise<T | null>((resolve) => {
    let settled = false;
    const timer = window.setTimeout(() => {
      if (settled) return;
      settled = true;
      resolve(null);
    }, remainingMs);
    request.then((value) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      resolve(value);
    }).catch(() => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      resolve(null);
    });
  });
}

type AcpPaginationRequestToken = {
  componentInstanceId: string;
  eventWindowKey: string;
  requestSeq: number;
  windowSessionId: string | null;
  windowTimelineGeneration: number;
};

export interface AcpLoadedEventWindow {
  sessionId: string | null;
  timelineGeneration: number;
  events: AcpUiEventVm[];
}

interface AcpOwnedLoadedEventWindow extends AcpLoadedEventWindow {
  eventWindowKey: string;
}

type AcpLoadedEventWindowRelation =
  | "same"
  | "incoming-newer"
  | "incoming-older"
  | "different-session";

type AcpGenerationScopedLiveEvent = {
  event: AcpUiEventVm;
  timelineGeneration: number;
};

type LiveStreamingMarkdownTarget = {
  key: string;
  position: number;
};

type AcpCanonicalHeadHandoffIntent = "ordinary" | "recovery";
type AcpCanonicalHeadHandoff = (
  intent: AcpCanonicalHeadHandoffIntent,
) => Promise<boolean>;

function mergeCanonicalHeadHandoffIntent(
  current: AcpCanonicalHeadHandoffIntent | null,
  incoming: AcpCanonicalHeadHandoffIntent,
) {
  return current === "recovery" || incoming === "recovery"
    ? "recovery"
    : "ordinary";
}
const CONTEXT_COMPACTION_DELAYED_AFTER_SECONDS = 120;

export const ACP_SESSION_SCROLL_AREA_CLASS_NAME = goldThemedScrollbarClassName(
  "h-full min-w-0 overflow-y-auto",
);
export const ACP_RAW_SCROLL_AREA_CLASS_NAME = goldThemedScrollbarClassName(
  "h-full overflow-y-auto p-5",
);

export const ACP_SYSTEM_PROMPT_DIALOG_LAYOUT = {
  dialogContentClassName:
    "max-h-[86vh] gap-4 overflow-hidden border-border/50 bg-background/68 p-0 shadow-xl shadow-black/10 supports-[backdrop-filter]:bg-background/55 flex flex-col sm:max-w-5xl",
  headerClassName: "shrink-0 border-b px-5 py-4",
  scrollContainerClassName: goldThemedScrollbarClassName(
    "min-h-0 min-w-0 flex-1 overflow-hidden",
  ),
  bodyClassName: "relative h-full min-h-0 min-w-0 max-w-full",
  attemptSelectorClassName: "absolute left-2 top-2 z-30",
} as const;

function timelineEventKey(event: AcpTimelineItem | AcpUiEventVm) {
  if (event.kind === "agentLink") return event.id;
  if (event.kind === "activityBatch") return event.id;
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
  "elicitationRequest",
  "elicitationResponse",
  "timingUpdate",
  "rawDiagnostic",
  "runtimeError",
]);

export const ACP_OPTIMISTIC_SESSION_CACHE_LIMIT = 12;
export const DEFAULT_ACP_OPTIMISTIC_EVENTS_PER_SESSION_LIMIT =
  DEFAULT_ACP_CHAT_LOADED_EVENT_BUFFER_LIMIT;
const optimisticEventStore = new BoundedLruCache<string, AcpUiEventVm[]>(
  ACP_OPTIMISTIC_SESSION_CACHE_LIMIT,
);
const optimisticEventListeners = new Map<
  string,
  Set<(events: AcpUiEventVm[]) => void>
>();

function readStoredOptimisticEvents(
  sessionKey: string,
  maxEvents = DEFAULT_ACP_OPTIMISTIC_EVENTS_PER_SESSION_LIMIT,
) {
  const stored = optimisticEventStore.get(sessionKey) ?? [];
  const bounded = boundAcpOptimisticEvents(stored, maxEvents);
  if (bounded !== stored) optimisticEventStore.set(sessionKey, bounded);
  return bounded;
}

function updateStoredOptimisticEvents(
  sessionKey: string,
  updater: (current: AcpUiEventVm[]) => AcpUiEventVm[],
  maxEvents = DEFAULT_ACP_OPTIMISTIC_EVENTS_PER_SESSION_LIMIT,
) {
  const next = updater(readStoredOptimisticEvents(sessionKey, maxEvents));
  return replaceStoredOptimisticEvents(sessionKey, next, maxEvents);
}

function replaceStoredOptimisticEvents(
  sessionKey: string,
  next: AcpUiEventVm[],
  maxEvents = DEFAULT_ACP_OPTIMISTIC_EVENTS_PER_SESSION_LIMIT,
) {
  const boundedNext = boundAcpOptimisticEvents(next, maxEvents);
  if (boundedNext.length === 0) optimisticEventStore.delete(sessionKey);
  else optimisticEventStore.set(sessionKey, boundedNext);
  optimisticEventListeners
    .get(sessionKey)
    ?.forEach((listener) => listener(boundedNext));
  return boundedNext;
}

export function boundAcpOptimisticEvents(
  events: AcpUiEventVm[],
  maxEvents = DEFAULT_ACP_OPTIMISTIC_EVENTS_PER_SESSION_LIMIT,
) {
  const normalizedMaxEvents = Math.max(1, Math.floor(maxEvents));
  return events.length > normalizedMaxEvents
    ? events.slice(-normalizedMaxEvents)
    : events;
}

export function updateAcpOptimisticEvents(
  sessionKey: string,
  updater: (current: AcpUiEventVm[]) => AcpUiEventVm[],
  maxEvents = DEFAULT_ACP_OPTIMISTIC_EVENTS_PER_SESSION_LIMIT,
) {
  return updateStoredOptimisticEvents(sessionKey, updater, maxEvents);
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

export interface AcpBranchViewState {
  anchorKey: string | null;
  anchorOffset: number;
  scrollTop: number;
  atBottom: boolean;
  hasOlder: boolean;
  hasNewer: boolean;
}

interface AcpCachedResource {
  session?: AcpSessionVm;
  eventWindow?: AcpLoadedEventWindow;
  viewState?: AcpBranchViewState;
  contentHydrated?: boolean;
}

let acpResourceCacheSessionCount = DEFAULT_ACP_RESOURCE_CACHE_SESSION_COUNT;
let acpResourceStore = new BoundedLruCache<string, AcpCachedResource>(
  DEFAULT_ACP_RESOURCE_CACHE_SESSION_COUNT,
);

export function configureAcpResourceCacheSessionCount(value?: number) {
  const sessionCount = normalizeAcpResourceCacheSessionCount(value);
  if (sessionCount !== acpResourceCacheSessionCount) {
    acpResourceCacheSessionCount = sessionCount;
    acpResourceStore = new BoundedLruCache<string, AcpCachedResource>(sessionCount);
  }
  return sessionCount;
}

function storeAcpResourcePart(sessionKey: string, patch: Partial<AcpCachedResource>) {
  const current = acpResourceStore.peek(sessionKey) ?? {};
  acpResourceStore.set(sessionKey, { ...current, ...patch });
}

export function restoreAcpSession(sessionKey: string) {
  return acpResourceStore.get(sessionKey)?.session ?? null;
}

export function storeAcpSession(sessionKey: string, session: AcpSessionVm) {
  storeAcpResourcePart(sessionKey, { session });
}

export function hasHydratedAcpSessionContent(sessionKey: string) {
  return acpResourceStore.peek(sessionKey)?.contentHydrated === true;
}

export function markAcpSessionContentHydrated(sessionKey: string) {
  storeAcpResourcePart(sessionKey, { contentHydrated: true });
}

export function restoreAcpBranchViewState(sessionKey: string) {
  return acpResourceStore.get(sessionKey)?.viewState ?? null;
}

export function storeAcpBranchViewState(sessionKey: string, state: AcpBranchViewState) {
  storeAcpResourcePart(sessionKey, { viewState: state });
}

export function shouldInitiallyFollowAcpBranch(
  state?: Pick<AcpBranchViewState, 'atBottom'> | null,
) {
  return state?.atBottom ?? true;
}

function isStoredAcpHistoricalWindow(
  state?: Pick<AcpBranchViewState, 'atBottom' | 'hasNewer'> | null,
) {
  return Boolean(state && (!state.atBottom || state.hasNewer));
}

function acpSessionTimelineGeneration(session?: AcpSessionVm | null) {
  return session?.eventPage.generation ?? 0;
}

export function createAcpLoadedEventWindow(
  session: AcpSessionVm | null | undefined,
  events: AcpUiEventVm[] = session?.events ?? [],
): AcpLoadedEventWindow {
  return {
    sessionId: session?.sessionId ?? null,
    timelineGeneration: acpSessionTimelineGeneration(session),
    events,
  };
}

function compareAcpLoadedEventWindowToSession(
  window: Pick<AcpLoadedEventWindow, "sessionId" | "timelineGeneration">,
  session: AcpSessionVm,
): AcpLoadedEventWindowRelation {
  const incomingSessionId = session.sessionId ?? null;
  if (
    window.sessionId
    && incomingSessionId
    && window.sessionId !== incomingSessionId
  ) {
    return "different-session";
  }
  const incomingGeneration = acpSessionTimelineGeneration(session);
  if (incomingGeneration > window.timelineGeneration) return "incoming-newer";
  if (incomingGeneration < window.timelineGeneration) return "incoming-older";
  return "same";
}

function compareAcpLoadedEventWindowToLiveEvent(
  window: Pick<AcpLoadedEventWindow, "sessionId" | "timelineGeneration">,
  event: AcpUiEventVm,
  timelineGeneration: number,
): AcpLoadedEventWindowRelation {
  if (
    window.sessionId
    && event.sessionId
    && window.sessionId !== event.sessionId
  ) {
    return "different-session";
  }
  if (timelineGeneration > window.timelineGeneration) return "incoming-newer";
  if (timelineGeneration < window.timelineGeneration) return "incoming-older";
  return "same";
}

function acpSessionHasTimelineBeyondLoadedWindow(
  window: AcpLoadedEventWindow,
  session: AcpSessionVm,
) {
  const relation = compareAcpLoadedEventWindowToSession(window, session);
  if (relation === "different-session" || relation === "incoming-newer") return true;
  if (relation === "incoming-older") return false;
  if (session.eventPage.hasNewer) return true;
  const loadedNewestPosition = window.events.reduce<number | null>((latest, event) => (
    Math.max(latest ?? 0, timelineEventPosition(event))
  ), null);
  const incomingNewestPosition = session.eventPage.newestSeq
    ?? session.events.reduce<number | null>((latest, event) => (
      Math.max(latest ?? 0, timelineEventPosition(event))
    ), null);
  return incomingNewestPosition !== null
    && (loadedNewestPosition === null || incomingNewestPosition > loadedNewestPosition);
}

function paginationTokenOwnsLoadedEventWindow(
  token: AcpPaginationRequestToken,
  window: Pick<AcpLoadedEventWindow, "sessionId" | "timelineGeneration">,
) {
  return token.windowTimelineGeneration === window.timelineGeneration
    && token.windowSessionId === window.sessionId;
}

export function restoreAcpLoadedEventWindow(
  sessionKey: string,
  session: AcpSessionVm | null | undefined,
  eventPageSize: number,
  preserveStoredWindow = false,
): AcpLoadedEventWindow {
  const stored = acpResourceStore.get(sessionKey)?.eventWindow;
  const storedOwnsSession = !session
    || stored?.sessionId === (session.sessionId ?? null);
  if (stored && storedOwnsSession && (preserveStoredWindow || !session)) {
    return {
      ...stored,
      events: limitAcpEvents(stored.events, "start", eventPageSize),
    };
  }
  if (!session) return createAcpLoadedEventWindow(null);
  const incoming = createAcpLoadedEventWindow(session);
  if (!stored || compareAcpLoadedEventWindowToSession(stored, session) !== "same") {
    return {
      ...incoming,
      events: limitAcpEvents(incoming.events, "start", eventPageSize),
    };
  }
  const merged = mergeAcpEventWindowsForSession(
    stored.sessionId,
    incoming.sessionId,
    stored.events,
    incoming.events,
    alignAcpDisplaySeq,
  );
  return {
    ...incoming,
    events: limitAcpEvents(merged, "start", eventPageSize),
  };
}

export function storeAcpLoadedEventWindow(
  sessionKey: string,
  eventWindow: AcpLoadedEventWindow,
  eventPageSize: number,
) {
  storeAcpResourcePart(sessionKey, {
    eventWindow: {
      ...eventWindow,
      events: limitAcpEvents(eventWindow.events, "start", eventPageSize),
    },
  });
}

export function resetAcpResourceCache() {
  acpResourceStore.clear();
  optimisticEventStore.clear();
}

export function createAcpSessionCacheKey(
  namespace: string | null | undefined,
  taskId: string,
  runId: string,
  roundId: string,
  nodeId: string,
  attemptId: string,
  projectId?: string | null,
  outerNodeId?: string | null,
  outerAttemptId?: string | null,
  branchId?: string | null,
) {
  return [
    namespace?.trim() || "default",
    projectId?.trim() || "default-project",
    taskId,
    runId,
    roundId,
    nodeId,
    attemptId,
    outerNodeId ?? "",
    outerAttemptId ?? "",
    branchId ?? "root",
  ].join(":");
}

export function createAcpEventWindowCacheKey(input: {
  cacheNamespace?: string | null;
  projectId?: string | null;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
  branchId?: string | null;
  eventIdPrefix?: string | null;
}) {
  const sessionKey = createAcpSessionCacheKey(
    input.cacheNamespace,
    input.taskId,
    input.runId,
    input.roundId,
    input.nodeId,
    input.attemptId,
    input.projectId,
    input.outerNodeId,
    input.outerAttemptId,
    input.branchId,
  );
  return `${sessionKey}:${input.eventIdPrefix ?? ""}`;
}

function normalizeEventPageSize(value?: number) {
  return Number.isFinite(value) && value && value > 0
    ? Math.floor(value)
    : DEFAULT_ACP_CHAT_EVENT_PAGE_SIZE;
}

function normalizeEventWindowPageCount(value?: number) {
  return Number.isFinite(value) && value && value > 0
    ? Math.floor(value)
    : DEFAULT_ACP_CHAT_EVENT_WINDOW_PAGE_COUNT;
}

export function loadedEventBufferLimit(
  eventPageSize: number,
  eventWindowPageCount = DEFAULT_ACP_CHAT_EVENT_WINDOW_PAGE_COUNT,
) {
  const normalizedPageSize = normalizeEventPageSize(eventPageSize);
  const normalizedWindowPageCount = normalizeEventWindowPageCount(eventWindowPageCount);
  return Math.max(
    MIN_LOADED_EVENT_BUFFER_LIMIT,
    normalizedPageSize,
    Math.min(
      ACP_CHAT_LOADED_EVENT_BUFFER_MAX_MULTIPAGE_ITEMS,
      normalizedPageSize * normalizedWindowPageCount,
    ),
  );
}

export function resolveAcpHasOlderEvents(
  sessionHasOlder: boolean,
  mergedEventCount: number,
  visibleEventCount: number,
) {
  return sessionHasOlder || visibleEventCount < mergedEventCount;
}

function conversationReplayHasUncoveredNewerEvents(
  replay: ReturnType<typeof readConversationBranchReplaySnapshot>,
  eventPage: AcpSessionVm["eventPage"],
) {
  if (replay.events.length === 0 && !replay.requiresCatchUp) return false;
  const pageGeneration = eventPage.generation ?? 0;
  if (
    replay.timelineGeneration > 0
    && pageGeneration > 0
    && replay.timelineGeneration !== pageGeneration
  ) {
    return replay.timelineGeneration > pageGeneration;
  }
  const coveredRevision = eventPage.coveredRevision
    ?? eventPage.newestRevision;
  if (replay.headRevision > 0 && coveredRevision != null) {
    return replay.headRevision > coveredRevision;
  }
  return replay.headSeq > (eventPage.newestSeq ?? 0);
}

interface AcpTimelineWatermark {
  generation: number;
  coveredRevision: number;
  coveredSeq: number | null;
}

function reconcileAcpTimelineWatermark(
  current: AcpTimelineWatermark,
  incoming: AcpSessionVm["eventPage"],
): AcpTimelineWatermark {
  const incomingGeneration = incoming.generation ?? current.generation;
  const incomingCoveredRevision = incoming.coveredRevision
    ?? incoming.newestRevision
    ?? 0;
  const incomingCoveredSeq = incoming.newestSeq ?? null;
  if (incomingGeneration > current.generation) {
    return {
      generation: incomingGeneration,
      coveredRevision: incomingCoveredRevision,
      coveredSeq: incomingCoveredSeq,
    };
  }
  if (incomingGeneration < current.generation) return current;
  return {
    generation: current.generation,
    coveredRevision: Math.max(
      current.coveredRevision,
      incomingCoveredRevision,
    ),
    coveredSeq: current.coveredSeq === null
      ? incomingCoveredSeq
      : incomingCoveredSeq === null
        ? current.coveredSeq
        : Math.max(current.coveredSeq, incomingCoveredSeq),
  };
}

function createAcpTimelineWatermark(
  eventPage?: AcpSessionVm["eventPage"] | null,
): AcpTimelineWatermark {
  return eventPage
    ? reconcileAcpTimelineWatermark(
        { generation: 0, coveredRevision: 0, coveredSeq: null },
        eventPage,
      )
    : { generation: 0, coveredRevision: 0, coveredSeq: null };
}

function acpTimelineWatermarkCoversSequenceLoss(
  watermark: AcpTimelineWatermark,
  lossGeneration: number,
  lossSeq: number,
) {
  if (lossSeq <= 0) return true;
  if (lossGeneration > 0 && watermark.generation > lossGeneration) return true;
  if (watermark.generation < lossGeneration) return false;
  if (lossGeneration > 0 && watermark.generation !== lossGeneration) return false;
  return (watermark.coveredSeq ?? 0) >= lossSeq;
}

function pendingAcpInteractionAdvancesCanonicalTimeline(
  event: AcpUiEventVm,
  timelineGeneration: number,
  timelineRevision: number | null | undefined,
  canonicalWatermark: AcpTimelineWatermark,
) {
  if (
    (event.kind !== "permissionRequest" && event.kind !== "elicitationRequest")
    || event.status?.toLowerCase() !== "pending"
  ) {
    return false;
  }
  if (timelineGeneration > canonicalWatermark.generation) return true;
  if (timelineGeneration < canonicalWatermark.generation) return false;
  if (timelineRevision != null) {
    return timelineRevision > canonicalWatermark.coveredRevision;
  }
  const eventPosition = timelineEventPosition(event);
  return canonicalWatermark.coveredSeq !== null
    && eventPosition > canonicalWatermark.coveredSeq;
}

function acpSessionHasPendingInteractionSignal(session: AcpSessionVm) {
  return session.pendingInteractions.length > 0
    || session.events.some((event) => (
      (event.kind === "permissionRequest" || event.kind === "elicitationRequest")
      && event.status?.toLowerCase() === "pending"
    ));
}

function hasAdvancedCanonicalAcpSessionRevision(
  previous: AcpSessionVm,
  next: AcpSessionVm,
) {
  const previousGeneration = previous.eventPage.generation ?? 0;
  const nextGeneration = next.eventPage.generation ?? 0;
  if (previousGeneration !== nextGeneration) return false;
  const previousRevision = previous.eventPage.coveredRevision
    ?? previous.eventPage.newestRevision
    ?? 0;
  const nextRevision = next.eventPage.coveredRevision
    ?? next.eventPage.newestRevision
    ?? 0;
  return nextRevision > previousRevision;
}

function shouldPreferObservedAcpSessionOverCanonicalResponse(
  canonical: AcpSessionVm,
  observed: AcpSessionVm,
) {
  if (!isSameAcpSessionForMetadata(canonical, observed)) return false;
  const canonicalGeneration = canonical.eventPage.generation ?? 0;
  const observedGeneration = observed.eventPage.generation ?? 0;
  if (canonicalGeneration !== observedGeneration) {
    return observedGeneration > canonicalGeneration;
  }
  if (hasAdvancedCanonicalAcpSessionRevision(canonical, observed)) return true;
  if (hasAdvancedCanonicalAcpSessionRevision(observed, canonical)) return false;
  if (hasAdvancedAcpSessionProjection(canonical, observed)) return true;
  if (hasAdvancedAcpSessionProjection(observed, canonical)) return false;
  const canonicalUpdatedAt = parseAcpTimestamp(canonical.sessionUpdatedAt);
  const observedUpdatedAt = parseAcpTimestamp(observed.sessionUpdatedAt);
  if (observedUpdatedAt === null) return false;
  return canonicalUpdatedAt === null || observedUpdatedAt > canonicalUpdatedAt;
}

function hasOlderAcpTimelineProjection(
  previous: AcpSessionVm | null | undefined,
  next: AcpSessionVm | null | undefined,
) {
  if (!previous || !next || !isSameAcpSessionForMetadata(previous, next)) return false;
  const previousGeneration = previous.eventPage.generation;
  const nextGeneration = next.eventPage.generation;
  if (previousGeneration != null && nextGeneration != null) {
    if (nextGeneration < previousGeneration) return true;
    if (nextGeneration > previousGeneration) return false;
  }
  const previousRevision = previous.eventPage.coveredRevision
    ?? previous.eventPage.newestRevision;
  const nextRevision = next.eventPage.coveredRevision
    ?? next.eventPage.newestRevision;
  return previousRevision != null
    && nextRevision != null
    && nextRevision < previousRevision;
}

export type AcpSessionPaginationUpdateMode = "replace" | "append-newer";

export function reconcileAcpEventPageForUpdate(
  previous: AcpSessionVm["eventPage"] | null | undefined,
  incoming: AcpSessionVm["eventPage"],
  mode: AcpSessionPaginationUpdateMode,
): AcpSessionVm["eventPage"] {
  if (mode === "replace" || !previous) return incoming;
  const incomingHasNewest = (incoming.newestSeq ?? Number.NEGATIVE_INFINITY)
    >= (previous.newestSeq ?? Number.NEGATIVE_INFINITY);
  return {
    ...incoming,
    loadedCount: Math.min(
      incoming.total,
      previous.loadedCount + incoming.loadedCount,
    ),
    oldestSeq: previous.oldestSeq ?? incoming.oldestSeq,
    newestSeq: incomingHasNewest ? incoming.newestSeq : previous.newestSeq,
    hasOlder: previous.hasOlder,
    oldestCursor: previous.oldestCursor ?? incoming.oldestCursor,
    newestCursor: incomingHasNewest
      ? incoming.newestCursor
      : previous.newestCursor,
  };
}

export function ACPChatDialog(
  {
    session,
    providerCatalog,
    sessionEstablished = false,
    sessionReferenceId,
    projectId,
    taskId,
    taskUuid,
    runId,
    roundId,
    nodeId,
    attemptId,
    outerNodeId,
    outerAttemptId,
    branchId: requestedBranchId,
    readOnly = false,
    runtimeComposerContext,
    manualCheckPending = false,
    systemPromptOptions,
    showSystemPromptAction = true,
    showRawFramesAction = true,
    directSessionHeader,
    eventIdPrefix,
    eventPageSize,
    eventWindowPageCount,
    inlineContentMaxBytes,
    liveUpdatesPaused: externalLiveUpdatesPaused = false,
    onOptimisticEventsChange,
    onManualCheckSubmitted,
    onSessionStopped,
    onLifecycleSnapshot,
    onAtBottomChange,
    onInitialSessionQueryStateChange,
    allowEventOnlySessionShell = true,
    usageCompact,
    cacheNamespace,
    turnFileCardPreviewLimit = DEFAULT_TURN_FILE_CARD_PREVIEW_LIMIT,
    turnAttachmentCardPreviewLimit = DEFAULT_TURN_ATTACHMENT_CARD_PREVIEW_LIMIT,
    wallpaperSurface = false,
    worktreePath,
    showBranchControl = false,
    managedWorktreeBranch,
  }: ACPChatDialogProps,
) {
  const { t } = useTranslation();
  const rightWorkspace = useOptionalRightWorkspaceCommands();
  const effectiveEventPageSize = normalizeEventPageSize(eventPageSize);
  const effectiveEventWindowPageCount = normalizeEventWindowPageCount(
    eventWindowPageCount,
  );
  const branchId = requestedBranchId ?? session?.branchId ?? 'root';
  const attemptWorkspaceLocator = useMemo<AcpAttemptWorkspaceLocator>(() => ({
    projectId,
    taskId,
    taskUuid,
    runId,
    roundId,
    nodeId,
    attemptId,
    outerNodeId,
    outerAttemptId,
    branchId,
  }), [attemptId, branchId, nodeId, outerAttemptId, outerNodeId, projectId, roundId, runId, taskId, taskUuid]);
  const systemPromptWorkspaceKey = acpAttemptWorkspaceResourceKey('system-prompt', attemptWorkspaceLocator);
  const rawFramesWorkspaceKey = acpAttemptWorkspaceResourceKey('raw-frames', attemptWorkspaceLocator);
  const branchLiveSnapshot = useConversationBranchLiveSnapshot(
    {
      projectId,
      taskId,
      taskUuid,
      runId,
      roundId,
      nodeId,
      attemptId,
      outerNodeId,
      outerAttemptId,
    },
    branchId,
  );
  const effectiveLoadedEventBufferLimit = loadedEventBufferLimit(
    effectiveEventPageSize,
    effectiveEventWindowPageCount,
  );
  const sessionKey = createAcpSessionCacheKey(
    cacheNamespace,
    taskId,
    runId,
    roundId,
    nodeId,
    attemptId,
    projectId,
    outerNodeId,
    outerAttemptId,
    branchId,
  );
  const eventWindowKey = createAcpEventWindowCacheKey({
    cacheNamespace,
    projectId,
    taskId,
    runId,
    roundId,
    nodeId,
    attemptId,
    outerNodeId,
    outerAttemptId,
    branchId,
    eventIdPrefix,
  });
  const sessionIdentity = eventWindowKey;
  const composerDraft = useAcpComposerDraft(eventWindowKey);
  const restoredSession = session ?? restoreAcpSession(eventWindowKey);
  const componentInstanceIdRef = useRef(createAcpChatDialogInstanceId());
  const componentInstanceId = componentInstanceIdRef.current;
  const restoredOptimisticEvents = readStoredOptimisticEvents(
    sessionKey,
    effectiveLoadedEventBufferLimit,
  );
  const restoredBranchViewState = restoreAcpBranchViewState(eventWindowKey);
  const restoredPromptEvent = latestSendingOptimisticEvent(
    restoredOptimisticEvents,
  );
  const restoredPrompt = restoredPromptEvent?.content?.trim() || null;
  const restoredPromptId = promptIdFromEvent(restoredPromptEvent);
  const [currentSession, setCurrentSession] = useState<AcpSessionVm | null>(
    restoredSession,
  );
  const [loadedEventWindow, setLoadedEventWindow] = useState<AcpOwnedLoadedEventWindow>(
    () => ({
      eventWindowKey,
      ...restoreAcpLoadedEventWindow(
        eventWindowKey,
        restoredSession,
        effectiveLoadedEventBufferLimit,
        isStoredAcpHistoricalWindow(restoredBranchViewState),
      ),
    }),
  );
  const loadedEvents = loadedEventWindow.eventWindowKey === eventWindowKey
    ? loadedEventWindow.events
    : [];
  const loadedEventWindowRef = useRef<AcpOwnedLoadedEventWindow>(loadedEventWindow);
  const [optimisticEvents, setOptimisticEvents] = useState<AcpUiEventVm[]>(
    () => restoredOptimisticEvents,
  );
  const optimisticEventsRef = useRef<AcpUiEventVm[]>(optimisticEvents);
  optimisticEventsRef.current = optimisticEvents;
  const onOptimisticEventsChangeRef = useRef(onOptimisticEventsChange);
  onOptimisticEventsChangeRef.current = onOptimisticEventsChange;
  const prompt = composerDraft.draft.content;
  const setPrompt = composerDraft.setContent;
  const quotes = composerDraft.draft.quotes;
  const setQuotes = composerDraft.setQuotes;
  const conversationRootRef = useRef<HTMLDivElement>(null);
  const [composerContextError, setComposerContextError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [queueSubmitPending, setQueueSubmitPending] = useState(false);
  const [promptCommandPending, setPromptCommandPending] = useState(false);
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
  const activeTurnPromptRef = useRef(activeTurnPrompt);
  const activeTurnPromptIdRef = useRef(activeTurnPromptId);
  activeTurnPromptRef.current = activeTurnPrompt;
  activeTurnPromptIdRef.current = activeTurnPromptId;
  const [streamingMarkdownItemKey, setStreamingMarkdownItemKey] = useState<string | null>(null);
  const [sendError, setSendError] = useState<string | null>(null);
  const [cancelError, setCancelError] = useState<string | null>(null);
  const [manualCheckError, setManualCheckError] = useState<string | null>(null);
  const [manualCheckSubmitting, setManualCheckSubmitting] = useState(false);
  const [manualCheckResolved, setManualCheckResolved] = useState(false);
  const [runtimeContinueSubmitting, setRuntimeContinueSubmitting] = useState(false);
  const [runtimeContinueError, setRuntimeContinueError] = useState<string | null>(null);
  const [canvasMode, setCanvasMode] = useState<AcpCanvasMode>("chat");
  const [systemPromptOpen, setSystemPromptOpen] = useState(false);
  const [rawPage, setRawPage] = useState<AcpRawFramePageVm | null>(null);
  const [rawQuery, setRawQuery] = useState<AcpRawFrameQueryInput>({
    page: 0,
    pageSize: 100,
    order: "desc",
  });
  const [rawLoading, setRawLoading] = useState(false);
  const [initialSessionQueryState, setInitialSessionQueryState] = useState<
    AcpInitialSessionQueryState
  >(() => (
    !isTauriRuntime() || hasHydratedAcpSessionContent(eventWindowKey)
      ? "success"
      : "loading"
  ));
  const [sessionLoadError, setSessionLoadError] = useState<string | null>(null);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [returnToLatestPending, setReturnToLatestPending] = useState(false);
  const [hasOlderEvents, setHasOlderEvents] = useState(
    () => restoredBranchViewState?.hasOlder
      ?? restoredSession?.eventPage.hasOlder
      ?? false,
  );
  const [hasNewerEvents, setHasNewerEvents] = useState(
    () => restoredBranchViewState?.hasNewer
      ?? restoredSession?.eventPage.hasNewer
      ?? false,
  );
  const hasOlderEventsRef = useRef(hasOlderEvents);
  const hasNewerEventsRef = useRef(hasNewerEvents);
  hasOlderEventsRef.current = hasOlderEvents;
  hasNewerEventsRef.current = hasNewerEvents;
  const commitHasNewerEvents = useCallback((next: boolean) => {
    hasNewerEventsRef.current = next;
    setHasNewerEvents(next);
  }, []);
  const replayHasUncoveredNewerEvents = useCallback(
    (eventPage: AcpSessionVm["eventPage"]) => conversationReplayHasUncoveredNewerEvents(
      readConversationBranchReplaySnapshot(attemptWorkspaceLocator, branchId),
      eventPage,
    ),
    [attemptWorkspaceLocator, branchId],
  );
  const [dismissedPermissionIds, setDismissedPermissionIds] = useState<
    Set<string>
  >(() => new Set());
  const [permissionError, setPermissionError] = useState<string | null>(null);
  const [answeredElicitations, setAnsweredElicitations] = useState<
    Map<string, Record<string, unknown>>
  >(() => new Map());
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
    handlePaste,
  } = useAttachmentPicker({
    attachments: [composerDraft.draft.attachments, composerDraft.setAttachments],
    inlineContentMaxBytes,
  });
  useWindowDragGuard();
  const paginationDirectionRef = useRef<"older" | "newer" | null>(null);
  const preservingScrollRef = useRef(false);
  const chatContainerContextRef = useRef<ChatContainerContext | null>(null);
  const viewportAtBottomRef = useRef(restoredBranchViewState?.atBottom ?? true);
  const viewportManualIntentRef = useRef(
    !(restoredBranchViewState?.atBottom ?? true),
  );
  const [showReturnToLatest, setShowReturnToLatest] = useState(false);
  const showReturnToLatestRef = useRef(false);
  const returnToLatestButtonRef = useRef<HTMLButtonElement | null>(null);
  const returnToLatestButtonMountSequenceRef = useRef(0);
  const returnToLatestVisualProbeRef = useRef<AcpReturnToLatestVisualProbe | null>(null);
  const pendingBranchViewRestoreRef = useRef<AcpBranchViewState | null>(restoredBranchViewState);
  const cancelRequestedRef = useRef(false);
  const awaitTerminalStopRef = useRef(false);
  const terminalSessionNotifiedRef = useRef(false);
  const [stopCommandPending, setStopCommandPending] = useState(false);
  const [runtimeStopAccepted, setRuntimeStopAccepted] = useState(false);
  const [localRuntimeLifecycle, setLocalRuntimeLifecycle] = useState<ConversationAttemptLifecycleVm | null>(null);
  const lifecycleProjectionRef = useRef<ConversationAttemptLifecycleVm | null>(
    runtimeComposerContext?.lifecycle ?? null,
  );
  const [queueMutationPending, setQueueMutationPending] = useState(false);
  const [queueRestorePending, setQueueRestorePending] = useState(false);
  const latestSessionRef = useRef<AcpSessionVm | null>(restoredSession);
  const canonicalTimelineCoverageRef = useRef<{
    eventWindowKey: string;
    watermark: AcpTimelineWatermark;
  }>({
    eventWindowKey,
    watermark: createAcpTimelineWatermark(),
  });
  const sessionRefreshSeqRef = useRef(0);
  const sessionIdentityRef = useRef(sessionIdentity);
  sessionIdentityRef.current = sessionIdentity;
  const paginationRequestSeqRef = useRef(0);
  const paginationRequestOwnerRef = useRef<AcpPaginationRequestToken | null>(null);
  const paginationCursorGenerationStaleRef = useRef(false);
  const configGenerationRef = useRef(0);
  const configMutationGenerationRef = useRef(0);
  const composerTextareaRef = useRef<HTMLTextAreaElement>(null);
  const paginationAnchorRef = useRef<{ key: string; top: number } | null>(null);
  const pendingLiveEventsRef = useRef(
    new AcpLatestWinsEventBuffer<AcpGenerationScopedLiveEvent>(
      effectiveLoadedEventBufferLimit,
    ),
  );
  const liveEventFlushTimerRef = useRef<number | null>(null);
  const liveUpdatesDeferredUntilRef = useRef(0);
  const pendingLiveEventsSinceRef = useRef<number | null>(null);
  const liveUpdatesPausedRef = useRef(false);
  const liveBeforeReadyLogCountRef = useRef(0);
  const liveAnimationReadyRef = useRef(false);
  const liveStreamingTargetRef = useRef<LiveStreamingMarkdownTarget | null>(null);
  const pendingLatestLayoutCommitRef = useRef<string | null>(null);
  const canonicalHeadHandoffRef = useRef<AcpCanonicalHeadHandoff | null>(null);
  const canonicalHeadRecoveryPendingRef = useRef(false);
  const canonicalHeadHandoffInFlightRef = useRef(false);
  const canonicalHeadHandoffTrailingIntentRef = useRef<
    AcpCanonicalHeadHandoffIntent | null
  >(null);
  const canonicalHeadHandoffEpochRef = useRef(0);
  const canonicalHeadRecoveryAutoHandoffRef = useRef(false);
  const returnToLatestPendingRef = useRef(false);
  const sessionPropSyncIdentityRef = useRef(eventWindowKey);
  const sessionResetIdentityRef = useRef(eventWindowKey);
  const createReturnToLatestDiagnosticDetails = useCallback((
    scroller?: HTMLElement | null,
  ) => {
    const viewport = scroller
      ?? chatContainerContextRef.current?.scrollRef.current
      ?? null;
    const scrollTop = viewport?.scrollTop ?? null;
    const scrollHeight = viewport?.scrollHeight ?? null;
    const clientHeight = viewport?.clientHeight ?? null;
    return {
      componentInstanceId,
      sessionIdentity: sessionIdentityRef.current,
      scrollTop,
      scrollHeight,
      clientHeight,
      distanceFromBottom:
        scrollHeight === null || scrollTop === null || clientHeight === null
          ? null
          : scrollHeight - scrollTop - clientHeight,
      viewportAtBottom: viewportAtBottomRef.current,
      viewportManualIntent: viewportManualIntentRef.current,
      hasNewerEvents: hasNewerEventsRef.current,
      liveStreamingTargetKey: liveStreamingTargetRef.current?.key ?? null,
    };
  }, [componentInstanceId]);
  const commitShowReturnToLatest = useCallback((
    next: boolean,
    source: AcpReturnToLatestVisibilitySource,
    scroller?: HTMLElement | null,
  ) => {
    const previous = showReturnToLatestRef.current;
    if (previous === next) return;
    recordAcpStreamingDiagnostic("return-to-latest-trace", () => ({
      event: "visibility-change",
      source,
      previousVisible: previous,
      nextVisible: next,
      ...createReturnToLatestDiagnosticDetails(scroller),
    }));
    showReturnToLatestRef.current = next;
    setShowReturnToLatest(next);
  }, [createReturnToLatestDiagnosticDetails]);
  const commitReturnToLatestPending = useCallback((next: boolean) => {
    if (returnToLatestPendingRef.current === next) return;
    returnToLatestPendingRef.current = next;
    setReturnToLatestPending(next);
  }, []);
  const handleReturnToLatestButtonRef = useCallback((
    node: HTMLButtonElement | null,
  ) => {
    const previous = returnToLatestButtonRef.current;
    if (previous === node) return;
    if (previous) {
      returnToLatestVisualProbeRef.current?.stop(
        node ? "button-replaced" : "button-detach",
      );
      returnToLatestVisualProbeRef.current = null;
      recordAcpStreamingDiagnostic("return-to-latest-trace", () => ({
        event: "dom-detach",
        visible: showReturnToLatestRef.current,
        mountSequence: returnToLatestButtonMountSequenceRef.current,
        ...createReturnToLatestDiagnosticDetails(),
      }));
    }
    returnToLatestButtonRef.current = node;
    if (node) {
      returnToLatestButtonMountSequenceRef.current += 1;
      recordAcpStreamingDiagnostic("return-to-latest-trace", () => ({
        event: "dom-attach",
        visible: showReturnToLatestRef.current,
        mountSequence: returnToLatestButtonMountSequenceRef.current,
        ...createReturnToLatestDiagnosticDetails(),
      }));
    }
  }, [createReturnToLatestDiagnosticDetails]);
  useLayoutEffect(() => {
    const button = returnToLatestButtonRef.current;
    if (!button || !isAcpStreamingDiagnosticsEnabled()) return;
    if (!returnToLatestVisualProbeRef.current) {
      returnToLatestVisualProbeRef.current = startAcpReturnToLatestVisualProbe({
        button,
        viewport: chatContainerContextRef.current?.scrollRef.current ?? null,
        content: chatContainerContextRef.current?.contentRef.current ?? null,
        getDiagnosticDetails: createReturnToLatestDiagnosticDetails,
      });
    }
    returnToLatestVisualProbeRef.current.recordReactCommit();
  });
  const liveUpdatesPaused = Boolean(externalLiveUpdatesPaused || systemPromptOpen);
  liveUpdatesPausedRef.current = liveUpdatesPaused;

  const markCanonicalHeadRecovery = useCallback((autoHandoff: boolean) => {
    canonicalHeadRecoveryPendingRef.current = true;
    canonicalHeadRecoveryAutoHandoffRef.current =
      canonicalHeadRecoveryAutoHandoffRef.current || autoHandoff;
    commitHasNewerEvents(true);
  }, [commitHasNewerEvents]);

  const commitLoadedEventWindow = useCallback((
    ownerKey: string,
    next: AcpLoadedEventWindow,
  ) => {
    if (sessionIdentityRef.current !== ownerKey) return false;
    const previous = loadedEventWindowRef.current;
    const scopeChanged = previous.eventWindowKey !== ownerKey;
    const timelineOwnerChanged = scopeChanged
      || previous.timelineGeneration !== next.timelineGeneration
      || previous.sessionId !== next.sessionId;
    if (timelineOwnerChanged) {
      if (liveEventFlushTimerRef.current !== null) {
        window.clearTimeout(liveEventFlushTimerRef.current);
        liveEventFlushTimerRef.current = null;
      }
      pendingLiveEventsRef.current.clear();
      pendingLiveEventsSinceRef.current = null;
      paginationCursorGenerationStaleRef.current = false;
    }
    if (scopeChanged) {
      canonicalHeadHandoffEpochRef.current += 1;
      canonicalHeadRecoveryPendingRef.current = false;
      canonicalHeadHandoffInFlightRef.current = false;
      canonicalHeadHandoffTrailingIntentRef.current = null;
      canonicalHeadRecoveryAutoHandoffRef.current = false;
    }
    const ownedNext = { ...next, eventWindowKey: ownerKey };
    loadedEventWindowRef.current = ownedNext;
    storeAcpLoadedEventWindow(
      ownerKey,
      next,
      effectiveLoadedEventBufferLimit,
    );
    setLoadedEventWindow(ownedNext);
    return true;
  }, [effectiveLoadedEventBufferLimit]);

  const updateOptimisticEvents = useCallback((
    updater: (current: AcpUiEventVm[]) => AcpUiEventVm[],
  ) => {
    const next = updater(optimisticEventsRef.current);
    const boundedNext = replaceStoredOptimisticEvents(
      sessionKey,
      next,
      effectiveLoadedEventBufferLimit,
    );
    optimisticEventsRef.current = boundedNext;
    setOptimisticEvents(boundedNext);
    onOptimisticEventsChangeRef.current?.(boundedNext);
  }, [effectiveLoadedEventBufferLimit, sessionKey]);

  const isHistoricalTimelineWindow = useCallback(() => (
    hasNewerEventsRef.current
    || paginationDirectionRef.current !== null
    || !viewportAtBottomRef.current
  ), []);

  const hasExplicitHistoricalTimelineIntent = useCallback(() => (
    paginationDirectionRef.current !== null
    || viewportManualIntentRef.current
  ), []);

  const ownsPaginationRequest = useCallback((token: AcpPaginationRequestToken) => {
    const owner = paginationRequestOwnerRef.current;
    return Boolean(
      owner
      && owner.componentInstanceId === token.componentInstanceId
      && owner.eventWindowKey === token.eventWindowKey
      && owner.requestSeq === token.requestSeq
      && sessionIdentityRef.current === token.eventWindowKey
    );
  }, []);

  const ownsPaginationWindowRequest = useCallback((token: AcpPaginationRequestToken) => (
    ownsPaginationRequest(token)
    && paginationTokenOwnsLoadedEventWindow(
      token,
      loadedEventWindowRef.current,
    )
  ), [ownsPaginationRequest]);

  const beginPaginationRequest = useCallback((direction: "older" | "newer") => {
    const eventWindow = loadedEventWindowRef.current;
    const token: AcpPaginationRequestToken = {
      componentInstanceId,
      eventWindowKey,
      requestSeq: paginationRequestSeqRef.current + 1,
      windowSessionId: eventWindow.sessionId,
      windowTimelineGeneration: eventWindow.timelineGeneration,
    };
    paginationRequestSeqRef.current = token.requestSeq;
    paginationRequestOwnerRef.current = token;
    paginationDirectionRef.current = direction;
    return token;
  }, [componentInstanceId, eventWindowKey]);

  const finishPaginationRequest = useCallback((token: AcpPaginationRequestToken) => {
    if (!ownsPaginationRequest(token)) return false;
    paginationRequestOwnerRef.current = null;
    paginationDirectionRef.current = null;
    return true;
  }, [ownsPaginationRequest]);

  const settleOptimisticPromptAdmissions = useCallback((events: AcpUiEventVm[]) => {
    const admittedPrompts = events.filter(isGoldBandUserPrompt);
    if (admittedPrompts.length === 0) return;
    const acceptedActivePrompt = findMatchingGoldBandUserPrompt(
      admittedPrompts,
      activeTurnPromptRef.current,
      activeTurnPromptIdRef.current,
    );
    if (acceptedActivePrompt) {
      setActiveTurnStartedAt((current) => current ?? acceptedActivePrompt.timestamp);
      setSending(false);
    }
    updateOptimisticEvents((current) => {
      let changed = false;
      const next = current.map((event) => {
        if (!hasMatchingUserPrompt(admittedPrompts, event)) return event;
        const eventRaw = rawObject(event.raw);
        if (event.status !== "sending" && eventRaw?.optimistic !== true) {
          return event;
        }
        const settledRaw = { ...(eventRaw ?? {}) };
        delete settledRaw.optimistic;
        changed = true;
        return {
          ...event,
          status: null,
          raw: settledRaw,
        };
      });
      return changed ? next : current;
    });
  }, [updateOptimisticEvents]);

  const settlePendingInteractionsForLifecycle = useCallback((
    lifecycle: Pick<ConversationAttemptLifecycleVm, 'acp'>,
  ) => {
    if (!isTerminalAcpLifecycle(lifecycle)) return;
    setCurrentSession((currentSession) => {
      const base = currentSession ?? latestSessionRef.current;
      const settled = settlePendingAcpInteractionsForLifecycle(base, lifecycle);
      if (settled === base) return currentSession;
      latestSessionRef.current = settled;
      if (settled) storeAcpSession(eventWindowKey, settled);
      return settled;
    });
  }, [eventWindowKey]);

  const applyLifecycleProjection = useCallback((
    incoming: ConversationAttemptLifecycleVm,
    source: "context" | "local" = "local",
  ) => {
    const current = lifecycleProjectionRef.current;
    if (shouldKeepLocalRuntimeLifecycleOverride(current, incoming)) return;
    const merged = mergeConversationAttemptLifecycle(current, incoming);
    lifecycleProjectionRef.current = merged;
    setLocalRuntimeLifecycle(source === "context" && merged === incoming ? null : merged);
    settlePendingInteractionsForLifecycle(merged);
  }, [settlePendingInteractionsForLifecycle]);

  const applyCachedLiveControlFacets = useCallback((
    acp: ConversationAttemptLifecycleVm['acp'],
    promptQueue: ConversationAttemptLifecycleVm['promptQueue'],
  ) => {
    const current = lifecycleProjectionRef.current;
    if (!current) {
      settlePendingInteractionsForLifecycle({ acp });
      return;
    }
    const merged = mergeConversationAttemptLiveControlFacets(current, { acp, promptQueue });
    if (merged !== current) {
      lifecycleProjectionRef.current = merged;
      setLocalRuntimeLifecycle(merged);
    }
    settlePendingInteractionsForLifecycle(merged);
  }, [settlePendingInteractionsForLifecycle]);

  useEffect(() => {
    logAcpSessionReadyLifecycle("component-mount", componentInstanceId, sessionIdentity);
    return () => {
      logAcpSessionReadyLifecycle("component-unmount", componentInstanceId, sessionIdentity);
    };
  }, []);

  useEffect(() => {
    logAcpSessionReadyLifecycle("identity-change", componentInstanceId, sessionIdentity);
  }, [componentInstanceId, sessionIdentity]);

  useEffect(() => {
    onInitialSessionQueryStateChange?.(initialSessionQueryState);
  }, [initialSessionQueryState, onInitialSessionQueryStateChange]);

  useEffect(
    () => subscribeStoredOptimisticEvents(sessionKey, (events) => {
      optimisticEventsRef.current = events;
      setOptimisticEvents(events);
    }),
    [sessionKey],
  );

  useEffect(() => {
    setManualCheckResolved(false);
    setManualCheckSubmitting(false);
    setManualCheckError(null);
    setRuntimeContinueSubmitting(false);
    setRuntimeContinueError(null);
    setLocalRuntimeLifecycle(null);
    lifecycleProjectionRef.current = null;
    setQueueSubmitPending(false);
  }, [attemptId, manualCheckPending, nodeId, roundId, runId, taskId]);

  useEffect(() => {
    const incoming = runtimeComposerContext?.lifecycle;
    if (!incoming) return;
    applyLifecycleProjection(incoming, "context");
  }, [applyLifecycleProjection, runtimeComposerContext?.lifecycle]);

  useLayoutEffect(() => {
    const identityChanged = sessionPropSyncIdentityRef.current !== eventWindowKey;
    sessionPropSyncIdentityRef.current = eventWindowKey;
    const previousCanonicalSession = identityChanged
      ? null
      : latestSessionRef.current;
    if (
      !identityChanged
      && hasOlderAcpTimelineProjection(previousCanonicalSession, session)
    ) {
      return;
    }
    const pendingInteractionNeedsVisibleConvergence = Boolean(
      !identityChanged
      && session
      && !liveUpdatesPausedRef.current
      && !canonicalHeadRecoveryPendingRef.current
      && !hasExplicitHistoricalTimelineIntent()
      && acpSessionHasPendingInteractionSignal(session)
      && acpSessionHasTimelineBeyondLoadedWindow(
        loadedEventWindowRef.current,
        session,
      )
    );
    const preserveVisibleTimeline = !pendingInteractionNeedsVisibleConvergence
      && !identityChanged
      && isHistoricalTimelineWindow()
      && (!session || compareAcpLoadedEventWindowToSession(
        loadedEventWindowRef.current,
        session,
      ) !== "different-session");
    setCurrentSession((previous) => {
      if (!identityChanged && !session && previous) {
        latestSessionRef.current = previous;
        return previous;
      }
      const reconciled = reconcileAcpSessionForDisplay(
        identityChanged ? null : previous,
        session ?? null,
      );
      const next = settlePendingAcpInteractionsForLifecycle(
        reconciled,
        lifecycleProjectionRef.current,
      );
      latestSessionRef.current = next;
      if (next) storeAcpSession(eventWindowKey, next);
      return next;
    });
    if (!session && identityChanged) {
      const restored = restoreAcpLoadedEventWindow(
        eventWindowKey,
        null,
        effectiveLoadedEventBufferLimit,
      );
      const cachedSession = restoreAcpSession(eventWindowKey);
      const cachedViewState = restoreAcpBranchViewState(eventWindowKey);
      commitLoadedEventWindow(eventWindowKey, restored);
      setHasOlderEvents(
        cachedViewState?.hasOlder ?? cachedSession?.eventPage.hasOlder ?? false,
      );
      commitHasNewerEvents(
        cachedViewState?.hasNewer ?? cachedSession?.eventPage.hasNewer ?? false,
      );
      return;
    }
    if (!session) return;
    settleOptimisticPromptAdmissions(session.events);
    if (preserveVisibleTimeline) {
      commitHasNewerEvents(
        hasNewerEventsRef.current
        || acpSessionHasTimelineBeyondLoadedWindow(
          loadedEventWindowRef.current,
          session,
        ),
      );
      return;
    }
    const currentWindow = identityChanged
      ? restoreAcpLoadedEventWindow(
          eventWindowKey,
          session,
          effectiveLoadedEventBufferLimit,
          isStoredAcpHistoricalWindow(restoreAcpBranchViewState(eventWindowKey)),
        )
      : loadedEventWindowRef.current;
    const sameWindowOwner = compareAcpLoadedEventWindowToSession(
      currentWindow,
      session,
    ) === "same";
    const merged = sameWindowOwner
      ? mergeAcpEventWindowsForSession(
          currentWindow.sessionId,
          session.sessionId,
          currentWindow.events,
          session.events,
          alignAcpDisplaySeq,
        )
      : session.events;
    const limited = limitAcpEvents(
      merged,
      "start",
      effectiveLoadedEventBufferLimit,
    );
    setHasOlderEvents(resolveAcpHasOlderEvents(
      session.eventPage.hasOlder,
      merged.length,
      limited.length,
    ));
    commitHasNewerEvents(session.eventPage.hasNewer);
    commitLoadedEventWindow(eventWindowKey, {
      sessionId: session.sessionId ?? null,
      timelineGeneration: acpSessionTimelineGeneration(session),
      events: limited,
    });
  }, [commitHasNewerEvents, commitLoadedEventWindow, effectiveLoadedEventBufferLimit, eventWindowKey, hasExplicitHistoricalTimelineIntent, isHistoricalTimelineWindow, session, settleOptimisticPromptAdmissions]);

  useEffect(() => {
    const identityChanged = sessionResetIdentityRef.current !== eventWindowKey;
    sessionResetIdentityRef.current = eventWindowKey;
    const cachedSession = session ?? restoreAcpSession(eventWindowKey);
    const storedOptimisticEvents = readStoredOptimisticEvents(
      sessionKey,
      effectiveLoadedEventBufferLimit,
    );
    const storedBranchViewState = restoreAcpBranchViewState(eventWindowKey);
    const storedLoadedEventWindow = restoreAcpLoadedEventWindow(
      eventWindowKey,
      cachedSession,
      effectiveLoadedEventBufferLimit,
      isStoredAcpHistoricalWindow(storedBranchViewState),
    );
    const storedPromptEvent = latestSendingOptimisticEvent(
      storedOptimisticEvents,
    );
    setCurrentSession((previous) => {
      if (!identityChanged && !session && previous) {
        latestSessionRef.current = previous;
        return previous;
      }
      const next = settlePendingAcpInteractionsForLifecycle(
        reconcileAcpSessionForDisplay(
          identityChanged ? null : previous,
          cachedSession,
        ),
        lifecycleProjectionRef.current,
      );
      latestSessionRef.current = next;
      if (next) storeAcpSession(eventWindowKey, next);
      return next;
    });
    setInitialSessionQueryState(
      !isTauriRuntime() || hasHydratedAcpSessionContent(eventWindowKey)
        ? "success"
        : "loading",
    );
    setSessionLoadError(null);
    commitLoadedEventWindow(eventWindowKey, storedLoadedEventWindow);
    optimisticEventsRef.current = storedOptimisticEvents;
    setOptimisticEvents(storedOptimisticEvents);
    setDismissedPermissionIds(new Set());
    setPermissionError(null);
    setSendError(null);
    setCancelError(null);
    setCancelling(false);
    setPromptCommandPending(false);
    setStopCommandPending(false);
    setRuntimeStopAccepted(false);
    setAwaitingResponse(Boolean(storedPromptEvent));
    setActiveTurnPrompt(storedPromptEvent?.content?.trim() || null);
    setActiveTurnPromptId(promptIdFromEvent(storedPromptEvent));
    setActiveTurnStartedAt(null);
    setRawPage(null);
    setRawQuery({ page: 0, pageSize: 100, order: "desc" });
    setLoadingOlder(false);
    commitReturnToLatestPending(false);
    setHasOlderEvents(
      storedBranchViewState?.hasOlder ?? cachedSession?.eventPage.hasOlder ?? false,
    );
    commitHasNewerEvents(
      storedBranchViewState?.hasNewer ?? cachedSession?.eventPage.hasNewer ?? false,
    );
    paginationRequestSeqRef.current += 1;
    paginationRequestOwnerRef.current = null;
    paginationDirectionRef.current = null;
    preservingScrollRef.current = false;
    paginationAnchorRef.current = null;
    liveUpdatesDeferredUntilRef.current = 0;
    pendingLiveEventsSinceRef.current = null;
    const restoredViewportAtBottom = storedBranchViewState?.atBottom ?? true;
    viewportAtBottomRef.current = restoredViewportAtBottom;
    viewportManualIntentRef.current = !restoredViewportAtBottom;
    commitShowReturnToLatest(false, "session-reset");
    pendingBranchViewRestoreRef.current = storedBranchViewState;
    cancelRequestedRef.current = false;
    awaitTerminalStopRef.current = false;
    terminalSessionNotifiedRef.current = false;
    sessionRefreshSeqRef.current += 1;
    liveBeforeReadyLogCountRef.current = 0;
    liveAnimationReadyRef.current = false;
    liveStreamingTargetRef.current = null;
    pendingLatestLayoutCommitRef.current = null;
    setStreamingMarkdownItemKey(null);
    setCanvasMode("chat");
  }, [commitHasNewerEvents, commitLoadedEventWindow, commitReturnToLatestPending, commitShowReturnToLatest, effectiveLoadedEventBufferLimit, eventWindowKey, sessionKey]);

  useEffect(() => {
    if (branchId !== 'root' || !branchLiveSnapshot.acp) return;
    applyCachedLiveControlFacets(branchLiveSnapshot.acp, branchLiveSnapshot.promptQueue);
  }, [applyCachedLiveControlFacets, branchId, branchLiveSnapshot.acp, branchLiveSnapshot.promptQueue]);

  useEffect(() => {
    if (loadedEventWindow.eventWindowKey !== eventWindowKey) return;
    storeAcpLoadedEventWindow(
      eventWindowKey,
      {
        sessionId: loadedEventWindow.sessionId,
        timelineGeneration: loadedEventWindow.timelineGeneration,
        events: loadedEventWindow.events,
      },
      effectiveLoadedEventBufferLimit,
    );
  }, [effectiveLoadedEventBufferLimit, eventWindowKey, loadedEventWindow]);

  useEffect(() => () => {
    paginationRequestSeqRef.current += 1;
    paginationRequestOwnerRef.current = null;
    pendingLatestLayoutCommitRef.current = null;
  }, []);

  useLayoutEffect(() => {
    if (pendingLatestLayoutCommitRef.current === null) return;
    if (pendingLatestLayoutCommitRef.current !== sessionIdentity) {
      pendingLatestLayoutCommitRef.current = null;
      return;
    }
    const context = chatContainerContextRef.current;
    const scroller = context?.scrollRef.current;
    if (!context || !scroller) return;
    pendingLatestLayoutCommitRef.current = null;
    alignChatContainerViewportToBottomBeforePaint(scroller);
    void context.scrollToBottom({ animation: "instant" });
  });

  useLayoutEffect(() => () => {
    const scroller = chatContainerContextRef.current?.scrollRef.current;
    if (!scroller) return;
    storeAcpBranchViewState(
      eventWindowKey,
      captureAcpBranchViewState(
        scroller,
        viewportAtBottomRef.current,
        hasOlderEventsRef.current,
        hasNewerEventsRef.current,
      ),
    );
  }, [eventWindowKey]);

  const baseSession = currentSession ?? session;
  const projectionLifecycle = localRuntimeLifecycle ?? runtimeComposerContext?.lifecycle;
  const runtimeActiveFromContext = !runtimeStopAccepted && (runtimeComposerContext?.lifecycle?.runtime.active ?? isRuntimeActiveStatus(runtimeComposerContext?.runtimeStatus));
  const cancelledDirectAttemptShell = shouldCreateCancelledDirectAttemptShell({
    isOrchestrated: runtimeComposerContext?.isOrchestrated ?? true,
    lifecycle: projectionLifecycle,
  });
  const initializationLifecycleActive = Boolean(
    (!runtimeStopAccepted || localRuntimeLifecycle != null)
    && (
      projectionLifecycle?.runtime.active
      || (projectionLifecycle?.acp.liveTurnActivity ?? 'idle') !== 'idle'
      || projectionLifecycle?.acp.stopping
      || projectionLifecycle?.composer.mode === 'runtime-active'
      || projectionLifecycle?.composer.mode === 'stopping'
    ),
  );
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
  const establishedSessionShell = useMemo(
    () =>
      sessionEstablished &&
      !baseSession &&
      !liveSessionShell
        ? createEstablishedAcpSessionShell(
            loadedEvents,
            runtimeActiveFromContext ? "running" : "completed",
            sessionReferenceId,
          )
        : null,
    [
      baseSession,
      liveSessionShell,
      loadedEvents,
      runtimeActiveFromContext,
      sessionEstablished,
      sessionReferenceId,
    ],
  );
  const attemptSessionShell = useMemo(
    () =>
      (initializationLifecycleActive || cancelledDirectAttemptShell) &&
      !baseSession &&
      !liveSessionShell &&
      !establishedSessionShell
        ? createLiveAcpSessionShell(
            loadedEvents,
            cancelledDirectAttemptShell
              ? "cancelled"
              : runtimeActiveFromContext ? "running" : "completed",
          )
        : null,
    [
      baseSession,
      cancelledDirectAttemptShell,
      establishedSessionShell,
      liveSessionShell,
      loadedEvents,
      runtimeActiveFromContext,
      initializationLifecycleActive,
    ],
  );
  const visibleSession = useMemo(
    () =>
      baseSession
        ? createVisibleAcpSession(
            baseSession,
            loadedEvents,
            effectiveLoadedEventBufferLimit,
            isHistoricalTimelineWindow() ? "historical" : "live-head",
          )
        : (liveSessionShell ?? establishedSessionShell ?? attemptSessionShell),
    [
      baseSession,
      effectiveLoadedEventBufferLimit,
      attemptSessionShell,
      liveSessionShell,
      establishedSessionShell,
      hasNewerEvents,
      isHistoricalTimelineWindow,
      loadedEvents,
    ],
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
  const agentCommands = useAgentCommands(
    effective?.provider,
    effective?.providerCwd ?? effective?.cwd,
    effective?.availableCommands,
  );
  const restoreComposerFocus = useCallback(() => {
    restoreSlashCommandInputFocus(composerTextareaRef);
  }, []);
  const slashCommands = useSlashCommandController({
    input: prompt,
    commands: agentCommands.commands,
    contextKey: agentCommands.catalogKey,
    onInputChange: setPrompt,
    onInputFocusRequested: restoreComposerFocus,
  });
  const committedSlashCommand = useMemo(
    () => parseCommittedSlashCommand(prompt, agentCommands.commands),
    [agentCommands.commands, prompt],
  );
  const sessionConfigViewModel = useMemo(
    () => createAcpSessionConfigViewModel(effective?.config, providerCatalog),
    [effective?.config, providerCatalog],
  );
  const effectiveEvents = effective?.events ?? [];
  const effectiveSessionTerminal = isSessionTerminalStatus(effective?.status);
  const localLifecycle = localRuntimeLifecycle ?? runtimeComposerContext?.lifecycle;
  const hasResponseAfterActiveTurn = hasResponseAfterTurn(effectiveEvents, activeTurnStartedAt);
  const activeTurnTerminal = isTerminalLifecycleForTurn(localLifecycle, activeTurnPromptId);
  const activeAwaitingResponse = awaitingResponse && !activeTurnTerminal;
  const waitingForOptimisticPrompt =
    Boolean(pendingOptimisticPrompt) &&
    !hasResponseAfterActiveTurn;
  const localSubmissionPending = promptCommandPending || sending || waitingForOptimisticPrompt;
  const runtimeActive = runtimeActiveFromContext && !(isSessionCompletedStatus(effective?.status ?? baseSession?.status) && !localSubmissionPending);
  const canInferPendingPermission = canInferPendingInteractionFromWindow(
    effective,
    hasNewerEvents,
    "permission",
  );
  const pendingPermissionCandidate =
    effective?.pendingInteractions?.find(
      (request): request is AcpPermissionRequestVm =>
        request.kind === "permission"
        && !dismissedPermissionIds.has(request.interactionId),
    ) ?? (canInferPendingPermission
      ? pendingPermissionFromEvents(effectiveEvents, dismissedPermissionIds)
      : null);
  const hidePendingPermission = shouldHidePendingAcpInteractions(
    localLifecycle,
    activeTurnPromptId,
    cancelling,
    stopCommandPending,
    pendingPermissionCandidate?.turnId,
  );
  const pendingPermission =
    hidePendingPermission
      ? null
      : pendingPermissionCandidate;
  const hidePendingElicitation = shouldHidePendingAcpInteractions(
    localLifecycle,
    activeTurnPromptId,
    cancelling,
    stopCommandPending,
    effective?.pendingInteractions.find(
      (request) => request.kind === "elicitation",
    )?.turnId,
  );
  const pendingElicitationRequest = hidePendingElicitation
    ? null
    : effective?.pendingInteractions.find(
        (request): request is AcpElicitationRequestVm =>
          request.kind === "elicitation"
          && !answeredElicitations.has(request.interactionId),
      );
  const pendingElicitation = pendingElicitationRequest
    ? pendingElicitationFromRequest(pendingElicitationRequest)
    : null;
  const waitingForUserInteraction = Boolean(
    pendingPermission || pendingElicitation,
  );
  const projectedSessionStatus = activityProjectionStatus(
    projectionLifecycle,
    effective?.status,
    localSubmissionPending,
    activeTurnPromptId,
  );
  const timelineProjection = useMemo(
    () => buildAcpTimelineProjection(
      effectiveEvents,
      projectedSessionStatus,
      effective?.timelineProjection,
    ),
    [effective?.timelineProjection, effectiveEvents, projectedSessionStatus],
  );
  const todoEntries = timelineProjection.todoEntries;
  const timeline = useStableAcpTimeline(timelineProjection.timeline);
  const effectiveTimelineGeneration = acpSessionTimelineGeneration(effective);
  const timelineWindowOwner = useMemo<AcpTimelineWindowOwner>(() => ({
    eventWindowKey,
    sessionId: loadedEventWindow.eventWindowKey === eventWindowKey
      ? loadedEventWindow.sessionId
      : (effective?.sessionId ?? null),
    timelineGeneration: loadedEventWindow.eventWindowKey === eventWindowKey
      ? loadedEventWindow.timelineGeneration
      : effectiveTimelineGeneration,
  }), [
    effective?.sessionId,
    effectiveTimelineGeneration,
    eventWindowKey,
    loadedEventWindow.eventWindowKey,
    loadedEventWindow.sessionId,
    loadedEventWindow.timelineGeneration,
  ]);
  const timelineSurfaceState = resolveAcpTimelineSurfaceState({
    hasTimelineItems: timeline.length > 0,
    initialSessionLoading: initialSessionQueryState === "loading",
    runtimeActive: initializationLifecycleActive,
    sending,
  });
  const sessionSnapshotSettled = shouldSettleAcpComposerTransientState(
    localLifecycle,
    effective?.status,
    activeTurnPromptId,
  );
  const acpSessionActive = isSessionActiveStatus(effective?.status)
    && !sessionSnapshotSettled;
  const sessionActive = acpSessionActive || runtimeActive || projectionLifecycle?.acp.liveTurnActivity !== "idle" || promptCommandPending;
  const messageAttachmentLocator = useMemo<MessageAttachmentLocator>(
    () => ({
      projectId,
      taskId,
      runId,
      roundId,
      nodeId,
      attemptId,
      outerNodeId,
      outerAttemptId,
    }),
    [
      attemptId,
      nodeId,
      outerAttemptId,
      outerNodeId,
      projectId,
      roundId,
      runId,
      taskId,
    ],
  );

  const handleOpenMessageAttachment = useCallback(
    (attachment: MessageAttachmentPreview) => {
      if (!rightWorkspace?.scopeKey) return;
      const assetKind = isTaskInputMessageAttachment(attachment)
        ? 'input-attachment' as const
        : 'message-attachment' as const;
      void rightWorkspace.openResource({
        kind: 'conversation-asset',
        key: conversationAssetWorkspaceResourceKey(assetKind, attemptWorkspaceLocator, attachment.name, attachment.path),
        scopeKey: rightWorkspace.scopeKey,
        title: attachment.name,
        description: attachment.path,
        attention: false,
        locator: attemptWorkspaceLocator,
        assetKind,
        name: attachment.name,
        path: attachment.path,
      });
    },
    [attemptWorkspaceLocator, rightWorkspace],
  );


  const showManualCheckActions = manualCheckPending && !manualCheckResolved;
  const showRuntimeContinueAction = Boolean(
    (runtimeComposerContext?.isOrchestrated ?? true)
      && localLifecycle?.continueKind
      && (localLifecycle.continueKind !== 'recover-completed-attempt' || localLifecycle.runtime.revision != null)
      && localLifecycle.runtime.continuable
      && !localLifecycle.runtime.active
      && localLifecycle.acp.liveTurnActivity === "idle",
  );

  const handleOpenComposerAttachment = useCallback((attachment: AttachmentItem) => {
    if (!rightWorkspace?.scopeKey) return;
    void rightWorkspace.openResource(createDraftAttachmentWorkspaceResource({
      scopeKey: rightWorkspace.scopeKey,
      projectId,
      attachment,
    }));
  }, [projectId, rightWorkspace]);

  const closeComposerAttachmentPreview = useCallback((attachment: AttachmentItem) => {
    if (!rightWorkspace?.scopeKey) return;
    void rightWorkspace.closeTab(draftAttachmentWorkspaceResourceKey(rightWorkspace.scopeKey, attachment.id));
  }, [rightWorkspace]);

  const removeComposerAttachment = useCallback((id: string) => {
    const attachment = pendingAttachments.find((item) => item.id === id);
    if (attachment) closeComposerAttachmentPreview(attachment);
    removeAttachment(id);
  }, [closeComposerAttachmentPreview, pendingAttachments, removeAttachment]);

  const clearComposerAttachments = useCallback(() => {
    pendingAttachments.forEach(closeComposerAttachmentPreview);
    clearAttachments();
  }, [clearAttachments, closeComposerAttachmentPreview, pendingAttachments]);
  useEffect(() => {
    if (!shouldSettleRuntimeContinueSubmission(runtimeContinueSubmitting, showRuntimeContinueAction)) return;
    setRuntimeContinueSubmitting(false);
  }, [runtimeContinueSubmitting, showRuntimeContinueAction]);
  const sessionInitializationInterrupted = isAcpSessionInitializationInterrupted({
    orchestrated: runtimeComposerContext?.isOrchestrated ?? true,
    runtimeStatus: localLifecycle?.runtime.status ?? runtimeComposerContext?.runtimeStatus,
    runtimePauseReason: localLifecycle?.runtime.pauseReason,
    runtimeActive: runtimeActiveFromContext,
    sessionId: baseSession?.sessionId,
    sessionEstablished,
    baseSessionReady: isAcpSessionReadyForInitialDisplay(baseSession),
    loadedEventCount: loadedEvents.length,
  });
  const sessionInitializationFailed = isAcpSessionInitializationFailed({
    runtimeStatus: localLifecycle?.runtime.status ?? runtimeComposerContext?.runtimeStatus,
    runtimePauseReason: localLifecycle?.runtime.pauseReason,
    runtimeActive: runtimeActiveFromContext,
    runtimeComposerMode: localLifecycle?.composer.mode,
    runtimeErrorMessage: runtimeComposerContext?.runtimeError,
    sessionId: baseSession?.sessionId,
    sessionEstablished,
    baseSessionReady: isAcpSessionReadyForInitialDisplay(baseSession),
    loadedEventCount: loadedEvents.length,
  });
  const composerLatestEvent = timeline.at(-1) ?? null;
  const turnAccepted = Boolean(activeTurnStartedAt);
  const hasTurnResponse = hasResponseAfterActiveTurn;
  const composerState = deriveAcpRuntimeComposerState({
    lifecycle: localLifecycle,
    promptQueueEnabled: runtimeComposerContext?.promptQueueEnabled,
    workflowValid: runtimeComposerContext?.workflowValid ?? true,
    workflowInvalidMessage: runtimeComposerContext?.workflowError,
    pauseMessage: runtimeComposerContext?.pauseMessage,
    runtimeErrorMessage: runtimeComposerContext?.runtimeError,
    acpStatus: effective?.status,
    prompt,
    hasAttachments: pendingAttachments.length > 0,
    waitingForUserInteraction,
    sending,
    awaitingResponse: activeAwaitingResponse,
    waitingForOptimisticPrompt,
    localTurnId: activeTurnPromptId,
    cancelling,
    stopCommandPending,
    turnAccepted,
    hasResponseAfterTurn: hasTurnResponse,
    hasTimelineItems: timeline.length > 0,
    hasEffectiveEvents: effectiveEvents.length > 0,
    initialTimelinePending: timelineSurfaceState === 'pending',
    timelineProcessingKind: processingKindFromTimeline(composerLatestEvent, false),
  });
  const stopInProgress = composerState.stopInProgress;
  const composerInputDisabled = composerState.inputDisabled;
  const composerSessionSeconds = useSessionTimingSeconds(
    effective?.timing,
    effective?.sessionElapsedSeconds ?? null,
    sessionActive && !effectiveSessionTerminal,
  );
  const composerProcessingKind: AcpProcessingKind = composerState.processingKind;
  const showComposerStatus = composerState.showStatus;
  const composerStatusLabel = processingLabel(t, composerProcessingKind);
  const composerPlaceholder = composerPlaceholderText(composerState, t);
  const supersededBy = localLifecycle?.composer.supersededBy;
  const supersededSession = composerState.mode === 'session-superseded'
    && supersededBy
    && runtimeComposerContext?.supersededSessionNavigation
    ? {
        label: `${supersededBy.nodeId} / ${supersededBy.attemptId}`,
        ...runtimeComposerContext.supersededSessionNavigation,
      }
    : null;
  const canSubmitPrompt = composerState.canSubmit
    && !queueSubmitPending
    && !queueRestorePending;
  const promptQueue = localRuntimeLifecycle?.promptQueue
    ?? runtimeComposerContext?.lifecycle?.promptQueue
    ?? null;
  const promptQueueVisible = Boolean(promptQueue?.items.length);
  const composerDraftOccupied = prompt.length > 0
    || pendingAttachments.length > 0
    || quotes.length > 0;
  const showBranchInfo = Boolean(showBranchControl && projectId);
  const showComposerInfoPanel = showComposerStatus
    || composerSessionSeconds != null
    || Boolean(worktreePath)
    || showBranchInfo
    || hasAcpUsagePanelContent(effective?.usage);
  const composerInfoTabTarget = !showComposerInfoPanel
    ? null
    : !readOnly && showManualCheckActions
      ? "manual"
      : todoEntries.length > 0
        ? "todo"
        : !readOnly && promptQueueVisible && promptQueue
          ? "queue"
          : !readOnly
            ? "composer"
            : null;
  const canStopSession = composerState.canStop;
  const sendButtonBusy = sending || waitingForOptimisticPrompt;
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

  const settleLiveStreamingMarkdown = useCallback(() => {
    recordAcpStreamingDiagnostic("stream-settle", () => ({
      componentInstanceId,
      sessionIdentity,
      previousTarget: liveStreamingTargetRef.current?.key ?? null,
    }));
    liveStreamingTargetRef.current = null;
    setStreamingMarkdownItemKey(null);
  }, [componentInstanceId, sessionIdentity]);

  const observeLiveStreamingEvent = useCallback((update: AcpUiEventVm) => {
    if (update.kind === "timingUpdate") return;
    recordAcpStreamingDiagnostic("animation-readiness", () => ({
      componentInstanceId,
      sessionIdentity,
      ready: liveAnimationReadyRef.current,
      eventKind: update.kind,
      eventId: update.id,
      eventSeq: update.seq,
      eventEndedSeq: update.endedSeq ?? null,
      contentLength: update.content?.length ?? null,
    }));
    if (!liveAnimationReadyRef.current) return;
    const event = normalizeEventUpdate(update);
    if (!event) {
      recordAcpStreamingDiagnostic("target-decision", () => ({
        componentInstanceId,
        sessionIdentity,
        decision: "normalization-rejected",
        eventKind: update.kind,
        eventId: update.id,
      }));
      return;
    }
    const previousTarget = liveStreamingTargetRef.current;
    const target = nextLiveStreamingMarkdownTarget(
      previousTarget,
      event,
      latestUserPromptPosition(loadedEventWindowRef.current.events),
    );
    recordAcpStreamingDiagnostic("target-decision", () => ({
      componentInstanceId,
      sessionIdentity,
      decision: target
        ? target === previousTarget ? "unchanged" : "selected"
        : "settled",
      eventKind: event.kind,
      eventId: event.id,
      eventSeq: event.seq,
      eventEndedSeq: event.endedSeq ?? null,
      previousTarget: previousTarget?.key ?? null,
      nextTarget: target?.key ?? null,
      latestUserPromptPosition: latestUserPromptPosition(
        loadedEventWindowRef.current.events,
      ),
    }));
    if (target === liveStreamingTargetRef.current) return;
    if (!target) {
      settleLiveStreamingMarkdown();
      return;
    }
    liveStreamingTargetRef.current = target;
    setStreamingMarkdownItemKey(target.key);
  }, [normalizeEventUpdate, settleLiveStreamingMarkdown]);

  useEffect(() => {
    if (!sessionActive) settleLiveStreamingMarkdown();
  }, [sessionActive, settleLiveStreamingMarkdown]);

  const applySessionUpdate = useCallback((
    updated: AcpSessionVm | null,
    source = "session-update",
    paginationMode: AcpSessionPaginationUpdateMode = "replace",
  ) => {
    if (sessionIdentityRef.current !== eventWindowKey) return;
    const incoming = normalizeSessionUpdate(updated);
    const pendingInteractionNeedsVisibleConvergence = Boolean(
      incoming
      && !liveUpdatesPausedRef.current
      && !canonicalHeadRecoveryPendingRef.current
      && !hasExplicitHistoricalTimelineIntent()
      && acpSessionHasPendingInteractionSignal(incoming)
      && acpSessionHasTimelineBeyondLoadedWindow(
        loadedEventWindowRef.current,
        incoming,
      )
    );
    const preserveVisibleTimeline = (
      liveUpdatesPausedRef.current
      || canonicalHeadRecoveryPendingRef.current
      || (
        isHistoricalTimelineWindow()
        && !pendingInteractionNeedsVisibleConvergence
      )
    )
      && (!incoming || compareAcpLoadedEventWindowToSession(
        loadedEventWindowRef.current,
        incoming,
      ) !== "different-session");
    const previous = latestSessionRef.current;
    if (hasOlderAcpTimelineProjection(previous, incoming)) return;
    const reconciled = reconcileAcpSessionForDisplay(previous, incoming);
    const paginated = reconciled
      ? {
          ...reconciled,
          eventPage: reconcileAcpEventPageForUpdate(
            previous?.eventPage,
            reconciled.eventPage,
            paginationMode,
          ),
        }
      : null;
    const normalized = settlePendingAcpInteractionsForLifecycle(
      paginated,
      lifecycleProjectionRef.current,
    );
    const refEquivalent = sessionsEquivalent(previous, normalized);
    logAcpSessionReady(source, componentInstanceId, sessionIdentity, normalized, {
      refEquivalent,
      previousReady: previous ? isAcpInitialSessionReady(previous) : false,
      incomingHadTimingRejected: incoming !== normalized,
    });
    latestSessionRef.current = normalized;
    if (normalized) {
      reconcileConversationBranchSession({
        projectId,
        taskId,
        taskUuid,
        runId,
        roundId,
        nodeId,
        attemptId,
        outerNodeId,
        outerAttemptId,
      }, normalized);
      storeAcpSession(eventWindowKey, normalized);
    }
    setCurrentSession((current) =>
      sessionsEquivalent(current, normalized) ? current : normalized,
    );
    if (!normalized) return;
    settleOptimisticPromptAdmissions(normalized.events);
    if (preserveVisibleTimeline) {
      const timelineBeyondVisibleWindow = acpSessionHasTimelineBeyondLoadedWindow(
        loadedEventWindowRef.current,
        normalized,
      );
      commitHasNewerEvents(
        hasNewerEventsRef.current
        || timelineBeyondVisibleWindow,
      );
      if (
        liveUpdatesPausedRef.current
        && (
          timelineBeyondVisibleWindow
          || acpSessionEventsSignature(previous) !== acpSessionEventsSignature(normalized)
        )
      ) {
        markCanonicalHeadRecovery(true);
      }
      return;
    }
    commitHasNewerEvents(normalized.eventPage.hasNewer);
    const currentWindow = loadedEventWindowRef.current;
    const sameWindowOwner = compareAcpLoadedEventWindowToSession(
      currentWindow,
      normalized,
    ) === "same";
    const merged = sameWindowOwner
      ? mergeAcpEventWindowsForSession(
          currentWindow.sessionId,
          normalized.sessionId,
          currentWindow.events,
          normalized.events,
          alignAcpDisplaySeq,
        )
      : normalized.events;
    const limited = limitAcpEvents(
      merged,
      "start",
      effectiveLoadedEventBufferLimit,
    );
    setHasOlderEvents(resolveAcpHasOlderEvents(
      normalized.eventPage.hasOlder,
      merged.length,
      limited.length,
    ));
    commitLoadedEventWindow(eventWindowKey, {
      sessionId: normalized.sessionId ?? null,
      timelineGeneration: acpSessionTimelineGeneration(normalized),
      events: limited,
    });
  }, [attemptId, commitHasNewerEvents, commitLoadedEventWindow, componentInstanceId, effectiveLoadedEventBufferLimit, eventWindowKey, hasExplicitHistoricalTimelineIntent, isHistoricalTimelineWindow, markCanonicalHeadRecovery, nodeId, normalizeSessionUpdate, outerAttemptId, outerNodeId, projectId, roundId, runId, sessionIdentity, settleOptimisticPromptAdmissions, taskId, taskUuid]);

  const refreshSessionAfterConfigUnavailable = useCallback(async (error: unknown) => {
    if (!isAcpSessionConfigValueUnavailableError(error)) return;
    try {
      const updated = await getAcpSession(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        { branchId, pageSize: effectiveEventPageSize, eventLimit: effectiveEventPageSize },
        latestSessionRef.current,
        outerNodeId,
        outerAttemptId,
      );
      if (updated) applySessionUpdate(updated, "config-unavailable-refresh");
    } catch {
      // Preserve the original structured command error if the recovery read also fails.
    }
  }, [applySessionUpdate, attemptId, branchId, effectiveEventPageSize, nodeId, outerAttemptId, outerNodeId, projectId, roundId, runId, taskId]);

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
    projectId,
    roundId,
    runId,
    taskId,
  ]);

  const patchSessionConfig = useCallback((patch: Partial<NonNullable<AcpSessionVm["config"]>>) => {
    const base = latestSessionRef.current;
    if (!base) return null;
    const previousConfig = base.config ? { ...base.config } : null;
    const generation = configMutationGenerationRef.current + 1;
    configMutationGenerationRef.current = generation;
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
    return { generation, patch, previousConfig };
  }, []);

  const rollbackSessionConfig = useCallback((mutation: NonNullable<ReturnType<typeof patchSessionConfig>>) => {
    if (configMutationGenerationRef.current !== mutation.generation) return;
    const base = latestSessionRef.current;
    if (!base) return;
    const config = { ...(base.config ?? {}) };
    const mutableConfig = config as Record<string, unknown>;
    for (const key of Object.keys(mutation.patch)) {
      const previousValue = (mutation.previousConfig as Record<string, unknown> | null)?.[key];
      if (previousValue === undefined) delete mutableConfig[key];
      else mutableConfig[key] = previousValue;
    }
    const updated = { ...base, config };
    latestSessionRef.current = updated;
    setCurrentSession(updated);
  }, []);

  const handleAcpSessionModelChange = useCallback((modelId: string | null) => {
    const config = latestSessionRef.current?.config;
    const selected = modelId ? findAcpConfigOption(
      config?.models,
      config?.configOptions,
      "model",
      modelId,
    ) : null;
    const mutation = patchSessionConfig({
      modelOverrideId: modelId,
      ...(modelId ? { currentModelId: modelId, currentModelName: selected?.name ?? modelId } : {}),
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
        if (mutation) rollbackSessionConfig(mutation);
        setSendError(displayAppError(t, error));
        console.error("Failed to set ACP session model:", error);
      });
  }, [
    applySessionUpdate,
    attemptId,
    nodeId,
    outerAttemptId,
    outerNodeId,
    patchSessionConfig,
    rollbackSessionConfig,
    roundId,
    runId,
    taskId,
    t,
  ]);

  const handleAcpSessionPermissionModeChange = useCallback((permissionModeId: string | null) => {
    const config = latestSessionRef.current?.config;
    const selected = permissionModeId ? findAcpConfigOption(
      config?.modes,
      config?.configOptions,
      "mode",
      permissionModeId,
    ) : null;
    const mutation = patchSessionConfig({
      permissionModeOverrideId: permissionModeId,
      ...(permissionModeId ? { currentModeId: permissionModeId, currentModeName: selected?.name ?? permissionModeId } : {}),
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
        if (mutation) rollbackSessionConfig(mutation);
        setSendError(displayAppError(t, error));
        console.error("Failed to set ACP session permission mode:", error);
      });
  }, [
    applySessionUpdate,
    attemptId,
    nodeId,
    outerAttemptId,
    outerNodeId,
    patchSessionConfig,
    rollbackSessionConfig,
    roundId,
    runId,
    taskId,
    t,
  ]);

  const handleAcpSessionConfigOptionChange = useCallback((optionId: string, optionValue: string | null) => {
    const current = latestSessionRef.current?.config?.configOptionOverrides ?? {};
    const next = { ...current };
    if (optionValue) next[optionId] = optionValue;
    else delete next[optionId];
    const mutation = patchSessionConfig({ configOptionOverrides: next });
    setAcpSessionConfigOption(
      projectId,
      taskId,
      runId,
      roundId,
      nodeId,
      attemptId,
      optionId,
      optionValue,
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
        if (mutation) rollbackSessionConfig(mutation);
        setSendError(displayAppError(t, error));
        console.error("Failed to set ACP session config option:", error);
      });
  }, [applySessionUpdate, attemptId, nodeId, outerAttemptId, outerNodeId, patchSessionConfig, projectId, rollbackSessionConfig, roundId, runId, taskId, t]);

  const applyEventUpdates = useCallback((
    updates: AcpUiEventVm[],
    timelineGeneration: number,
    projectTimeline = true,
  ) => {
    const currentWindow = loadedEventWindowRef.current;
    if (updates.some((event) => (
      compareAcpLoadedEventWindowToLiveEvent(
        currentWindow,
        event,
        timelineGeneration,
      ) !== "same"
    ))) {
      return;
    }
    const normalizedEvents = updates
      .map((event) => normalizeEventUpdate(event))
      .filter((event): event is AcpUiEventVm => Boolean(event));
    if (normalizedEvents.length === 0) return;
    settleOptimisticPromptAdmissions(normalizedEvents);
    const latestTiming = latestLiveSessionTimingFromEvents(normalizedEvents);
    const branchResult = latestAgentBranchResult(normalizedEvents);
    const hasUsageUpdate = normalizedEvents.some((event) => event.kind === "usageUpdate");
    const hasPromptInteractionLifecycleUpdate = normalizedEvents.some(
      (event) => event.kind === "permissionRequest"
        || event.kind === "elicitationRequest"
        || event.kind === "elicitationResponse",
    );
    if (latestTiming || branchResult || hasUsageUpdate || hasPromptInteractionLifecycleUpdate) {
      setCurrentSession((current) => {
        const latest = latestSessionRef.current;
        const base =
          latest && (!current || shouldPreferAcpSessionMetadata(latest, current))
            ? latest
            : (current ?? latest);
        const reconciled = reconcileAcpSessionForDisplay(
          latest,
          projectAcpSessionControlEvents(
            base,
            normalizedEvents,
            lifecycleProjectionRef.current,
          ),
        );
        latestSessionRef.current = reconciled;
        if (reconciled) storeAcpSession(eventWindowKey, reconciled);
        return reconciled;
      });
    }
    const normalizedUpdates = liveTimelineUpdatesFromEvents(normalizedEvents);
    if (normalizedUpdates.length === 0) return;
    if (!projectTimeline) {
      commitHasNewerEvents(true);
      return;
    }
    if (
      hasNewerEventsRef.current
      || paginationDirectionRef.current !== null
      || !viewportAtBottomRef.current
    ) {
      // The visible list is a historical window. The router has already
      // retained this live event for replay, so keep the user's window and
      // anchor intact and expose the existing newer-pagination path.
      commitHasNewerEvents(true);
      return;
    }
    commitHasNewerEvents(false);
    const activeWindow = loadedEventWindowRef.current;
    const merged = mergeAcpEvents(activeWindow.events, normalizedUpdates);
    const limited = limitAcpEvents(
      merged,
      "start",
      effectiveLoadedEventBufferLimit,
    );
    setHasOlderEvents((current) => resolveAcpHasOlderEvents(
      current,
      merged.length,
      limited.length,
    ));
    commitLoadedEventWindow(eventWindowKey, {
      ...activeWindow,
      events: limited,
    });
  }, [commitHasNewerEvents, commitLoadedEventWindow, effectiveLoadedEventBufferLimit, eventWindowKey, normalizeEventUpdate, settleOptimisticPromptAdmissions]);

  const applyEventUpdate = useCallback((
    event: AcpUiEventVm | null | undefined,
    timelineGeneration: number,
  ) => {
    if (!event) return;
    applyEventUpdates([event], timelineGeneration);
  }, [applyEventUpdates]);

  const flushPendingLiveEvents = useCallback(() => {
    if (liveEventFlushTimerRef.current !== null) {
      window.clearTimeout(liveEventFlushTimerRef.current);
      liveEventFlushTimerRef.current = null;
    }
    const scopedUpdates = pendingLiveEventsRef.current.drain();
    pendingLiveEventsSinceRef.current = null;
    if (scopedUpdates.length === 0) return;
    const currentWindow = loadedEventWindowRef.current;
    const applicableUpdates = scopedUpdates.filter((update) => (
      compareAcpLoadedEventWindowToLiveEvent(
        currentWindow,
        update.event,
        update.timelineGeneration,
      ) === "same"
    ));
    if (applicableUpdates.length === 0) return;
    const updates = applicableUpdates.map((update) => update.event);
    const timelineGeneration = applicableUpdates[0].timelineGeneration;
    const { timingUpdates, timelineUpdates } = partitionAcpLiveTimingUpdates(updates);
    if (timingUpdates.length > 0) {
      applyEventUpdates(timingUpdates, timelineGeneration);
    }
    // The timer and latest-wins map are the single flight. Publishing synchronously
    // here prevents React from retaining obsolete cumulative snapshots in transitions.
    if (timelineUpdates.length > 0) {
      applyEventUpdates(timelineUpdates, timelineGeneration);
    }
  }, [applyEventUpdates]);

  const liveFlushDeferRemainingMs = useCallback(() => (
    Math.max(0, liveUpdatesDeferredUntilRef.current - performance.now())
  ), []);

  const liveFlushMaxDeferRemainingMs = useCallback(() => {
    const pendingSince = pendingLiveEventsSinceRef.current;
    if (pendingSince === null) return LIVE_EVENT_MAX_DEFER_MS;
    return Math.max(0, pendingSince + LIVE_EVENT_MAX_DEFER_MS - performance.now());
  }, []);

  const schedulePendingLiveFlush = useCallback((delayMs: number) => {
    if (liveEventFlushTimerRef.current !== null) {
      window.clearTimeout(liveEventFlushTimerRef.current);
      liveEventFlushTimerRef.current = null;
    }

    const schedule = (nextDelayMs: number) => {
      liveEventFlushTimerRef.current = window.setTimeout(() => {
        liveEventFlushTimerRef.current = null;
        if (liveUpdatesPausedRef.current || pendingLiveEventsRef.current.size === 0) return;
        const deferRemainingMs = Math.min(
          liveFlushDeferRemainingMs(),
          liveFlushMaxDeferRemainingMs(),
        );
        if (deferRemainingMs > 0) {
          schedule(deferRemainingMs);
          return;
        }
        flushPendingLiveEvents();
      }, Math.max(0, Math.ceil(nextDelayMs)));
    };

    schedule(delayMs);
  }, [flushPendingLiveEvents, liveFlushDeferRemainingMs, liveFlushMaxDeferRemainingMs]);

  const flushOrSchedulePendingLiveEvents = useCallback((immediate = false) => {
    if (pendingLiveEventsRef.current.size === 0 || liveUpdatesPausedRef.current) return;
    if (immediate) {
      flushPendingLiveEvents();
      return;
    }
    const deferRemainingMs = Math.min(
      liveFlushDeferRemainingMs(),
      liveFlushMaxDeferRemainingMs(),
    );
    if (deferRemainingMs > 0) {
      schedulePendingLiveFlush(deferRemainingMs);
      return;
    }
    flushPendingLiveEvents();
  }, [flushPendingLiveEvents, liveFlushDeferRemainingMs, liveFlushMaxDeferRemainingMs, schedulePendingLiveFlush]);

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

  const handleFollowIntentChange = useCallback((
    following: boolean,
    cause: ChatContainerFollowIntentCause,
  ) => {
    if (following) {
      viewportManualIntentRef.current = false;
      return;
    }
    if (
      cause === "user-wheel-up"
      || cause === "user-key-up"
      || cause === "user-scrollbar-up"
      || cause === "content-expansion-user-scroll"
    ) {
      viewportManualIntentRef.current = true;
    }
  }, []);

  const handleAtBottomChange = useCallback((viewportAtBottom: boolean) => {
    viewportAtBottomRef.current = viewportAtBottom;
    if (!viewportAtBottom && liveStreamingTargetRef.current) {
      settleLiveStreamingMarkdown();
    }
    const scroller = chatContainerContextRef.current?.scrollRef.current;
    const distanceFromBottom = scroller
      ? scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight
      : 0;
    commitShowReturnToLatest(
      shouldShowReturnToLatest(
        showReturnToLatestRef.current,
        viewportAtBottom,
        hasNewerEventsRef.current,
        viewportManualIntentRef.current || hasNewerEventsRef.current,
        distanceFromBottom,
      ),
      "at-bottom-change",
      scroller,
    );
    if (scroller) {
      storeAcpBranchViewState(
        eventWindowKey,
        captureAcpBranchScrollState(
          scroller,
          viewportAtBottom,
          hasOlderEventsRef.current,
          hasNewerEventsRef.current,
        ),
      );
    }
    onAtBottomChange?.(
      isAcpConversationAtBottom(viewportAtBottom, hasNewerEvents),
    );
  }, [commitShowReturnToLatest, eventWindowKey, hasNewerEvents, onAtBottomChange, settleLiveStreamingMarkdown]);

  const requestCanonicalHeadHandoff = useCallback((
    requestedIntent: AcpCanonicalHeadHandoffIntent,
    autoHandoff: boolean,
  ) => {
    if (requestedIntent === "recovery") {
      canonicalHeadRecoveryPendingRef.current = true;
      canonicalHeadRecoveryAutoHandoffRef.current =
        canonicalHeadRecoveryAutoHandoffRef.current || autoHandoff;
      commitHasNewerEvents(true);
    }

    // A loss discovered while any canonical read is in flight must survive
    // that read. Recovery wins over an ordinary trailing handoff because only
    // a successful recovery is allowed to clear the loss gate.
    if (canonicalHeadHandoffInFlightRef.current) {
      canonicalHeadHandoffTrailingIntentRef.current =
        mergeCanonicalHeadHandoffIntent(
          canonicalHeadHandoffTrailingIntentRef.current,
          requestedIntent,
        );
      return;
    }

    const canStart = requestedIntent === "ordinary"
      || canonicalHeadRecoveryAutoHandoffRef.current
      || canonicalHeadHandoffTrailingIntentRef.current !== null;
    if (!canStart) return;

    const intent = mergeCanonicalHeadHandoffIntent(
      canonicalHeadHandoffTrailingIntentRef.current,
      requestedIntent,
    );
    if (liveUpdatesPausedRef.current) {
      canonicalHeadHandoffTrailingIntentRef.current = intent;
      return;
    }

    const startHandoff = (nextIntent: AcpCanonicalHeadHandoffIntent) => {
      const handoff = canonicalHeadHandoffRef.current;
      if (!handoff) {
        canonicalHeadHandoffTrailingIntentRef.current =
          mergeCanonicalHeadHandoffIntent(
            canonicalHeadHandoffTrailingIntentRef.current,
            nextIntent,
          );
        return;
      }
      const ownerKey = sessionIdentityRef.current;
      const ownerEpoch = canonicalHeadHandoffEpochRef.current;
      canonicalHeadHandoffTrailingIntentRef.current = null;
      canonicalHeadHandoffInFlightRef.current = true;
      liveAnimationReadyRef.current = false;
      settleLiveStreamingMarkdown();

      const completeHandoff = (succeeded: boolean) => {
        if (
          sessionIdentityRef.current !== ownerKey
          || canonicalHeadHandoffEpochRef.current !== ownerEpoch
        ) return;
        canonicalHeadHandoffInFlightRef.current = false;

        const trailingIntent = canonicalHeadHandoffTrailingIntentRef.current;
        if (trailingIntent) {
          if (liveUpdatesPausedRef.current) return;
          startHandoff(trailingIntent);
          return;
        }

        commitReturnToLatestPending(false);
        if (!succeeded || nextIntent !== "recovery") return;
        canonicalHeadRecoveryPendingRef.current = false;
        canonicalHeadRecoveryAutoHandoffRef.current = false;
      };

      void handoff(nextIntent).then(
        completeHandoff,
        () => completeHandoff(false),
      );
    };

    startHandoff(intent);
  }, [commitHasNewerEvents, commitReturnToLatestPending, settleLiveStreamingMarkdown]);

  const requestCanonicalHeadRecovery = useCallback((autoHandoff: boolean) => {
    requestCanonicalHeadHandoff("recovery", autoHandoff);
  }, [requestCanonicalHeadHandoff]);

  const enqueueLiveEventUpdate = useCallback(
    (event: AcpUiEventVm, timelineGeneration: number) => {
      const relation = compareAcpLoadedEventWindowToLiveEvent(
        loadedEventWindowRef.current,
        event,
        timelineGeneration,
      );
      if (relation === "different-session") return false;
      const normalizedAdmission = normalizeEventUpdate(event);
      if (normalizedAdmission) {
        settleOptimisticPromptAdmissions([normalizedAdmission]);
      }
      if (relation === "incoming-older") return false;
      if (relation !== "same") {
        const preserveVisibleTimeline = isHistoricalTimelineWindow();
        if (liveEventFlushTimerRef.current !== null) {
          window.clearTimeout(liveEventFlushTimerRef.current);
          liveEventFlushTimerRef.current = null;
        }
        pendingLiveEventsRef.current.clear();
        pendingLiveEventsSinceRef.current = null;
        requestCanonicalHeadRecovery(!preserveVisibleTimeline);
        return false;
      }
      const preserveVisibleTimeline = isHistoricalTimelineWindow();
      if (
        canonicalHeadRecoveryPendingRef.current
        || liveUpdatesPausedRef.current
        || preserveVisibleTimeline
      ) {
        applyEventUpdates([event], timelineGeneration, false);
        if (
          liveUpdatesPausedRef.current
          && !preserveVisibleTimeline
          && liveTimelineUpdatesFromEvents([event]).length > 0
        ) {
          requestCanonicalHeadRecovery(true);
        }
        return false;
      }
      if (event.kind === "timingUpdate") {
        applyEventUpdate(event, timelineGeneration);
        return true;
      }
      const decision = decideAcpLiveEventFlush({
        coalescable: isCoalescableAcpLiveEvent(event),
        paused: liveUpdatesPausedRef.current,
        deferRemainingMs: liveFlushDeferRemainingMs(),
        maxDeferRemainingMs: liveFlushMaxDeferRemainingMs(),
        flushDelayMs: LIVE_EVENT_FLUSH_MS,
        hasScheduledFlush: liveEventFlushTimerRef.current !== null,
      });

      if (decision.applyImmediately) {
        const bufferedToolKey = liveToolEventBufferKey(event);
        const pendingToolUpdate = bufferedToolKey
          ? pendingLiveEventsRef.current.get(bufferedToolKey)
          : null;
        const eventToApply = pendingToolUpdate
          && pendingToolUpdate.timelineGeneration === timelineGeneration
          ? mergeAcpLiveToolEvent(pendingToolUpdate.event, event, mergeRaw)
          : event;
        if (bufferedToolKey) pendingLiveEventsRef.current.delete(bufferedToolKey);
        if (decision.flushPendingBeforeApply) flushPendingLiveEvents();
        applyEventUpdate(eventToApply, timelineGeneration);
        return true;
      }

      if (!decision.buffer) return false;
      if (pendingLiveEventsRef.current.size === 0) {
        pendingLiveEventsSinceRef.current = performance.now();
      }
      const bufferKey = liveEventBufferKey(event);
      const pendingUpdate = pendingLiveEventsRef.current.get(bufferKey);
      const evictedKey = pendingLiveEventsRef.current.replace(
        bufferKey,
        {
          event: pendingUpdate
            && pendingUpdate.timelineGeneration === timelineGeneration
            ? mergeBufferedLiveEvent(pendingUpdate.event, event)
            : event,
          timelineGeneration,
        },
      );
      if (evictedKey !== null) {
        if (liveEventFlushTimerRef.current !== null) {
          window.clearTimeout(liveEventFlushTimerRef.current);
          liveEventFlushTimerRef.current = null;
        }
        pendingLiveEventsRef.current.clear();
        pendingLiveEventsSinceRef.current = null;
        requestCanonicalHeadRecovery(true);
        return false;
      }
      if (decision.scheduleDelayMs !== null) {
        schedulePendingLiveFlush(decision.scheduleDelayMs);
      }
      return true;
    },
    [applyEventUpdate, applyEventUpdates, flushPendingLiveEvents, isHistoricalTimelineWindow, liveFlushDeferRemainingMs, liveFlushMaxDeferRemainingMs, normalizeEventUpdate, requestCanonicalHeadRecovery, schedulePendingLiveFlush, settleOptimisticPromptAdmissions],
  );

  useEffect(() => {
    if (liveUpdatesPaused) {
      if (liveEventFlushTimerRef.current !== null) {
        window.clearTimeout(liveEventFlushTimerRef.current);
        liveEventFlushTimerRef.current = null;
      }
      if (pendingLiveEventsRef.current.size > 0) {
        pendingLiveEventsRef.current.clear();
        pendingLiveEventsSinceRef.current = null;
        requestCanonicalHeadRecovery(true);
      }
      return;
    }
    if (canonicalHeadRecoveryPendingRef.current) {
      requestCanonicalHeadRecovery(
        canonicalHeadRecoveryAutoHandoffRef.current,
      );
      return;
    }
    const trailingIntent = canonicalHeadHandoffTrailingIntentRef.current;
    if (trailingIntent) {
      requestCanonicalHeadHandoff(trailingIntent, true);
      return;
    }
    flushOrSchedulePendingLiveEvents();
  }, [flushOrSchedulePendingLiveEvents, liveUpdatesPaused, requestCanonicalHeadHandoff, requestCanonicalHeadRecovery]);

  useLayoutEffect(() => {
    const scroller = chatContainerContextRef.current?.scrollRef.current;
    if (!scroller) return;
    const anchor = paginationAnchorRef.current;
    if (anchor) {
      paginationAnchorRef.current = null;
      if (applyAcpScrollAnchorCompensation(scroller, anchor.key, anchor.top)) {
        preservingScrollRef.current = true;
        requestAnimationFrame(() => {
          preservingScrollRef.current = false;
        });
      }
    }
  }, [timeline]);

  useLayoutEffect(() => {
    const pending = pendingBranchViewRestoreRef.current;
    const scroller = chatContainerContextRef.current?.scrollRef.current;
    if (!pending || !scroller) return;
    pendingBranchViewRestoreRef.current = null;
    if (pending.atBottom) {
      chatContainerContextRef.current?.scrollToBottom({ animation: "instant" });
      return;
    }
    if (
      pending.anchorKey
      && applyAcpScrollAnchorCompensation(
        scroller,
        pending.anchorKey,
        scroller.getBoundingClientRect().top + pending.anchorOffset,
      )
    ) {
      // The saved real-DOM anchor owns restoration when it is still present.
    } else {
      scroller.scrollTop = pending.scrollTop;
    }
    chatContainerContextRef.current?.stopScroll();
    const distanceFromBottom = scroller.scrollHeight
      - scroller.scrollTop
      - scroller.clientHeight;
    commitShowReturnToLatest(
      shouldShowReturnToLatest(
        showReturnToLatestRef.current,
        false,
        hasNewerEventsRef.current,
        viewportManualIntentRef.current || hasNewerEventsRef.current,
        distanceFromBottom,
      ),
      "branch-view-restore",
      scroller,
    );
  }, [commitShowReturnToLatest, eventWindowKey, timeline]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const renewLease = getRuntimeApi().renewAcpSessionLease;
    if (!renewLease) return;
    let active = true;
    let timer: number | null = null;
    const schedule = (delayMs: number) => {
      if (!active) return;
      timer = window.setTimeout(() => {
        void run();
      }, Math.max(1_000, delayMs));
    };
    const run = async () => {
      try {
        const nextDelayMs = await renewLease(
          projectId,
          taskId,
          runId,
          roundId,
          nodeId,
          attemptId,
          outerNodeId,
          outerAttemptId,
        );
        schedule(nextDelayMs);
      } catch {
        schedule(ACP_SESSION_LEASE_RETRY_MS);
      }
    };
    void run();
    return () => {
      active = false;
      if (timer !== null) window.clearTimeout(timer);
    };
  }, [
    attemptId,
    nodeId,
    outerAttemptId,
    outerNodeId,
    projectId,
    roundId,
    runId,
    taskId,
  ]);

  useEffect(() => {
    if (
      sessionInitializationInterrupted ||
      sessionInitializationFailed ||
      cancelledDirectAttemptShell
    ) {
      setInitialSessionQueryState("success");
      return;
    }
    if (!isTauriRuntime()) {
      setInitialSessionQueryState("success");
      return;
    }
    const restoredHydratedContent = hasHydratedAcpSessionContent(eventWindowKey);
    setInitialSessionQueryState(restoredHydratedContent ? "success" : "loading");
    setSessionLoadError(null);
    let active = true;
    let stopListening: (() => void) | null = null;
    const refreshSeq = sessionRefreshSeqRef.current + 1;
    sessionRefreshSeqRef.current = refreshSeq;
    const effectTraceId = createAcpSessionQueryTraceId(
      componentInstanceId,
      branchId,
      refreshSeq,
    );
    logAcpSessionQueryTiming("effect-start", effectTraceId, sessionIdentity, {
      refreshSeq,
    });
    logAcpSessionReadyLifecycle("effect-start", componentInstanceId, sessionIdentity, {
      refreshSeq,
    });
    const branchLocator = {
      projectId,
      taskId,
      taskUuid,
      runId,
      roundId,
      nodeId,
      attemptId,
      outerNodeId,
      outerAttemptId,
    };
    let retryAttempt = 0;
    let lastLoadError: unknown = null;
    let initialFetchSucceeded = false;
    // Visible/cache state may contain transient Router replay. ACK coverage is
    // rebuilt only from canonical responses observed by this effect.
    let snapshotWatermark = createAcpTimelineWatermark();
    canonicalTimelineCoverageRef.current = {
      eventWindowKey,
      watermark: snapshotWatermark,
    };
    const reconcileCanonicalQueryWatermark = (
      eventPage: AcpSessionVm["eventPage"],
    ) => {
      snapshotWatermark = reconcileAcpTimelineWatermark(
        snapshotWatermark,
        eventPage,
      );
      if (
        active
        && sessionIdentityRef.current === eventWindowKey
      ) {
        canonicalTimelineCoverageRef.current = {
          eventWindowKey,
          watermark: snapshotWatermark,
        };
      }
    };
    let dynamicTerminalContentRefreshRequested = false;
    let branchRefreshInFlight = false;
    let branchRefreshTrailing = false;
    let branchRefreshRequestSeq = 0;

    const requestSelectedBranchRefresh = () => {
      if (
        !active
        || sessionRefreshSeqRef.current !== refreshSeq
        || sessionIdentityRef.current !== eventWindowKey
      ) {
        return;
      }
      if (branchRefreshInFlight) {
        branchRefreshTrailing = true;
        return;
      }
      branchRefreshInFlight = true;
      branchRefreshTrailing = false;
      branchRefreshRequestSeq += 1;
      const requestSeq = branchRefreshRequestSeq;
      const ownerKey = eventWindowKey;
      void getAcpSession(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        { branchId, pageSize: effectiveEventPageSize, eventLimit: effectiveEventPageSize },
        latestSessionRef.current,
        outerNodeId,
        outerAttemptId,
      ).then((updated) => {
        if (
          !updated
          || !active
          || sessionRefreshSeqRef.current !== refreshSeq
          || sessionIdentityRef.current !== ownerKey
          || branchRefreshRequestSeq !== requestSeq
          || branchRefreshTrailing
        ) {
          return;
        }
        applySessionUpdate(updated, 'subscription-branch-refresh');
        reconcileCanonicalQueryWatermark(updated.eventPage);
      }).catch(() => {}).finally(() => {
        branchRefreshInFlight = false;
        if (
          !active
          || sessionRefreshSeqRef.current !== refreshSeq
          || sessionIdentityRef.current !== ownerKey
        ) {
          branchRefreshTrailing = false;
          return;
        }
        if (branchRefreshTrailing) {
          branchRefreshTrailing = false;
          requestSelectedBranchRefresh();
        }
      });
    };

    void (async () => {
      stopListening = subscribeConversationAttemptEvents(branchLocator, (event) => {
        const locatorMatches = conversationEventMatchesAttempt(event, branchLocator);
        recordAcpStreamingDiagnostic("locator-match", () => ({
          componentInstanceId,
          sessionIdentity,
          active,
          matched: locatorMatches,
          mismatches: acpStreamingLocatorMismatches(event, branchLocator),
          ...summarizeAcpStreamingEvent(event),
        }));
        if (!active || !locatorMatches) return;
        if (event.lifecycle) {
          applyLifecycleProjection(event.lifecycle);
        }
        if (event.timelineRecoveryRequired) {
          requestCanonicalHeadRecovery(!isHistoricalTimelineWindow());
        }
        if (event.event) {
          if ((event.branchId ?? 'root') !== branchId) {
            recordAcpStreamingDiagnostic("locator-match", () => ({
              componentInstanceId,
              sessionIdentity,
              active,
              matched: false,
              mismatches: [{
                field: "branchId",
                eventValue: event.branchId ?? "root",
                locatorValue: branchId,
              }],
              ...summarizeAcpStreamingEvent(event),
            }));
            return;
          }
          const latest = latestSessionRef.current;
          if (
            (!latest || !isAcpInitialSessionReady(latest)) &&
            liveBeforeReadyLogCountRef.current < 12
          ) {
            liveBeforeReadyLogCountRef.current += 1;
            logAcpSessionReadyLifecycle("live-event-before-ready", componentInstanceId, sessionIdentity, {
              liveLogIndex: liveBeforeReadyLogCountRef.current,
              eventKind: event.event.kind,
              eventId: event.event.id,
              eventSeq: event.event.seq,
              current: summarizeAcpSessionReady(latest),
            });
          }
          if (!isValidConversationTimelineGeneration(event.timelineGeneration)) {
            requestCanonicalHeadRecovery(!isHistoricalTimelineWindow());
            return;
          }
          const pendingInteractionNeedsCanonicalRecovery =
            initialFetchSucceeded
            && compareAcpLoadedEventWindowToLiveEvent(
              loadedEventWindowRef.current,
              event.event,
              event.timelineGeneration,
            ) === "same"
            && pendingAcpInteractionAdvancesCanonicalTimeline(
              event.event,
              event.timelineGeneration,
              event.timelineRevision,
              canonicalTimelineCoverageRef.current.eventWindowKey === eventWindowKey
                ? canonicalTimelineCoverageRef.current.watermark
                : snapshotWatermark,
            );
          if (pendingInteractionNeedsCanonicalRecovery) {
            requestCanonicalHeadRecovery(!hasExplicitHistoricalTimelineIntent());
          }
          const acceptedForVisibleGeneration = enqueueLiveEventUpdate(
            event.event,
            event.timelineGeneration,
          );
          if (acceptedForVisibleGeneration && !isHistoricalTimelineWindow()) {
            observeLiveStreamingEvent(event.event);
          }
        } else {
          flushOrSchedulePendingLiveEvents(true);
          if (
            shouldRefreshDynamicTerminalSessionContent(event, branchId)
            && !dynamicTerminalContentRefreshRequested
          ) {
            dynamicTerminalContentRefreshRequested = true;
            void getAcpSession(
              projectId,
              taskId,
              runId,
              roundId,
              nodeId,
              attemptId,
              { branchId, pageSize: effectiveEventPageSize, eventLimit: effectiveEventPageSize },
              latestSessionRef.current,
              outerNodeId,
              outerAttemptId,
            ).then((updated) => {
              if (
                !updated
                || !active
                || sessionRefreshSeqRef.current !== refreshSeq
              ) {
                return;
              }
              applySessionUpdate(updated, 'subscription-dynamic-terminal-refresh');
              reconcileCanonicalQueryWatermark(updated.eventPage);
            }).catch(() => {});
            return;
          }
          if (!event.session) return;
          if (branchId !== 'root') {
            // Agent branch envelopes carry root/session metadata rather than the
            // selected branch body. Coalesce bursts into one in-flight read plus
            // one latest trailing read, and fence both responses to this owner.
            requestSelectedBranchRefresh();
            return;
          }
          // Guard against subscription refresh overwriting a pending user config change
          const incoming = event.session;
          logAcpSessionReady("subscription-session:incoming", componentInstanceId, sessionIdentity, incoming ?? null, {
            hasPendingLocalConfigChange: configGenerationRef.current > 0,
          });
          if (configGenerationRef.current > 0 && latestSessionRef.current?.config) {
            const cfg = latestSessionRef.current.config;
            if (incoming.config) {
              incoming.config = {
                ...incoming.config,
                modelOverrideId: cfg.modelOverrideId,
                permissionModeOverrideId: cfg.permissionModeOverrideId,
                configOptionOverrides: cfg.configOptionOverrides,
                currentModelId: cfg.currentModelId,
                currentModelName: cfg.currentModelName,
                currentModeId: cfg.currentModeId,
                currentModeName: cfg.currentModeName,
              };
            }
          }
          if (isSessionTerminalStatus(incoming.status)) {
            settleLiveStreamingMarkdown();
          }
          const current = latestSessionRef.current;
          const incomingGeneration = event.timelineGeneration
            ?? incoming.eventPage.generation;
          const currentGeneration = Math.max(
            snapshotWatermark.generation,
            current?.eventPage.generation ?? 0,
          );
          const sessionIdentityConflicts = Boolean(
            current?.sessionId
            && incoming.sessionId
            && current.sessionId !== incoming.sessionId,
          );
          if (
            incoming.branchId !== "root"
            || sessionIdentityConflicts
            || (incomingGeneration != null && incomingGeneration < currentGeneration)
          ) {
            return;
          }
          applySessionUpdate(incoming, "subscription-session");
          // Subscription session snapshots may advance the visible canonical
          // projection, but only full-head/delta query responses prove replay
          // coverage. Keep their sequence/revision out of the ACK watermark.
          if (isAcpSessionReadyForInitialDisplay(incoming)) {
            initialFetchSucceeded = true;
            lastLoadError = null;
            markAcpSessionContentHydrated(eventWindowKey);
            setSessionLoadError(null);
            setInitialSessionQueryState("success");
          }
        }
      });
      await ensureConversationEventRouterStarted();
      if (!active || sessionRefreshSeqRef.current !== refreshSeq) return;
      logAcpSessionReadyLifecycle("subscription-listening", componentInstanceId, sessionIdentity, {
        refreshSeq,
      });
      while (active && sessionRefreshSeqRef.current === refreshSeq) {
        const requestTraceId = `${effectTraceId}:request-${retryAttempt + 1}`;
        const requestStartedAt = performance.now();
        logAcpSessionQueryTiming("request-start", requestTraceId, sessionIdentity, {
          retryAttempt,
          refreshSeq,
        });
        try {
          const updated = await getAcpSession(
            projectId,
            taskId,
            runId,
            roundId,
            nodeId,
            attemptId,
            {
              ...(isAcpSessionQueryTimingDebugEnabled()
                ? { traceId: requestTraceId }
                : {}),
              branchId,
              pageSize: effectiveEventPageSize,
              eventLimit: effectiveEventPageSize,
            },
            latestSessionRef.current,
            outerNodeId,
            outerAttemptId,
          );
          logAcpSessionQueryTiming("request-complete", requestTraceId, sessionIdentity, {
            retryAttempt,
            refreshSeq,
            elapsedMs: Math.round(performance.now() - requestStartedAt),
            returnedEventCount: updated?.events.length ?? 0,
            projectedAgentCount: updated?.timelineProjection?.agents.length ?? 0,
          });
          if (!active || sessionRefreshSeqRef.current !== refreshSeq) break;
          logAcpSessionReady("initial-fetch:response", componentInstanceId, sessionIdentity, updated, {
            retryAttempt,
            refreshSeq,
            active,
            currentRefreshSeq: sessionRefreshSeqRef.current,
          });
          if (updated && active && sessionRefreshSeqRef.current === refreshSeq) {
            const updatedGeneration = updated.eventPage.generation;
            if (
              (initialFetchSucceeded && !isAcpSessionReadyForInitialDisplay(updated))
              || (
                updatedGeneration != null
                && updatedGeneration < snapshotWatermark.generation
              )
            ) {
              break;
            }
            lastLoadError = null;
            setSessionLoadError(null);
            applySessionUpdate(updated, "initial-fetch");
            reconcileCanonicalQueryWatermark(updated.eventPage);
            if (isAcpSessionReadyForInitialDisplay(updated)) {
              markAcpSessionContentHydrated(eventWindowKey);
              initialFetchSucceeded = true;
              setInitialSessionQueryState("success");
              break;
            }
          }
        } catch (error) {
          logAcpSessionQueryTiming("request-error", requestTraceId, sessionIdentity, {
            retryAttempt,
            refreshSeq,
            elapsedMs: Math.round(performance.now() - requestStartedAt),
            error: String(error),
          });
          if (!active || sessionRefreshSeqRef.current !== refreshSeq) break;
          lastLoadError = error;
          // provider resolution / IO error — retry may not help but we try once more
        }
        const delay = missingAcpSessionRetryDelay(retryAttempt);
        if (delay === null) break;
        retryAttempt += 1;
        await new Promise((resolve) => setTimeout(resolve, delay));
      }
      if (active && sessionRefreshSeqRef.current === refreshSeq) {
        if (!initialFetchSucceeded) {
          if (restoredHydratedContent) {
            setInitialSessionQueryState("success");
            return;
          }
          setSessionLoadError(
            lastLoadError
              ? displayAppError(t, lastLoadError)
              : t("acp.missingSessionReason"),
          );
          setInitialSessionQueryState("error");
          return;
        }
        const replayCut = readConversationBranchReplaySnapshot(branchLocator, branchId);
        const requiredLossWatermarkGeneration = replayCut.lossWatermarkGeneration;
        const requiredLossWatermarkRevision = replayCut.lossWatermarkRevision;
        const requiredLossWatermarkSeq = replayCut.lossWatermarkSeq;
        if (isHistoricalTimelineWindow()) {
          if (replayCut.events.length > 0 || replayCut.requiresCatchUp) {
            commitHasNewerEvents(true);
          }
          liveAnimationReadyRef.current = false;
          return;
        }
        const requiredTimelineGeneration = replayCut.timelineGeneration
          || requiredLossWatermarkGeneration;
        const recoveryDeadlineAt = performance.now() + ACP_REPLAY_CATCH_UP_MAX_MS;
        let replayAcknowledged = false;
        let catchUpCandidateSession = latestSessionRef.current;
        const currentSessionId = () => catchUpCandidateSession?.sessionId
          ?? latestSessionRef.current?.sessionId
          ?? null;

        if (
          replayCut.sessionId !== null
          && replayCut.sessionId !== currentSessionId()
        ) {
          commitHasNewerEvents(true);
        } else {
          const refreshCanonicalSnapshot = async (source: string) => {
            const refreshResponse = await awaitAcpReplayCatchUpRequest(getAcpSession(
              projectId,
              taskId,
              runId,
              roundId,
              nodeId,
              attemptId,
              {
                branchId,
                pageSize: effectiveEventPageSize,
                eventLimit: effectiveEventPageSize,
              },
              catchUpCandidateSession ?? latestSessionRef.current,
              outerNodeId,
              outerAttemptId,
            ), recoveryDeadlineAt);
            if (
              !refreshResponse
              || !active
              || sessionRefreshSeqRef.current !== refreshSeq
              || sessionIdentityRef.current !== eventWindowKey
            ) return false;
            const refreshed = reconcileCanonicalAcpSessionForDisplay(
              latestSessionRef.current,
              refreshResponse,
            );
            applySessionUpdate(refreshed, source);
            catchUpCandidateSession = refreshed;
            reconcileCanonicalQueryWatermark(refreshResponse.eventPage);
            return true;
          };

          if (snapshotWatermark.generation < requiredTimelineGeneration) {
            await refreshCanonicalSnapshot("replay-generation-refresh");
          }

          const newerGenerationCoversLoss = Boolean(
            requiredLossWatermarkGeneration > 0
            && snapshotWatermark.generation > requiredLossWatermarkGeneration,
          );
          const requiresRevisionCatchUp = replayCut.requiresCatchUp
            && !newerGenerationCoversLoss
            && (
              requiredLossWatermarkGeneration === snapshotWatermark.generation
              || (
                requiredLossWatermarkGeneration === 0
                && snapshotWatermark.generation === 0
              )
            )
            && snapshotWatermark.coveredRevision < requiredLossWatermarkRevision;
          let catchUpRequestCount = 0;
          let catchUpRetryAttempt = 0;
          while (
            active
            && sessionRefreshSeqRef.current === refreshSeq
            && sessionIdentityRef.current === eventWindowKey
            && requiresRevisionCatchUp
            && snapshotWatermark.coveredRevision < requiredLossWatermarkRevision
            && catchUpRequestCount < ACP_REPLAY_CATCH_UP_MAX_PAGES
            && performance.now() < recoveryDeadlineAt
          ) {
            catchUpRequestCount += 1;
            const delta = await awaitAcpReplayCatchUpRequest(getAcpSession(
              projectId,
              taskId,
              runId,
              roundId,
              nodeId,
              attemptId,
              {
                branchId,
                afterRevision: snapshotWatermark.coveredRevision,
                pageSize: effectiveEventPageSize,
                eventLimit: effectiveEventPageSize,
              },
              catchUpCandidateSession,
              outerNodeId,
              outerAttemptId,
            ), recoveryDeadlineAt);
            if (!delta) {
              const delay = missingAcpSessionRetryDelay(catchUpRetryAttempt);
              if (delay === null || performance.now() + delay >= recoveryDeadlineAt) {
                break;
              }
              catchUpRetryAttempt += 1;
              await new Promise((resolve) => window.setTimeout(resolve, delay));
              continue;
            }
            catchUpRetryAttempt = 0;
            const deltaGeneration = delta.eventPage.generation
              ?? snapshotWatermark.generation;
            if (deltaGeneration !== snapshotWatermark.generation) break;
            const nextRevision = delta.eventPage.newestRevision
              ?? delta.eventPage.coveredRevision
              ?? snapshotWatermark.coveredRevision;
            if (nextRevision <= snapshotWatermark.coveredRevision) break;
            const reconciledDelta = reconcileAcpSessionForDisplay(
              catchUpCandidateSession,
              delta,
            ) ?? delta;
            const mergedDeltaEvents = limitAcpEvents(
              mergeAcpEvents(
                catchUpCandidateSession?.events ?? [],
                delta.events,
              ),
              "start",
              effectiveLoadedEventBufferLimit,
            );
            catchUpCandidateSession = {
              ...reconciledDelta,
              events: mergedDeltaEvents,
              eventPage: reconcileAcpEventPageForUpdate(
                catchUpCandidateSession?.eventPage,
                delta.eventPage,
                "append-newer",
              ),
            };
            reconcileCanonicalQueryWatermark({
              ...delta.eventPage,
              generation: snapshotWatermark.generation,
              coveredRevision: nextRevision,
            });
          }

          let sequenceRetryAttempt = 0;
          while (
            active
            && sessionRefreshSeqRef.current === refreshSeq
            && sessionIdentityRef.current === eventWindowKey
            && !acpTimelineWatermarkCoversSequenceLoss(
              snapshotWatermark,
              requiredLossWatermarkGeneration,
              requiredLossWatermarkSeq,
            )
            && catchUpRequestCount < ACP_REPLAY_CATCH_UP_MAX_PAGES
            && performance.now() < recoveryDeadlineAt
          ) {
            const delay = missingAcpSessionRetryDelay(sequenceRetryAttempt);
            if (delay === null || performance.now() + delay >= recoveryDeadlineAt) {
              break;
            }
            sequenceRetryAttempt += 1;
            await new Promise((resolve) => window.setTimeout(resolve, delay));
            catchUpRequestCount += 1;
            await refreshCanonicalSnapshot("replay-sequence-catch-up");
          }

          const caughtUpFixedCut = snapshotWatermark.generation >= requiredTimelineGeneration
            && (
              !requiresRevisionCatchUp
              || snapshotWatermark.coveredRevision >= requiredLossWatermarkRevision
            )
            && acpTimelineWatermarkCoversSequenceLoss(
              snapshotWatermark,
              requiredLossWatermarkGeneration,
              requiredLossWatermarkSeq,
            );
          if (caughtUpFixedCut) {
            const cutAcknowledged = acknowledgeConversationBranchReplay(
              branchLocator,
              branchId,
              currentSessionId(),
              snapshotWatermark.generation,
              snapshotWatermark.coveredRevision,
              snapshotWatermark.coveredSeq,
              replayCut.generation,
            );
            const finalReplayCut = readConversationBranchReplaySnapshot(
              branchLocator,
              branchId,
            );
            const finalCutHasReplay = finalReplayCut.events.length > 0
              || finalReplayCut.requiresCatchUp;
            const finalCutSafe = cutAcknowledged
              && (
                !finalCutHasReplay
                || finalReplayCut.sessionId === currentSessionId()
              )
              && (
                !finalCutHasReplay
                || finalReplayCut.timelineGeneration === snapshotWatermark.generation
                || (
                  finalReplayCut.timelineGeneration === 0
                  && snapshotWatermark.generation === 0
                )
              )
              && finalReplayCut.lossWatermarkRouterGeneration
                <= replayCut.generation;
            if (finalCutSafe) {
              replayAcknowledged = acknowledgeConversationBranchReplay(
                branchLocator,
                branchId,
                currentSessionId(),
                snapshotWatermark.generation,
                snapshotWatermark.coveredRevision,
                snapshotWatermark.coveredSeq,
                finalReplayCut.generation,
              );
              if (replayAcknowledged) {
                if (catchUpCandidateSession) {
                  applySessionUpdate(
                    catchUpCandidateSession,
                    "replay-gap-catch-up",
                  );
                }
                const replayBelongsToSnapshot =
                  replayCut.timelineGeneration === snapshotWatermark.generation
                  || (
                    replayCut.timelineGeneration === 0
                    && snapshotWatermark.generation === 0
                  );
                const replayEvents = replayBelongsToSnapshot
                  ? mergeAcpEvents(replayCut.events, finalReplayCut.events)
                  : finalReplayCut.events;
                if (replayEvents.length > 0) {
                  applyEventUpdates(replayEvents, snapshotWatermark.generation);
                }
              }
            }
          }
        }

        const remainingReplay = readConversationBranchReplaySnapshot(
          branchLocator,
          branchId,
        );
        const remainingHasState = remainingReplay.events.length > 0
          || remainingReplay.requiresCatchUp;
        const remainingTimelineMatches = !remainingHasState
          || remainingReplay.timelineGeneration === snapshotWatermark.generation
          || (
            remainingReplay.timelineGeneration === 0
            && snapshotWatermark.generation === 0
          );
        const remainingSessionMatches = !remainingHasState
          || remainingReplay.sessionId === currentSessionId();
        let hasRemainingReplay = remainingReplay.requiresCatchUp
          || !remainingTimelineMatches
          || !remainingSessionMatches;
        if (
          !hasRemainingReplay
          && remainingReplay.events.length > 0
        ) {
          // These events have already passed through the keyed live listener
          // into the page's latest-wins buffer. Prefix-ACK them without another
          // DOM projection; a concurrent newer prefix remains retained.
          hasRemainingReplay = !acknowledgeConversationBranchReplay(
            branchLocator,
            branchId,
            currentSessionId(),
            snapshotWatermark.generation,
            snapshotWatermark.coveredRevision,
            snapshotWatermark.coveredSeq,
            remainingReplay.generation,
          );
        }
        commitHasNewerEvents(hasRemainingReplay || !replayAcknowledged);
        if (!replayAcknowledged) {
          recordAcpStreamingDiagnostic("animation-readiness", () => ({
            componentInstanceId,
            sessionIdentity,
            ready: false,
            reason: "replay-catch-up-pending",
            snapshotCoveredRevision: snapshotWatermark.coveredRevision,
            snapshotGeneration: snapshotWatermark.generation,
            requiredLossWatermarkGeneration,
            requiredLossWatermarkRevision,
            replayGeneration: replayCut.generation,
          }));
          return;
        }
        liveAnimationReadyRef.current = !hasRemainingReplay;
        recordAcpStreamingDiagnostic("animation-readiness", () => ({
          componentInstanceId,
          sessionIdentity,
          ready: true,
          reason: "fixed-loss-watermark-covered",
          snapshotCoveredRevision: snapshotWatermark.coveredRevision,
          snapshotGeneration: snapshotWatermark.generation,
          requiredLossWatermarkGeneration,
          requiredLossWatermarkRevision,
          replayGeneration: replayCut.generation,
        }));
      }
    })();
    return () => {
      logAcpSessionQueryTiming("effect-cleanup", effectTraceId, sessionIdentity, {
        refreshSeq,
      });
      logAcpSessionReadyLifecycle("effect-cleanup", componentInstanceId, sessionIdentity, {
        refreshSeq,
        hadStopListening: Boolean(stopListening),
      });
      flushPendingLiveEvents();
      active = false;
      stopListening?.();
      if (liveEventFlushTimerRef.current !== null) {
        window.clearTimeout(liveEventFlushTimerRef.current);
        liveEventFlushTimerRef.current = null;
      }
      pendingLiveEventsRef.current.clear();
      pendingLiveEventsSinceRef.current = null;
    };
  }, [
    applySessionUpdate,
    applyEventUpdates,
    attemptId,
    branchId,
    cancelledDirectAttemptShell,
    enqueueLiveEventUpdate,
    eventWindowKey,
    effectiveEventPageSize,
    flushPendingLiveEvents,
    flushOrSchedulePendingLiveEvents,
    hasExplicitHistoricalTimelineIntent,
    isHistoricalTimelineWindow,
    nodeId,
    observeLiveStreamingEvent,
    outerAttemptId,
    outerNodeId,
    roundId,
    runId,
    settleLiveStreamingMarkdown,
    sessionInitializationFailed,
    sessionInitializationInterrupted,
    taskId,
    t,
  ]);

  useEffect(() => {
    if (runtimeStopAccepted && !(runtimeComposerContext?.lifecycle?.runtime.active ?? isRuntimeActiveStatus(runtimeComposerContext?.runtimeStatus))) {
      setRuntimeStopAccepted(false);
    }
  }, [runtimeComposerContext?.lifecycle?.runtime.active, runtimeComposerContext?.runtimeStatus, runtimeStopAccepted]);

  useEffect(() => {
    if (!activeTurnTerminal || !activeTurnPromptId) return;
    const terminalTurnId = activeTurnPromptId;
    setSending(false);
    setPromptCommandPending(false);
    setAwaitingResponse(false);
    setCancelling(false);
    setActiveTurnPrompt(null);
    setActiveTurnPromptId(null);
    setActiveTurnStartedAt(null);
    awaitTerminalStopRef.current = false;
    updateOptimisticEvents((current) => current.filter(
      (event) => (
        promptIdFromEvent(event) !== terminalTurnId
        || !isPendingOptimisticPrompt(event)
      ),
    ));
    const shouldNotifyStopped = cancelRequestedRef.current;
    cancelRequestedRef.current = false;
    if (shouldNotifyStopped) onSessionStopped?.();
  }, [activeTurnPromptId, activeTurnTerminal, onSessionStopped]);

  useEffect(() => {
    const terminalSession = shouldSettleAcpComposerTransientState(
      localLifecycle,
      effective?.status,
      activeTurnPromptId,
    );
    if (stopCommandPending || sending || waitingForOptimisticPrompt) {
      return;
    }
    if (promptCommandPending && !cancelling) {
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
    activeTurnPromptId,
    awaitingResponse,
    cancelling,
    effective?.status,
    localLifecycle,
    onSessionStopped,
    promptCommandPending,
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
    if (acceptedPrompt) {
      if (!activeTurnStartedAt) setActiveTurnStartedAt(acceptedPrompt.timestamp);
      if (sending) setSending(false);
    }
    updateOptimisticEvents((current) => {
      const next = current.filter((event) =>
        shouldMergeOptimisticEvent(loadedEvents, event),
      );
      return next.length === current.length ? current : next;
    });
  }, [activeTurnPrompt, activeTurnPromptId, activeTurnStartedAt, loadedEvents, sending]);

  const preserveScrollPosition = useCallback(() => {}, []);

  const rejoinLatestHead = useCallback(async (
    headSession: AcpSessionVm,
    seedEvents: AcpUiEventVm[] = headSession.events,
    requestToken: AcpPaginationRequestToken,
    intent: AcpCanonicalHeadHandoffIntent,
    discardReplayThroughCut: ReturnType<
      typeof readConversationBranchReplaySnapshot
    > | null = null,
  ) => {
    if (!ownsPaginationWindowRequest(requestToken)) return false;
    liveAnimationReadyRef.current = false;
    settleLiveStreamingMarkdown();

    const observedDisplaySession = latestSessionRef.current;
    const preferObservedCanonicalSession = Boolean(
      observedDisplaySession
      && shouldPreferObservedAcpSessionOverCanonicalResponse(
        headSession,
        observedDisplaySession,
      ),
    );
    let candidateSession = reconcileCanonicalAcpSessionForDisplay(
      observedDisplaySession,
      headSession,
    );
    const candidateSourceEvents = preferObservedCanonicalSession
      ? observedDisplaySession!.events
      : seedEvents;
    let candidateEvents = limitAcpEvents(
      candidateSourceEvents,
      "start",
      effectiveLoadedEventBufferLimit,
    );
    let candidateWindowTrimmed = candidateEvents.length < candidateSourceEvents.length;
    let candidatePage = preferObservedCanonicalSession
      ? observedDisplaySession!.eventPage
      : headSession.eventPage;
    let canonicalWatermark = createAcpTimelineWatermark(headSession.eventPage);
    let candidateGeneration = canonicalWatermark.generation;
    let caughtThroughRevision = canonicalWatermark.coveredRevision;
    let canonicalSessionId = headSession.sessionId ?? null;
    let generationRefreshAttempted = false;
    const canonicalOwnerWasOvertaken = () => {
      const observed = latestSessionRef.current;
      if (!observed) return false;
      const observedSessionId = observed.sessionId?.trim() || null;
      if (
        canonicalSessionId !== null
        && observedSessionId !== null
        && observedSessionId !== canonicalSessionId
      ) {
        return true;
      }
      return (observed.eventPage.generation ?? 0) > canonicalWatermark.generation;
    };

    if (canonicalOwnerWasOvertaken()) {
      requestCanonicalHeadHandoff(intent, true);
      return false;
    }

    const adoptCandidateSession = (
      preferredSession: AcpSessionVm,
      sourceEvents: AcpUiEventVm[] = preferredSession.events,
      resetTrimmedWindow = false,
    ) => {
      candidateSession = preferredSession;
      candidateEvents = limitAcpEvents(
        sourceEvents,
        "start",
        effectiveLoadedEventBufferLimit,
      );
      const sourceWindowTrimmed = candidateEvents.length < sourceEvents.length;
      candidateWindowTrimmed = resetTrimmedWindow
        ? sourceWindowTrimmed
        : candidateWindowTrimmed || sourceWindowTrimmed;
    };

    const refreshCanonicalHead = async (deadlineAt: number) => {
      const response = await awaitAcpReplayCatchUpRequest(getAcpSession(
          projectId,
          taskId,
          runId,
          roundId,
          nodeId,
          attemptId,
          {
            branchId,
            pageSize: effectiveEventPageSize,
            eventLimit: effectiveEventPageSize,
          },
          latestSessionRef.current,
          outerNodeId,
          outerAttemptId,
        ), deadlineAt);
      if (!ownsPaginationWindowRequest(requestToken)) return false;
      const refreshed = normalizeSessionUpdate(response);
      if (!refreshed) return false;
      const refreshedGeneration = refreshed.eventPage.generation
        ?? canonicalWatermark.generation;
      if (refreshedGeneration < canonicalWatermark.generation) return false;
      if (
        canonicalSessionId !== null
        && refreshed.sessionId != null
        && refreshed.sessionId !== canonicalSessionId
      ) return false;
      const previousGeneration = canonicalWatermark.generation;
      canonicalWatermark = reconcileAcpTimelineWatermark(
        canonicalWatermark,
        refreshed.eventPage,
      );
      candidateGeneration = canonicalWatermark.generation;
      caughtThroughRevision = canonicalWatermark.coveredRevision;
      canonicalSessionId = refreshed.sessionId ?? canonicalSessionId;
      const latestObservedSession = latestSessionRef.current;
      const preferLatestCanonicalSession = Boolean(
        latestObservedSession
        && shouldPreferObservedAcpSessionOverCanonicalResponse(
          refreshed,
          latestObservedSession,
        ),
      );
      const preferredSession = reconcileCanonicalAcpSessionForDisplay(
        latestObservedSession,
        refreshed,
      );
      candidatePage = preferLatestCanonicalSession
        ? latestObservedSession!.eventPage
        : refreshed.eventPage;
      adoptCandidateSession(
        preferredSession,
        preferLatestCanonicalSession
          ? latestObservedSession!.events
          : refreshed.events,
        canonicalWatermark.generation !== previousGeneration,
      );
      return true;
    };

    if (!ownsPaginationWindowRequest(requestToken)) return false;
    // C0 is immutable: recovery covers only the loss observed at this cut and
    // never chases a Router head that keeps advancing during awaited I/O.
    const replayCut = discardReplayThroughCut
      ?? readConversationBranchReplaySnapshot(
        attemptWorkspaceLocator,
        branchId,
      );
    const recoveryDeadlineAt = performance.now() + ACP_REPLAY_CATCH_UP_MAX_MS;
    const replayCutTimelineGeneration = replayCut.timelineGeneration
      || replayCut.lossWatermarkGeneration;
    const candidateSessionId = () => canonicalSessionId;
    if (
      replayCut.sessionId !== null
      && replayCut.sessionId !== candidateSessionId()
    ) {
      return false;
    }
    if (
      replayCutTimelineGeneration > 0
      && replayCutTimelineGeneration > candidateGeneration
    ) {
      if (generationRefreshAttempted) return false;
      generationRefreshAttempted = true;
      if (!await refreshCanonicalHead(recoveryDeadlineAt)) return false;
      if (candidateGeneration < replayCutTimelineGeneration) return false;
    }

    const newerGenerationCoversLoss = Boolean(
      replayCut.lossWatermarkGeneration > 0
      && candidateGeneration > replayCut.lossWatermarkGeneration,
    );
    const fixedLossWatermarkRevision = replayCut.lossWatermarkRevision;
    const requiresRevisionCatchUp = replayCut.requiresCatchUp
      && !newerGenerationCoversLoss
      && (
        replayCut.lossWatermarkGeneration === candidateGeneration
        || (
          replayCut.lossWatermarkGeneration === 0
          && candidateGeneration === 0
        )
      )
      && caughtThroughRevision < fixedLossWatermarkRevision;
    let catchUpPageCount = 0;
    while (
      requiresRevisionCatchUp
      && caughtThroughRevision < fixedLossWatermarkRevision
      && catchUpPageCount < ACP_REPLAY_CATCH_UP_MAX_PAGES
      && performance.now() < recoveryDeadlineAt
    ) {
      catchUpPageCount += 1;
      const deltaResponse = await awaitAcpReplayCatchUpRequest(getAcpSession(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        {
          branchId,
          afterRevision: caughtThroughRevision,
          pageSize: effectiveEventPageSize,
          eventLimit: effectiveEventPageSize,
        },
        candidateSession,
        outerNodeId,
        outerAttemptId,
      ), recoveryDeadlineAt);
      if (!ownsPaginationWindowRequest(requestToken)) return false;
      const delta = normalizeSessionUpdate(deltaResponse);
      if (!delta) return false;
      const deltaGeneration = delta.eventPage.generation ?? candidateGeneration;
      if (deltaGeneration !== candidateGeneration) return false;
      const nextRevision = delta.eventPage.newestRevision
        ?? delta.eventPage.coveredRevision
        ?? caughtThroughRevision;
      if (nextRevision <= caughtThroughRevision) return false;
      candidateSession = reconcileAcpSessionForDisplay(
        candidateSession,
        delta,
      ) ?? candidateSession;
      candidateSession = projectAcpSessionControlEvents(
        candidateSession,
        delta.events,
        lifecycleProjectionRef.current,
      ) ?? candidateSession;
      const mergedDeltaEvents = mergeAcpEvents(candidateEvents, delta.events);
      candidateEvents = limitAcpEvents(
        mergedDeltaEvents,
        "start",
        effectiveLoadedEventBufferLimit,
      );
      candidateWindowTrimmed = candidateWindowTrimmed
        || candidateEvents.length < mergedDeltaEvents.length;
      candidatePage = reconcileAcpEventPageForUpdate(
        candidatePage,
        delta.eventPage,
        "append-newer",
      );
      canonicalWatermark = reconcileAcpTimelineWatermark(
        canonicalWatermark,
        {
          ...delta.eventPage,
          generation: candidateGeneration,
          coveredRevision: nextRevision,
        },
      );
      candidateGeneration = canonicalWatermark.generation;
      caughtThroughRevision = canonicalWatermark.coveredRevision;
    }
    if (
      requiresRevisionCatchUp
      && caughtThroughRevision < fixedLossWatermarkRevision
    ) {
      return false;
    }

    let sequenceRetryAttempt = 0;
    while (
      !acpTimelineWatermarkCoversSequenceLoss(
        canonicalWatermark,
        replayCut.lossWatermarkGeneration,
        replayCut.lossWatermarkSeq,
      )
      && catchUpPageCount < ACP_REPLAY_CATCH_UP_MAX_PAGES
      && performance.now() < recoveryDeadlineAt
    ) {
      const delay = missingAcpSessionRetryDelay(sequenceRetryAttempt);
      if (delay === null || performance.now() + delay >= recoveryDeadlineAt) {
        break;
      }
      sequenceRetryAttempt += 1;
      await new Promise((resolve) => window.setTimeout(resolve, delay));
      catchUpPageCount += 1;
      await refreshCanonicalHead(recoveryDeadlineAt);
      if (!ownsPaginationWindowRequest(requestToken)) return false;
    }
    if (!acpTimelineWatermarkCoversSequenceLoss(
      canonicalWatermark,
      replayCut.lossWatermarkGeneration,
      replayCut.lossWatermarkSeq,
    )) {
      return false;
    }

    // The visible historical window deliberately stays on its old generation,
    // so the pagination token alone cannot detect a newer subscription owner.
    // Queue one fresh coordinator read instead of committing or ACKing the
    // stale canonical response.
    if (canonicalOwnerWasOvertaken()) {
      requestCanonicalHeadHandoff(intent, true);
      return false;
    }

    const cutAcknowledged = acknowledgeConversationBranchReplay(
      attemptWorkspaceLocator,
      branchId,
      candidateSessionId(),
      canonicalWatermark.generation,
      canonicalWatermark.coveredRevision,
      canonicalWatermark.coveredSeq,
      replayCut.generation,
    );
    if (!cutAcknowledged || !ownsPaginationWindowRequest(requestToken)) return false;

    // C1 is read once after the prefix ACK. New retained values are safe to
    // merge; a new loss or generation/session transition stays in Router for
    // the next explicit recovery instead of starting another unbounded chase.
    const finalReplayCut = readConversationBranchReplaySnapshot(
      attemptWorkspaceLocator,
      branchId,
    );
    const finalCutHasReplay = finalReplayCut.events.length > 0
      || finalReplayCut.requiresCatchUp;
    const finalCutSessionMatches = !finalCutHasReplay
      || finalReplayCut.sessionId === candidateSessionId();
    const finalCutGenerationMatches = !finalCutHasReplay
      || finalReplayCut.timelineGeneration === candidateGeneration
      || (
        finalReplayCut.timelineGeneration === 0
        && candidateGeneration === 0
      );
    const finalCutHasNewLoss = finalReplayCut.lossWatermarkRouterGeneration
      > replayCut.generation;
    if (
      !finalCutSessionMatches
      || !finalCutGenerationMatches
      || finalCutHasNewLoss
    ) {
      return false;
    }
    const finalCutAcknowledged = acknowledgeConversationBranchReplay(
      attemptWorkspaceLocator,
      branchId,
      candidateSessionId(),
      canonicalWatermark.generation,
      canonicalWatermark.coveredRevision,
      canonicalWatermark.coveredSeq,
      finalReplayCut.generation,
    );
    if (!finalCutAcknowledged || !ownsPaginationWindowRequest(requestToken)) {
      return false;
    }

    const replayBelongsToCandidate = replayCut.timelineGeneration === candidateGeneration
      || (replayCut.timelineGeneration === 0 && candidateGeneration === 0);
    const replayEvents = replayBelongsToCandidate
      ? discardReplayThroughCut
        ? finalReplayCut.events
        : mergeAcpEvents(replayCut.events, finalReplayCut.events)
      : finalReplayCut.events;
    const replayTimelineEvents = liveTimelineUpdatesFromEvents(replayEvents);
    const mergedEvents = mergeAcpEvents(candidateEvents, replayTimelineEvents);
    const latestEvents = limitAcpEvents(
      mergedEvents,
      "start",
      effectiveLoadedEventBufferLimit,
    );
    candidateWindowTrimmed = candidateWindowTrimmed
      || latestEvents.length < mergedEvents.length;
    const { oldestSeq, newestSeq } = acpPaginationSeqBounds(
      latestEvents,
      eventIdPrefix,
    );
    const hasOlder = resolveAcpHasOlderEvents(
      candidatePage.hasOlder || candidateWindowTrimmed,
      mergedEvents.length,
      latestEvents.length,
    );
    const remainingReplay = readConversationBranchReplaySnapshot(
      attemptWorkspaceLocator,
      branchId,
    );
    const hasRemainingReplay = remainingReplay.events.length > 0
      || remainingReplay.requiresCatchUp;
    const committedPage: AcpSessionVm["eventPage"] = {
      ...candidatePage,
      generation: candidateGeneration || candidatePage.generation,
      loadedCount: latestEvents.length,
      total: Math.max(candidatePage.total, latestEvents.length),
      oldestSeq,
      newestSeq,
      hasOlder,
      hasNewer: hasRemainingReplay,
      oldestCursor: oldestSeq == null ? null : formatTimelineCursor(oldestSeq),
      newestCursor: newestSeq == null ? null : formatTimelineCursor(newestSeq),
    };
    const projectedCandidate = projectAcpSessionControlEvents(
      candidateSession,
      replayEvents,
      lifecycleProjectionRef.current,
    );
    const committedSession = settlePendingAcpInteractionsForLifecycle(
      reconcileAcpSessionForDisplay(
        latestSessionRef.current,
        {
          ...(projectedCandidate ?? candidateSession),
          events: latestEvents,
          eventPage: committedPage,
        },
      ),
      lifecycleProjectionRef.current,
    );
    if (!committedSession || !ownsPaginationWindowRequest(requestToken)) {
      return false;
    }

    const preserveDetachedViewport = viewportManualIntentRef.current;
    const scroller = chatContainerContextRef.current?.scrollRef.current;
    const detachedViewState = preserveDetachedViewport && scroller
      ? captureAcpBranchViewState(
          scroller,
          false,
          hasOlderEventsRef.current,
          hasNewerEventsRef.current,
        )
      : null;
    settleOptimisticPromptAdmissions(latestEvents);
    latestSessionRef.current = committedSession;
    paginationAnchorRef.current = null;
    if (preserveDetachedViewport) {
      pendingBranchViewRestoreRef.current = detachedViewState;
      pendingLatestLayoutCommitRef.current = null;
    } else {
      viewportAtBottomRef.current = true;
      commitShowReturnToLatest(false, "canonical-head-rejoin", scroller);
      pendingLatestLayoutCommitRef.current = sessionIdentity;
    }
    liveAnimationReadyRef.current = !hasRemainingReplay;
    reconcileConversationBranchSession({
      projectId,
      taskId,
      taskUuid,
      runId,
      roundId,
      nodeId,
      attemptId,
      outerNodeId,
      outerAttemptId,
    }, committedSession);
    storeAcpSession(eventWindowKey, committedSession);
    setCurrentSession(committedSession);
    commitLoadedEventWindow(eventWindowKey, {
      sessionId: committedSession.sessionId ?? null,
      timelineGeneration: candidateGeneration,
      events: latestEvents,
    });
    setHasOlderEvents(hasOlder);
    commitHasNewerEvents(hasRemainingReplay);
    canonicalTimelineCoverageRef.current = {
      eventWindowKey,
      watermark: canonicalWatermark,
    };
    paginationCursorGenerationStaleRef.current = false;
    return true;
  }, [attemptId, attemptWorkspaceLocator, branchId, commitHasNewerEvents, commitLoadedEventWindow, commitShowReturnToLatest, effectiveEventPageSize, effectiveLoadedEventBufferLimit, eventIdPrefix, eventWindowKey, nodeId, normalizeSessionUpdate, outerAttemptId, outerNodeId, ownsPaginationWindowRequest, projectId, requestCanonicalHeadHandoff, roundId, runId, sessionIdentity, settleLiveStreamingMarkdown, settleOptimisticPromptAdmissions, taskId, taskUuid]);

  const loadOlderEvents = async () => {
    const previousWindow = loadedEventWindowRef.current;
    const previousEvents = previousWindow.events;
    if (
      paginationCursorGenerationStaleRef.current ||
      paginationDirectionRef.current !== null ||
      !hasOlderEventsRef.current ||
      previousEvents.length === 0
    )
      return;
    const { oldestSeq } = acpPaginationSeqBounds(previousEvents, eventIdPrefix);
    if (oldestSeq === null) return;
    const beforeCursor = formatTimelineCursor(oldestSeq);
    const scroller = chatContainerContextRef.current?.scrollRef.current;
    paginationAnchorRef.current = scroller
      ? captureVisibleAcpAnchor(scroller)
      : null;
    const requestToken = beginPaginationRequest("older");
    chatContainerContextRef.current?.stopScroll();
    setLoadingOlder(true);
    try {
      const response = await getAcpSession(
          projectId,
          taskId,
          runId,
          roundId,
          nodeId,
          attemptId,
          {
            branchId,
            beforeCursor,
            beforeSeq: oldestSeq,
            pageSize: effectiveEventPageSize,
            eventLimit: effectiveEventPageSize,
          },
          baseSession,
          outerNodeId,
          outerAttemptId,
        ).catch(() => null);
      if (!ownsPaginationRequest(requestToken)) return;
      const updated = normalizeSessionUpdate(response);
      if (!updated) {
        paginationAnchorRef.current = null;
        return;
      }
      const activeWindow = loadedEventWindowRef.current;
      if (!paginationTokenOwnsLoadedEventWindow(requestToken, activeWindow)) {
        paginationAnchorRef.current = null;
        return;
      }
      const windowRelation = compareAcpLoadedEventWindowToSession(
        activeWindow,
        updated,
      );
      if (windowRelation !== "same") {
        paginationAnchorRef.current = null;
        paginationCursorGenerationStaleRef.current = true;
        setHasOlderEvents(false);
        commitHasNewerEvents(true);
        return;
      }
      chatContainerContextRef.current?.stopScroll();
      const merged = mergeAcpEvents(updated.events, activeWindow.events);
      const limited = limitAcpEvents(
        merged,
        "end",
        effectiveLoadedEventBufferLimit,
      );
      setHasOlderEvents(updated.eventPage.hasOlder);
      commitHasNewerEvents(
        updated.eventPage.hasNewer
          || limited.length < merged.length
          || replayHasUncoveredNewerEvents(updated.eventPage),
      );
      commitLoadedEventWindow(eventWindowKey, {
        ...activeWindow,
        events: limited,
      });
    } finally {
      if (finishPaginationRequest(requestToken)) setLoadingOlder(false);
    }
  };

  const loadNewerEvents = async () => {
    const previousWindow = loadedEventWindowRef.current;
    const previousEvents = previousWindow.events;
    if (
      paginationCursorGenerationStaleRef.current ||
      paginationDirectionRef.current !== null ||
      !hasNewerEventsRef.current ||
      previousEvents.length === 0
    )
      return;
    const { newestSeq } = acpPaginationSeqBounds(previousEvents, eventIdPrefix);
    if (newestSeq === null) return;
    const afterCursor = formatTimelineCursor(newestSeq);
    const scroller = chatContainerContextRef.current?.scrollRef.current;
    paginationAnchorRef.current = scroller
      ? captureVisibleAcpAnchor(scroller)
      : null;
    const requestToken = beginPaginationRequest("newer");
    const handOffReachedHead = () => {
      paginationAnchorRef.current = null;
      if (!finishPaginationRequest(requestToken)) return false;
      requestCanonicalHeadHandoff(
        canonicalHeadRecoveryPendingRef.current ? "recovery" : "ordinary",
        true,
      );
      return true;
    };
    chatContainerContextRef.current?.stopScroll();
    try {
      const response = await getAcpSession(
          projectId,
          taskId,
          runId,
          roundId,
          nodeId,
          attemptId,
          {
            branchId,
            afterCursor,
            afterSeq: newestSeq,
            pageSize: effectiveEventPageSize,
            eventLimit: effectiveEventPageSize,
          },
          baseSession,
          outerNodeId,
          outerAttemptId,
        ).catch(() => null);
      if (!ownsPaginationRequest(requestToken)) return;
      const updated = normalizeSessionUpdate(response);
      if (!updated) {
        paginationAnchorRef.current = null;
        return;
      }
      const activeWindow = loadedEventWindowRef.current;
      if (!paginationTokenOwnsLoadedEventWindow(requestToken, activeWindow)) {
        paginationAnchorRef.current = null;
        return;
      }
      chatContainerContextRef.current?.stopScroll();
      const activeScroller = chatContainerContextRef.current?.scrollRef.current;
      const remainsAtNewerEdge = activeScroller
        ? activeScroller.scrollHeight - activeScroller.scrollTop - activeScroller.clientHeight
          < NEWER_PAGE_LOAD_THRESHOLD_PX
        : viewportAtBottomRef.current;
      const windowRelation = compareAcpLoadedEventWindowToSession(
        activeWindow,
        updated,
      );
      if (windowRelation !== "same") {
        paginationCursorGenerationStaleRef.current = true;
        if (
          (windowRelation === "incoming-newer"
            || windowRelation === "different-session")
          && !updated.eventPage.hasNewer
          && remainsAtNewerEdge
        ) {
          if (!handOffReachedHead()) commitHasNewerEvents(true);
          return;
        }
        paginationAnchorRef.current = null;
        commitHasNewerEvents(true);
        return;
      }
      if (
        !updated.eventPage.hasNewer
        && remainsAtNewerEdge
      ) {
        if (!handOffReachedHead()) commitHasNewerEvents(true);
        return;
      }
      commitHasNewerEvents(
        updated.eventPage.hasNewer
          || replayHasUncoveredNewerEvents(updated.eventPage),
      );
      const merged = mergeAcpEvents(activeWindow.events, updated.events);
      const limited = limitAcpEvents(
        merged,
        "start",
        effectiveLoadedEventBufferLimit,
      );
      setHasOlderEvents(resolveAcpHasOlderEvents(
        updated.eventPage.hasOlder,
        merged.length,
        limited.length,
      ));
      commitLoadedEventWindow(eventWindowKey, {
        ...activeWindow,
        events: limited,
      });
    } finally {
      finishPaginationRequest(requestToken);
    }
  };

  const returnToLatestEvents = async (
    intent: AcpCanonicalHeadHandoffIntent,
  ) => {
    if (paginationDirectionRef.current !== null) return false;
    const discardReplayThroughCut = intent === "recovery"
      ? readConversationBranchReplaySnapshot(
          attemptWorkspaceLocator,
          branchId,
        )
      : null;
    const requestToken = beginPaginationRequest("newer");
    liveAnimationReadyRef.current = false;
    settleLiveStreamingMarkdown();
    chatContainerContextRef.current?.stopScroll();
    try {
      const response = await getAcpSession(
          projectId,
          taskId,
          runId,
          roundId,
          nodeId,
          attemptId,
          { branchId, pageSize: effectiveEventPageSize, eventLimit: effectiveEventPageSize },
          baseSession,
          outerNodeId,
          outerAttemptId,
        ).catch(() => null);
      if (!ownsPaginationRequest(requestToken)) return false;
      const updated = normalizeSessionUpdate(response);
      if (!updated) return false;
      const rejoined = await rejoinLatestHead(
        updated,
        updated.events,
        requestToken,
        intent,
        discardReplayThroughCut,
      );
      if (!ownsPaginationRequest(requestToken)) return false;
      if (!rejoined) {
        commitHasNewerEvents(true);
        return false;
      }
      return true;
    } finally {
      finishPaginationRequest(requestToken);
    }
  };
  canonicalHeadHandoffRef.current = returnToLatestEvents;

  const handleReturnToLatestEvents = () => {
    viewportManualIntentRef.current = false;
    if (!hasNewerEventsRef.current) {
      void chatContainerContextRef.current?.scrollToBottom({
        animation: "instant",
      });
      return;
    }
    commitReturnToLatestPending(true);
    requestCanonicalHeadHandoff(
      canonicalHeadRecoveryPendingRef.current ? "recovery" : "ordinary",
      true,
    );
  };

  const releaseSubmittedAttachments = (attachments: AttachmentItem[]) => {
    attachments.forEach(closeComposerAttachmentPreview);
    revokeAttachmentPreviewUrls(attachments);
  };

  const submitPrompt = async (
    submission: ConversationPromptInput,
    draftSnapshot?: AcpComposerDraft,
    target: "conversation" | "runtime-continue" = "conversation",
  ) => {
    const { displayText: draftContent, quotes: submittedQuotes } = submission;
    if (!composerState.canSubmit || composerState.stopInProgress) return false;
    const enqueueing = target === "conversation"
      && composerState.submitTarget === "queue-prompt";
    if (enqueueing) {
      if (queueSubmitPending || cancelling || stopInProgress) return;
      setQueueSubmitPending(true);
      setSendError(null);
      let detachedDraft: AcpComposerDraft | null = null;
      let submissionAccepted = false;
      try {
        const attachmentPaths = await resolveAttachmentPaths();
        detachedDraft = draftSnapshot && composerDraft.clearIfUnchanged(draftSnapshot)
          ? draftSnapshot
          : null;
        if (draftSnapshot && !detachedDraft) return;
        if (detachedDraft) setComposerContextError(null);
        const result = await submitConversationPrompt(
          projectId,
          taskId,
          runId,
          roundId,
          nodeId,
          attemptId,
          submission,
          null,
          effective ?? null,
          outerNodeId,
          outerAttemptId,
          attachmentPaths.length > 0 ? attachmentPaths : undefined,
        );
        if (!isAcceptedQueuePromptSubmitKind(result.kind)) {
          throw new Error(`unexpected prompt queue response: ${result.kind}`);
        }
        submissionAccepted = true;
        if (result.session) applySessionUpdate(result.session, "queue-prompt-submit");
        if (detachedDraft) {
          releaseSubmittedAttachments(detachedDraft.attachments);
          requestAnimationFrame(() => composerTextareaRef.current?.focus());
        }
        if (result.lifecycle) {
          applyLifecycleProjection(result.lifecycle);
          emitLifecycleSnapshot(result.lifecycle, result.session ?? null);
        }
      } catch (error) {
        setSendError(displayAppError(t, error));
      } finally {
        if (detachedDraft && !submissionAccepted) {
          composerDraft.restoreIfEmpty(detachedDraft);
        }
        setQueueSubmitPending(false);
      }
      return submissionAccepted;
    }
    if (composerState.submitTarget !== "acp-prompt") return false;
    setSending(true);
    setPromptCommandPending(true);
    setSendError(null);
    let attPaths: string[];
    try {
      attPaths = await resolveAttachmentPaths();
    } catch {
      setSending(false);
      return false;
    }
    const effectivePrompt = serializeUserPromptSubmission(submission);
    const optimisticAttachments = optimisticAttachmentPreviews(
      draftSnapshot?.attachments ?? [],
      attPaths,
    );
    const optimisticEvent = optimisticUserEvent(
      draftContent,
      undefined,
      submittedQuotes,
      latestCanonicalTimelinePosition(loadedEventWindowRef.current.events),
      optimisticAttachments,
    );
    const promptId = promptIdFromEvent(optimisticEvent);
    const detachedDraft = draftSnapshot && composerDraft.clearIfUnchanged(draftSnapshot)
      ? draftSnapshot
      : null;
    if (draftSnapshot && !detachedDraft) {
      setSending(false);
      setPromptCommandPending(false);
      return false;
    }
    if (detachedDraft) setComposerContextError(null);
    if (localLifecycle?.promptQueue) {
      requestAnimationFrame(() => composerTextareaRef.current?.focus());
    }
    setSendError(null);
    chatContainerContextRef.current?.scrollToBottom({
      animation: "instant",
      ignoreEscapes: true,
    });
    setActiveTurnPrompt(effectivePrompt);
    setActiveTurnPromptId(promptId);
    setActiveTurnStartedAt(null);
    setAwaitingResponse(true);
    updateOptimisticEvents((current) => [...current, optimisticEvent]);
    const acceptedPromptAdmission = () => findMatchingGoldBandUserPrompt(
      mergeAcpEvents(
        mergeAcpEvents(
          latestSessionRef.current?.events ?? [],
          loadedEventWindowRef.current.events,
        ),
        optimisticEventsRef.current.filter(
          (event) => !isPendingOptimisticPrompt(event) && event.status !== "failed",
        ),
      ),
      draftContent,
      promptId,
      optimisticEvent.timestamp,
    );
    let submissionAccepted = false;
    try {
      const result = target === "runtime-continue"
        ? await continueConversationRuntime(
            projectId,
            taskId,
            runId,
            roundId,
            nodeId,
            attemptId,
            outerNodeId,
            outerAttemptId,
            submission,
            promptId,
            attPaths.length > 0 ? attPaths : undefined,
          )
        : await submitConversationPrompt(
            projectId,
            taskId,
            runId,
            roundId,
            nodeId,
            attemptId,
            submission,
            promptId,
            effective ?? null,
            outerNodeId,
            outerAttemptId,
            attPaths.length > 0 ? attPaths : undefined,
          );
      const updated = result.session ?? null;
      if (updated) applySessionUpdate(updated);
      if (result.lifecycle) {
        applyLifecycleProjection(result.lifecycle);
        emitLifecycleSnapshot(result.lifecycle, result.session ?? null);
      }
      const authoritativeAdmission = acceptedPromptAdmission();
      if (target === "runtime-continue" && result.kind === "runtime-continue-started") {
        submissionAccepted = true;
        setActiveTurnStartedAt(optimisticEvent.timestamp);
      } else if (!updated && isAcceptedAcpPromptSubmitKind(result.kind)) {
        submissionAccepted = true;
        setActiveTurnStartedAt(optimisticEvent.timestamp);
      } else if (authoritativeAdmission) {
        submissionAccepted = true;
        setActiveTurnStartedAt(authoritativeAdmission.timestamp);
        if (updated && isSessionTerminalStatus(updated.status)) {
          setAwaitingResponse(false);
        }
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
        submissionAccepted = true;
        setAwaitingResponse(true);
        setActiveTurnPrompt(null);
        setActiveTurnPromptId(null);
        setActiveTurnStartedAt(null);
        updateOptimisticEvents((current) =>
          current.filter((event) => event.id !== optimisticEvent.id),
        );
        return true;
      }
      const admittedBeforeRecovery = acceptedPromptAdmission();
      if (admittedBeforeRecovery) {
        submissionAccepted = true;
        setActiveTurnStartedAt(admittedBeforeRecovery.timestamp);
        return true;
      }
      await refreshSessionAfterConfigUnavailable(error);
      const admittedAfterRecovery = acceptedPromptAdmission();
      if (admittedAfterRecovery) {
        submissionAccepted = true;
        setActiveTurnStartedAt(admittedAfterRecovery.timestamp);
        return true;
      }
      const message = displayAppError(t, error);
      if (target === "runtime-continue") {
        setRuntimeContinueError(message);
      } else {
        setSendError(message);
      }
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
      if (detachedDraft) {
        if (submissionAccepted) {
          releaseSubmittedAttachments(detachedDraft.attachments);
        } else {
          composerDraft.restoreIfEmpty(detachedDraft);
        }
      }
      setSending(false);
      setPromptCommandPending(false);
    }
    return submissionAccepted;
  };

  const applyQueueLifecycle = (lifecycle?: ConversationAttemptLifecycleVm | null) => {
    if (!lifecycle) return;
    applyLifecycleProjection(lifecycle);
    emitLifecycleSnapshot(lifecycle, effective ?? null);
  };

  const restoreQueuedPrompt = async (itemId: string) => {
    if (queueMutationPending || queueSubmitPending || composerDraftOccupied) return;
    setQueueMutationPending(true);
    setQueueRestorePending(true);
    setSendError(null);
    try {
      const result = await restoreConversationQueuedPrompt(
        projectId, taskId, runId, roundId, nodeId, attemptId,
        itemId, outerNodeId, outerAttemptId,
      );
      applyQueueLifecycle(result.lifecycle);
      const restoredDraft = queuedPromptToAcpComposerDraft(result.draft);
      if (!composerDraft.restoreIfEmpty(restoredDraft)) {
        setSendError(t('acp.promptQueue.restoreDraftConflict'));
        return;
      }
      requestAnimationFrame(() => composerTextareaRef.current?.focus());
      if (result.draft.attachmentPaths.length > 0) {
        void statAttachmentFiles(result.draft.attachmentPaths).then((fileRefs) => {
          const enrichedDraft = queuedPromptToAcpComposerDraft(result.draft, fileRefs);
          if (!composerDraft.replaceIfUnchanged(restoredDraft, enrichedDraft)) {
            revokeAttachmentPreviewUrls(enrichedDraft.attachments);
          }
        }).catch(() => {
          // The canonical paths remain in the draft; missing preview metadata must not discard them.
        });
      }
    } catch (error) {
      setSendError(displayAppError(t, error));
    } finally {
      setQueueMutationPending(false);
      setQueueRestorePending(false);
    }
  };

  const reorderQueuedPrompts = async (orderedItemIds: string[], expectedRevision: number) => {
    if (queueMutationPending || queueSubmitPending) return;
    setQueueMutationPending(true);
    setSendError(null);
    try {
      const result = await reorderConversationQueuedPrompts(
        projectId, taskId, runId, roundId, nodeId, attemptId,
        expectedRevision, orderedItemIds, outerNodeId, outerAttemptId,
      );
      applyQueueLifecycle(result.lifecycle);
    } catch (error) {
      setSendError(displayAppError(t, error));
      throw error;
    } finally {
      setQueueMutationPending(false);
    }
  };

  const deleteQueuedPrompt = async (itemId: string) => {
    if (queueMutationPending || queueSubmitPending) return;
    setQueueMutationPending(true);
    setSendError(null);
    try {
      const result = await deleteConversationQueuedPrompt(
        projectId, taskId, runId, roundId, nodeId, attemptId,
        itemId, outerNodeId, outerAttemptId,
      );
      applyQueueLifecycle(result.lifecycle);
    } catch (error) {
      setSendError(displayAppError(t, error));
    } finally {
      setQueueMutationPending(false);
    }
  };

  const useQueuedPrompt = async (itemId: string) => {
    if (queueMutationPending || queueSubmitPending || sessionActive) return;
    setQueueMutationPending(true);
    setSendError(null);
    try {
      const result = await useConversationQueuedPrompt(
        projectId, taskId, runId, roundId, nodeId, attemptId,
        itemId, outerNodeId, outerAttemptId,
      );
      if (result.session) applySessionUpdate(result.session);
      applyQueueLifecycle(result.lifecycle);
    } catch (error) {
      setSendError(displayAppError(t, error));
    } finally {
      setQueueMutationPending(false);
    }
  };

  const send = async () => {
    if (!canSubmitPrompt) return;
    const draftSnapshot = composerDraft.draft;
    const submission = createUserPromptSubmission(prompt, quotes);
    if (composerState.submitTarget !== "none") {
      await submitPrompt(submission, draftSnapshot);
    }
  };

  const addSelectedQuote = useCallback((selection: AgentMessageSelection) => {
    setQuotes((current) => {
      const result = addComposerQuote(current, {
        id: crypto.randomUUID(),
        sourceKey: selection.sourceKey,
        text: selection.text,
      });
      if (!result.ok) {
        setComposerContextError(
          result.code === 'composer.quote.limit-exceeded'
            ? t('acp.quoteLimitExceeded', { max: result.maxChars.toLocaleString() })
            : result.code === 'composer.quote.count-exceeded'
              ? t('acp.quoteCountExceeded', { max: result.maxQuotes })
              : t('acp.quoteDuplicate'),
        );
        return current;
      }
      setComposerContextError(null);
      requestAnimationFrame(() => composerTextareaRef.current?.focus());
      return result.quotes;
    });
  }, [setQuotes, t]);

  const removeQuote = useCallback((id: string) => {
    setQuotes((current) => current.filter((quote) => quote.id !== id));
    setComposerContextError(null);
  }, [setQuotes]);

  const stopSession = async () => {
    if (!canStopSession || stopInProgress) return;
    cancelRequestedRef.current = true;
    setCancelling(true);
    setStopCommandPending(true);
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
      const stopPlan = planAcpStopResponse(result);
      const awaitTerminalStop = stopPlan.awaitTerminal;
      awaitTerminalStopRef.current = awaitTerminalStop;
      setRuntimeStopAccepted(stopPlan.accepted);
      if (result.lifecycle) {
        applyLifecycleProjection(result.lifecycle);
        emitLifecycleSnapshot(result.lifecycle, result.session ?? null);
      }
      if (stopPlan.sessionSnapshot) applySessionUpdate(stopPlan.sessionSnapshot);
      flushPendingLiveEvents();
      setStopCommandPending(false);
      setPromptCommandPending(false);
      setSending(false);
      setActiveTurnPrompt(null);
      setActiveTurnPromptId(null);
      setActiveTurnStartedAt(null);
      if (!awaitTerminalStop) {
        setCancelling(false);
        setAwaitingResponse(false);
        cancelRequestedRef.current = false;
        onSessionStopped?.();
      }
      updateOptimisticEvents(clearPendingOptimisticPromptsAfterStop);
    } catch (error) {
      setCancelError(displayAppError(t, error));
      setCancelling(false);
      setStopCommandPending(false);
      cancelRequestedRef.current = false;
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

  const continueRuntime = async () => {
    if (!showRuntimeContinueAction || runtimeContinueSubmitting) return;
    setRuntimeContinueError(null);
    setRuntimeContinueSubmitting(true);
    try {
      if (
        localLifecycle?.continueKind === 'continue-current-attempt'
        && canSubmitPrompt
      ) {
        const accepted = await submitPrompt(
          createUserPromptSubmission(prompt, quotes),
          composerDraft.draft,
          "runtime-continue",
        );
        if (!accepted) {
          setRuntimeContinueSubmitting(false);
          return;
        }
        setRuntimeStopAccepted(false);
        onSessionStopped?.();
        return;
      }
      const recoveryRevision = localLifecycle?.runtime.revision;
      let result: ConversationPromptSubmitVm;
      if (localLifecycle?.continueKind === 'recover-completed-attempt') {
        if (recoveryRevision == null) return;
        result = await recoverConversationRuntime(
            projectId,
            taskId,
            runId,
            roundId,
            nodeId,
            attemptId,
            recoveryRevision,
          );
      } else {
        result = await continueConversationRuntime(
            projectId,
            taskId,
            runId,
            roundId,
            nodeId,
            attemptId,
            outerNodeId,
            outerAttemptId,
          );
      }
      if (result.lifecycle) {
        applyLifecycleProjection(result.lifecycle);
        emitLifecycleSnapshot(result.lifecycle, result.session ?? null);
      }
      setRuntimeStopAccepted(false);
      onSessionStopped?.();
    } catch (error) {
      await refreshSessionAfterConfigUnavailable(error);
      setRuntimeContinueError(displayAppError(t, error));
      setRuntimeContinueSubmitting(false);
    }
  };

  const answerPermission = async (
    request: AcpPermissionRequestVm,
    optionId: string,
  ) => {
    const operationOwner = eventWindowKey;
    setPermissionError(null);
    setDismissedPermissionIds((current) =>
      new Set(current).add(request.interactionId),
    );
    try {
      const updated = await respondAcpPermission(
        projectId,
        taskId,
        runId,
        roundId,
        nodeId,
        attemptId,
        request.interactionId,
        optionId,
        effective,
        outerNodeId,
        outerAttemptId,
      );
      if (sessionIdentityRef.current !== operationOwner) return;
      if (branchId === 'root' || updated?.branchId === branchId) {
        applySessionUpdate(updated);
      } else {
        try {
          const refreshedBranch = await getAcpSession(
            projectId,
            taskId,
            runId,
            roundId,
            nodeId,
            attemptId,
            {
              branchId,
              pageSize: effectiveEventPageSize,
              eventLimit: effectiveEventPageSize,
            },
            effective,
            outerNodeId,
            outerAttemptId,
          );
          if (sessionIdentityRef.current !== operationOwner) return;
          applySessionUpdate(refreshedBranch);
        } catch (error) {
          if (sessionIdentityRef.current !== operationOwner) return;
          setPermissionError(displayAppError(t, error));
        }
      }
    } catch (error) {
      if (sessionIdentityRef.current !== operationOwner) return;
      setDismissedPermissionIds((current) => {
        const next = new Set(current);
        next.delete(request.interactionId);
        return next;
      });
      setPermissionError(displayAppError(t, error));
    }
  };

  const answerElicitation = async (
    elicitationId: string,
    content?: Record<string, unknown>,
  ) => {
    const operationOwner = eventWindowKey;
    setSendError(null);
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
    } catch (error) {
      if (sessionIdentityRef.current !== operationOwner) return;
      setAnsweredElicitations((current) => {
        const next = new Map(current);
        next.delete(elicitationId);
        return next;
      });
      setSendError(displayAppError(t, error));
    }
  };

  const declineElicitation = async (elicitationId: string) => {
    const operationOwner = eventWindowKey;
    setSendError(null);
    setAnsweredElicitations((current) => {
      const next = new Map(current);
      next.set(elicitationId, { __declined: true });
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
        "decline",
        null,
        outerNodeId,
        outerAttemptId,
      );
    } catch (error) {
      if (sessionIdentityRef.current !== operationOwner) return;
      setAnsweredElicitations((current) => {
        const next = new Map(current);
        next.delete(elicitationId);
        return next;
      });
      setSendError(displayAppError(t, error));
    }
  };

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
        order: next.order,
      });
    } finally {
      setRawLoading(false);
    }
  };

  const toggleRawFrames = async () => {
    if (rightWorkspace?.scopeKey) {
      rightWorkspace.openResource({
        kind: 'raw-frames',
        key: rawFramesWorkspaceKey,
        scopeKey: rightWorkspace.scopeKey,
        title: t('acp.rawFrames'),
        attention: false,
        locator: attemptWorkspaceLocator,
      });
      return;
    }
    preserveScrollPosition();
    if (canvasMode === "raw") {
      setCanvasMode("chat");
      return;
    }
    if (rawPage == null) await loadRawFrames(rawQuery);
    setCanvasMode("raw");
  };

  const openSystemPrompt = () => {
    if (rightWorkspace?.scopeKey) {
      rightWorkspace.openResource({
        kind: 'system-prompt',
        key: systemPromptWorkspaceKey,
        scopeKey: rightWorkspace.scopeKey,
        title: t('acp.systemPrompt'),
        attention: false,
        locator: attemptWorkspaceLocator,
      });
      return;
    }
    setSystemPromptOpen(true);
  };

  const scrollFrameRef = useRef<number | null>(null);

  const handleScrollRef = useRef<((scroller: HTMLDivElement) => void) | null>(null);
  handleScrollRef.current = (scroller) => {
    if (preservingScrollRef.current) return;
    // Scroll events also come from streaming layout. Follow intent is changed
    // only by user input and explicit pagination/restore boundaries.
    const scrollTop = scroller.scrollTop;
    if (scrollTop < HISTORY_LOAD_THRESHOLD_PX) void loadOlderEvents();
    const distanceFromBottom =
      scroller.scrollHeight - scrollTop - scroller.clientHeight;
    commitShowReturnToLatest(
      shouldShowReturnToLatest(
        showReturnToLatestRef.current,
        viewportAtBottomRef.current,
        hasNewerEventsRef.current,
        viewportManualIntentRef.current || hasNewerEventsRef.current,
        distanceFromBottom,
      ),
      "viewport-scroll",
      scroller,
    );
    if (distanceFromBottom < NEWER_PAGE_LOAD_THRESHOLD_PX && hasNewerEvents) {
      void loadNewerEvents();
    }
    storeAcpBranchViewState(eventWindowKey, captureAcpBranchScrollState(
      scroller,
      viewportAtBottomRef.current,
      hasOlderEventsRef.current,
      hasNewerEventsRef.current,
    ));
  };
  const handleScroll = useCallback((scroller: HTMLDivElement) => {
    if (scrollFrameRef.current != null) return;
    scrollFrameRef.current = requestAnimationFrame(() => {
      scrollFrameRef.current = null;
      handleScrollRef.current?.(scroller);
    });
  }, []);

  const sessionShellState = resolveAcpSessionShellState({
    hasBaseSession: Boolean(baseSession),
    baseSessionReady: isAcpSessionReadyForInitialDisplay(baseSession),
    hasLiveSessionShell: Boolean(liveSessionShell),
    hasEstablishedSessionShell: Boolean(establishedSessionShell),
    hasSettledAttemptShell: cancelledDirectAttemptShell,
    initialSessionLoading: initialSessionQueryState === "loading",
    initialSessionLoadFailed: initialSessionQueryState === "error",
    initializationFailed: sessionInitializationFailed,
    initializationInterrupted: sessionInitializationInterrupted,
    runtimeActive: runtimeActiveFromContext,
    showInitializingShell: initializationLifecycleActive,
  });

  if (sessionShellState === 'error') {
    return (
      <AcpErrorState
        reason={
          acpSessionLoadErrorReason(
            runtimeComposerContext?.runtimeError ?? runtimeComposerContext?.runtimeErrorFallback,
            sessionLoadError,
            baseSession,
            t("acp.missingSessionReason"),
          )
        }
        transparent={wallpaperSurface}
      />
    );
  }

  if (sessionShellState === 'interrupted') {
    return <AcpInterruptedState label={t("acp.sessionInterrupted")} transparent={wallpaperSurface} />;
  }

  if (isAcpSessionLoadingSurfaceState(sessionShellState)) {
    return (
      <AcpLoadingState
        label={t("common.loading")}
        transparent={wallpaperSurface}
      />
    );
  }

  if (!effective) {
    return (
      <AcpErrorState
        reason={
          runtimeComposerContext?.runtimeError
          ?? runtimeComposerContext?.runtimeErrorFallback
          ?? sessionLoadError
          ?? t("acp.missingSessionReason")
        }
        transparent={wallpaperSurface}
      />
    );
  }

  // A failed provider attempt can be followed by an automatic retry. Until
  // the runtime reaches its final terminal state, the retry progress is the
  // user-facing state; showing a terminal banner here is contradictory.
  const localLifecycleUsesRuntimeErrorFallback = shouldTreatAcpRuntimeErrorAsFallback(
    !(runtimeComposerContext?.isOrchestrated ?? true),
    localLifecycle,
  );
  const bannerRuntimeError = localLifecycleUsesRuntimeErrorFallback
    ? null
    : runtimeComposerContext?.runtimeError;
  const bannerRuntimeErrorFallback = localLifecycleUsesRuntimeErrorFallback
    ? runtimeComposerContext?.runtimeError ?? runtimeComposerContext?.runtimeErrorFallback
    : runtimeComposerContext?.runtimeErrorFallback;
  const visibleError = runtimeActive
    ? null
    : visibleAcpBannerError(
      bannerRuntimeError,
      effective,
      effectiveEvents,
      bannerRuntimeErrorFallback,
      localLifecycle?.acp.latestTurnStatus,
    );

  return (
    <TurnFileCardPreviewLimitContext.Provider value={turnFileCardPreviewLimit}>
    <TurnAttachmentCardPreviewLimitContext.Provider value={turnAttachmentCardPreviewLimit}>
    <AcpBranchLocatorContext.Provider value={attemptWorkspaceLocator}>
    <div
      ref={conversationRootRef}
      className={cn(
        "flex h-full min-h-0 min-w-0 flex-col",
        wallpaperSurface ? "bg-transparent" : "bg-background",
      )}
      data-conversation-branch-id={branchId}
    >
      <ACPSessionHeader
        session={effective}
        rawActive={resolveRawFramesActionActive(Boolean(rightWorkspace?.scopeKey), canvasMode === "raw")}
        rawLoading={rawLoading}
        showSystemPromptAction={showSystemPromptAction}
        showRawFramesAction={showRawFramesAction}
        directSessionHeader={directSessionHeader}
        systemPromptAvailable={
          Boolean(effective.systemPromptAppend?.trim()) ||
          Boolean(systemPromptOptions?.some((option) => option.prompt?.trim()))
        }
        onToggleRaw={toggleRawFrames}
        onOpenSystemPrompt={openSystemPrompt}
      />
      {readOnly && effective.branchExecution ? (
        <AgentBranchSessionSummary
          execution={effective.branchExecution}
          status={resolveConversationBranchDisplayStatus(
            effective.branchExecution.executionStatus || effective.status,
            branchLiveSnapshot.status,
          ) ?? effective.status}
          elapsedSeconds={effective.timing?.sessionElapsedSeconds ?? effective.sessionElapsedSeconds}
        />
      ) : null}
      <SystemPromptDialog
        open={systemPromptOpen}
        prompt={effective.systemPromptAppend}
        options={systemPromptOptions}
        onOpenChange={setSystemPromptOpen}
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
          <ConversationViewport
            scrollClassName={ACP_SESSION_SCROLL_AREA_CLASS_NAME}
            contextRef={chatContainerContextRef}
            initialFollowing={shouldInitiallyFollowAcpBranch(restoredBranchViewState)}
            onAtBottomChange={handleAtBottomChange}
            onFollowIntentChange={handleFollowIntentChange}
            onViewportScroll={handleScroll}
            onViewportUserScroll={handleLiveStreamUserInteraction}
          >
              {loadingOlder ? (
                <AcpListLoading label={t("acp.loadingOlderEvents")} />
              ) : hasOlderEvents ? (
                <AcpHistoryHint label={t("acp.scrollForHistory")} />
              ) : (
                <div className="h-3" />
              )}
              {timelineSurfaceState === 'empty' ? (
                <div className="p-5">
                  <EmptyAcpState />
                </div>
              ) : timelineSurfaceState === 'pending' ? (
                <div className="p-5">
                  <AcpPendingTimelineState label={composerStatusLabel} />
                </div>
              ) : (
                <AcpTimelineWindowOwnerContext.Provider
                  key={timelineWindowRenderScopeKey(
                    timelineWindowOwner.eventWindowKey,
                    timelineWindowOwner.sessionId,
                  )}
                  value={timelineWindowOwner}
                >
                  <div
                    className="mx-auto w-full max-w-[var(--conversation-content-rail-max-inline-size)] space-y-1 px-5 py-5"
                    data-acp-conversation-rail="timeline"
                  >
                    {timeline.map((item) => (
                      <div
                        key={timelineEventKey(item)}
                        data-acp-item-key={timelineEventKey(item)}
                      >
                        <ACPTimelineItemRenderer
                          event={item}
                          streamingMarkdownItemKey={streamingMarkdownItemKey}
                          messageAttachmentLocator={messageAttachmentLocator}
                          onMessageAttachmentClick={handleOpenMessageAttachment}
                        />
                      </div>
                    ))}
                  </div>
                </AcpTimelineWindowOwnerContext.Provider>
              )}
              <InterventionLayer>
                {sendError ? (
                  <AcpErrorBanner
                    title={t("acp.sendFailed")}
                    reason={sendError}
                  />
                ) : null}
                {cancelError ? (
                  <AcpErrorBanner
                    title={t("acp.stopFailed")}
                    reason={cancelError}
                  />
                ) : null}
                {manualCheckError ? (
                  <AcpErrorBanner
                    title={t("acp.manualCheckSubmitFailed")}
                    reason={manualCheckError}
                  />
                ) : null}
                {runtimeContinueError ? (
                  <AcpErrorBanner
                    title={t(localLifecycle?.continueKind === 'recover-completed-attempt' ? "acp.recoverWorkflowFailed" : "acp.continueWorkflowFailed")}
                    reason={runtimeContinueError}
                  />
                ) : null}
                {permissionError ? (
                  <AcpErrorBanner reason={permissionError} />
                ) : null}
                {pendingPermission ? (
                  <PermissionRequestCard
                    request={pendingPermission}
                    status="pending"
                    onSelect={(optionId) => void answerPermission(pendingPermission, optionId)}
                  />
                ) : null}
                {pendingElicitation ? (
                  <ElicitationCard
                    key={pendingElicitation.interactionId}
                    elicitationId={pendingElicitation.interactionId}
                    message={pendingElicitation.message}
                    schema={pendingElicitation.requestedSchema}
                    onRespond={(content) =>
                      answerElicitation(
                        pendingElicitation.interactionId,
                        content,
                      )
                    }
                    onDecline={() =>
                      declineElicitation(pendingElicitation.interactionId)
                    }
                  />
                ) : null}
              </InterventionLayer>
              <ConversationViewportFooter
                className={cn(
                  "z-20",
                  wallpaperSurface ? "bg-transparent" : "bg-background",
                )}
                data-acp-conversation-footer="viewport"
              >
                {showReturnToLatest ? (
                  <Button
                    ref={handleReturnToLatestButtonRef}
                    type="button"
                    size="sm"
                    variant="secondary"
                    className="absolute right-4 top-0 z-30 -translate-y-[calc(100%+1rem)] gap-1.5 rounded-full border border-border/60 bg-background/95 shadow-sm backdrop-blur"
                    disabled={returnToLatestPending}
                    onClick={handleReturnToLatestEvents}
                    data-acp-return-to-latest="true"
                  >
                    <ChevronDown className="size-3.5" />
                    {t("acp.returnToLatest")}
                  </Button>
                ) : null}
          <div className="px-5 pt-1 pb-2">
            <div
              className="relative mx-auto w-full max-w-[var(--conversation-content-rail-max-inline-size)] [--acp-composer-rail-shadow:var(--gb-material-shadow)] [filter:drop-shadow(var(--acp-composer-rail-shadow))] dark:[--acp-composer-rail-shadow:var(--gb-elevation-overlay)]"
              data-acp-conversation-rail="composer"
              style={ACP_SESSION_COMPOSER_BORDER_STYLE}
            >
              <AcpUsagePanel
                usage={effective?.usage}
                processingLabel={showComposerStatus ? composerStatusLabel : null}
                sessionSeconds={composerSessionSeconds}
                worktreePath={worktreePath}
                branchProjectId={showBranchInfo ? projectId : null}
                managedWorktreeBranch={effective?.worktreeBranch ?? managedWorktreeBranch}
                className={cn(
                  ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName,
                  "absolute left-0 top-0 z-20 w-max max-w-[calc(100%-0.625rem)] -translate-y-full flex-nowrap gap-x-2 rounded-t-md border-b-0 bg-card py-0.5 pl-2.5 pr-3 !shadow-none after:pointer-events-none after:absolute after:inset-x-0 after:bottom-[calc(-1*var(--acp-session-composer-border-width))] after:h-[var(--acp-session-composer-border-width)] after:bg-card after:content-['']",
                )}
              />
            {!readOnly && showManualCheckActions ? (
              <AcpManualCheckPanel
                submitting={manualCheckSubmitting}
                integratedInfoTab={composerInfoTabTarget === "manual"}
                onSuccess={() => void submitManualDecision("success")}
                onFailure={() => void submitManualDecision("failure")}
              />
            ) : null}
            {todoEntries.length > 0 ? (
              <AcpTodoPanel
                entries={todoEntries}
                attachedBelow={!readOnly}
                integratedInfoTab={composerInfoTabTarget === "todo"}
              />
            ) : null}
            {!readOnly && promptQueueVisible && promptQueue ? (
              <ConversationPromptQueue
                queue={promptQueue}
                sessionActive={sessionActive || stopInProgress}
                mutationPending={queueMutationPending || queueSubmitPending}
                composerOccupied={composerDraftOccupied}
                attachedAbove={todoEntries.length > 0}
                integratedInfoTab={composerInfoTabTarget === "queue"}
                onRestore={restoreQueuedPrompt}
                onReorder={reorderQueuedPrompts}
                onUse={useQueuedPrompt}
                onDelete={deleteQueuedPrompt}
              />
            ) : null}
            {readOnly ? null : composerState.externalKind ? (
              <AcpExternalComposerState
                kind={composerState.externalKind}
                message={composerState.externalMessage ?? ""}
                integratedInfoTab={composerInfoTabTarget === "composer"}
                onAction={
                  composerState.externalKind === "invalid-workflow"
                    ? runtimeComposerContext?.onRepair
                    : undefined
                }
              />
            ) : (
              <AcpConversationComposer
                prompt={prompt}
                onPromptChange={setPrompt}
                onSubmit={send}
                sending={sending}
                attachments={pendingAttachments}
                quotes={quotes}
                contextError={composerContextError}
                onRemoveQuote={removeQuote}
                onRemoveAttachment={removeComposerAttachment}
                onPreviewAttachment={handleOpenComposerAttachment}
                onClearAttachments={clearComposerAttachments}
                fileError={fileError}
                slashCommands={slashCommands.filteredCommands}
                slashMenuOpen={slashCommands.isOpen}
                slashMenuActiveIndex={slashCommands.activeIndex}
                onSlashMenuActiveIndexChange={slashCommands.setActiveIndex}
                onSlashMenuDismiss={slashCommands.dismiss}
                onSlashMenuSelect={(index) => { slashCommands.selectByIndex(index); }}
                textareaRef={composerTextareaRef}
                committedSlashCommand={committedSlashCommand ? {
                  prefix: committedSlashCommand.prefix,
                  description: committedSlashCommand.command.description,
                } : null}
                placeholder={composerPlaceholder}
                inputDisabled={composerInputDisabled || queueRestorePending}
                onTextareaKeyDown={slashCommands.onKeyDown}
                onDragEnter={dropZoneHandlers.onDragEnter}
                onDragOver={dropZoneHandlers.onDragOver}
                onDrop={dropZoneHandlers.onDrop}
                onPaste={handlePaste}
                fileInputRef={fileInputRef}
                onFilesChange={handleFilesFromInput}
                onPickFiles={pickFiles}
                canStop={canStopSession}
                stopInProgress={stopInProgress}
                onStop={stopSession}
                canSubmit={canSubmitPrompt}
                sendButtonBusy={composerState.submitTarget === "queue-prompt" ? queueSubmitPending : sendButtonBusy}
                showRuntimeContinue={showRuntimeContinueAction}
                runtimeContinueKind={localLifecycle?.continueKind ?? null}
                runtimeContinueSubmitting={runtimeContinueSubmitting}
                onRuntimeContinue={continueRuntime}
                configBar={(
                  <AcpSessionConfigBar
                    scopeKey={sessionIdentity}
                    viewModel={sessionConfigViewModel}
                    onModelChange={handleAcpSessionModelChange}
                    onConfigOptionChange={handleAcpSessionConfigOptionChange}
                    onPermissionModeChange={handleAcpSessionPermissionModeChange}
                  />
                )}
                attachedPanelVisible={promptQueueVisible || todoEntries.length > 0}
                integratedInfoTab={composerInfoTabTarget === "composer"}
                queueSubmit={composerState.submitTarget === "queue-prompt"}
                supersededSession={supersededSession}
              />
            )}
            </div>
          </div>
              </ConversationViewportFooter>
          </ConversationViewport>
        )}
      </div>
      {!readOnly && !queueRestorePending ? <AgentSelectionQuoteButton rootRef={conversationRootRef} onQuote={addSelectedQuote} /> : null}
    </div>
    </AcpBranchLocatorContext.Provider>
    </TurnAttachmentCardPreviewLimitContext.Provider>
    </TurnFileCardPreviewLimitContext.Provider>
  );
}

function AcpErrorState({ reason, transparent = false }: { reason: string; transparent?: boolean }) {
  return (
    <div className={cn("flex h-full min-h-0 flex-col", transparent ? "bg-transparent" : "bg-background")}>
      <AcpErrorBanner reason={reason} />
      <div className="flex-1" />
    </div>
  );
}

function AcpLoadingState({ label, transparent = false }: { label: string; transparent?: boolean }) {
  return <BrandLoadingState label={label} surface={transparent ? "transparent" : "background"} />;
}

function AcpInterruptedState({ label, transparent = false }: { label: string; transparent?: boolean }) {
  return (
    <div className={cn(
      "flex h-full min-h-0 items-center justify-center px-6 text-center text-sm font-medium text-muted-foreground",
      transparent ? "bg-transparent" : "bg-background",
    )}>
      {label}
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

export function captureAcpBranchViewState(
  scroller: HTMLElement,
  atBottom: boolean,
  hasOlder: boolean,
  hasNewer: boolean,
): AcpBranchViewState {
  const anchor = captureVisibleAcpAnchor(scroller);
  const scrollerTop = scroller.getBoundingClientRect().top;
  return {
    anchorKey: anchor?.key ?? null,
    anchorOffset: anchor ? anchor.top - scrollerTop : 0,
    scrollTop: scroller.scrollTop,
    atBottom,
    hasOlder,
    hasNewer,
  };
}

export function captureAcpBranchScrollState(
  scroller: Pick<HTMLElement, "scrollTop">,
  atBottom: boolean,
  hasOlder: boolean,
  hasNewer: boolean,
): AcpBranchViewState {
  return {
    // A measured anchor is invalid once the viewport moves. The scroll hot
    // path keeps an O(1) scrollTop fallback; unmount captures a fresh anchor.
    anchorKey: null,
    anchorOffset: 0,
    scrollTop: scroller.scrollTop,
    atBottom,
    hasOlder,
    hasNewer,
  };
}

function findAcpItemElement(scroller: HTMLElement, key: string) {
  return (
    Array.from(
      scroller.querySelectorAll<HTMLElement>("[data-acp-item-key]"),
    ).find((element) => element.dataset.acpItemKey === key) ?? null
  );
}

export function applyAcpScrollAnchorCompensation(
  scroller: HTMLElement,
  key: string,
  expectedTop: number,
) {
  const element = findAcpItemElement(scroller, key);
  if (!element) return false;
  scroller.scrollTop += element.getBoundingClientRect().top - expectedTop;
  return true;
}

function AcpExternalComposerState({
  kind,
  message,
  onAction,
  integratedInfoTab = false,
}: {
  kind: "invalid-workflow" | "runtime-error";
  message: string;
  onAction?: () => void;
  integratedInfoTab?: boolean;
}) {
  const { t } = useTranslation();
  const isError = kind === "runtime-error";
  return (
    <div
      className={cn(
        "flex min-w-0 items-center gap-3 rounded-2xl px-5 py-4 shadow-none",
        ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName,
        integratedInfoTab && "rounded-tl-none",
        isError
          ? "bg-destructive/5"
          : "bg-amber-500/5",
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
  integratedInfoTab = false,
}: {
  submitting: boolean;
  onSuccess: () => void;
  onFailure: () => void;
  integratedInfoTab?: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className={cn(
      "mb-3 flex min-w-0 items-center gap-3 rounded-2xl bg-card px-4 py-2.5 shadow-none",
      ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName,
      integratedInfoTab && "rounded-tl-none",
    )}>
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

export function AcpTodoPanel({
  entries,
  variant = "composer",
  attachedBelow = true,
  integratedInfoTab = false,
}: {
  entries: AcpTodoEntry[];
  variant?: "composer" | "nested";
  attachedBelow?: boolean;
  integratedInfoTab?: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(variant === "nested");

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
      data-acp-todo-panel="true"
      open={open}
      onOpenChange={setOpen}
      className={cn(
        "w-full",
        variant === "composer"
          ? cn(
              "overflow-hidden bg-card",
              ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName,
              attachedBelow ? "rounded-t-2xl" : "rounded-2xl",
              integratedInfoTab && "rounded-tl-none",
            )
          : "overflow-hidden rounded-lg border border-border/35 bg-transparent",
      )}
    >
      <CollapsibleTrigger asChild>
        <Button
          variant="ghost"
          className={cn(
            "h-auto w-full justify-between border-0 px-3 py-2 font-normal shadow-none hover:bg-muted/30 focus-visible:border-transparent focus-visible:ring-0",
            variant === "composer" ? "rounded-none hover:bg-transparent" : "rounded-lg",
          )}
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
        <div className={cn(
          "px-3 pb-1.5",
          variant === "nested" && "divide-y divide-border/25 border-t border-border/35",
        )}>
          {entries.map((entry, index) => (
            <div data-acp-todo-row="true" className="flex min-h-8 min-w-0 items-center gap-2 py-1 text-xs" key={index}>
              <TodoStatusMark status={entry.status} />
              <span className="min-w-0 flex-1 break-words text-foreground/90 [overflow-wrap:anywhere]">
                {entry.content}
              </span>
              {entry.status || entry.priority ? (
                <span className="shrink-0 text-ui-caption text-muted-foreground">
                  {entry.status ? displayStatus(t, entry.status) : entry.priority}
                </span>
              ) : null}
            </div>
          ))}
        </div>
      </CollapsibleContent>
    </Collapsible>
  );
}

function TodoStatusMark({
  status,
}: {
  status?: string;
}) {
  const normalized = status?.toLowerCase();
  if (normalized === "completed" || normalized === "complete") {
    return (
      <span className="flex size-5 shrink-0 items-center justify-center rounded-full bg-emerald-500/12 text-emerald-700 dark:text-emerald-300">
        <Check className="size-3" aria-hidden="true" />
      </span>
    );
  }
  if (normalized === "in_progress" || normalized === "running") {
    return (
      <span className="flex size-5 shrink-0 items-center justify-center">
        <AcpProcessingSpinner className="size-4" />
      </span>
    );
  }
  return (
    <span
      aria-hidden="true"
      data-acp-todo-pending-mark="true"
      className="mx-1.5 size-2 shrink-0 rounded-full border border-muted-foreground/55 bg-transparent"
    />
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

function AcpErrorBanner({ reason, title }: { reason: string; title?: string }) {
  const { t } = useTranslation();
  return (
    <div className="shrink-0 border-b border-destructive/20 bg-destructive/5 px-5 py-3 text-sm">
      <span className="font-semibold text-destructive">
        {title ?? t("acp.sessionFailed")}
      </span>
      <span className="ml-2 text-muted-foreground">{reason}</span>
    </div>
  );
}

type AcpSessionConfigBarProps = {
  scopeKey: string;
  viewModel: AcpSessionConfigViewModel;
  onModelChange?: (modelId: string | null) => void;
  onPermissionModeChange?: (permissionModeId: string | null) => void;
  onConfigOptionChange?: (optionId: string, optionValue: string | null) => void;
};

const AcpSessionConfigBar = memo(function AcpSessionConfigBar({
  viewModel,
  onModelChange,
  onConfigOptionChange,
  onPermissionModeChange,
}: AcpSessionConfigBarProps) {
  const { t } = useTranslation();
  const {
    modelOverrideId,
    modelOverrideName,
    canSelectUnspecifiedModel,
    permissionModeOverrideId,
    permissionModeOverrideName,
    canSelectUnspecifiedPermissionMode,
    currentModelId,
    currentModeId,
    availableModels,
    availablePermissionModes,
    thoughtLevel,
  } = viewModel;

  const handlePermissionModeSelect = useCallback(
    (permissionModeId: string | null) => {
      onPermissionModeChange?.(permissionModeId);
    },
    [onPermissionModeChange],
  );

  const permissionModeLabel = permissionModeOverrideName
    ?? t('conversation.home.unspecifiedPermissionMode');
  const showModels = availableModels.length > 0 || Boolean(currentModelId);
  const showPermissionModes = availablePermissionModes.length > 0 || Boolean(currentModeId);
  const permissionModeCanBeSelected = availablePermissionModes.length > 1
    || (canSelectUnspecifiedPermissionMode && availablePermissionModes.length > 0);

  if (!showModels && !showPermissionModes && !thoughtLevel) return null;

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-1.5 text-xs text-muted-foreground" data-acp-session-config-bar="true">
      <AcpModelThoughtSelects
        compact
        contentSide="top"
        align="start"
        triggerClassName={ACP_SESSION_COMPOSER_LAYOUT.configTriggerClassName}
        models={availableModels}
        modelValue={modelOverrideId}
        modelValueLabel={modelOverrideName}
        thoughtLevel={thoughtLevel ? {
          id: thoughtLevel.id,
          category: thoughtLevel.category,
          name: thoughtLevel.name,
          description: thoughtLevel.description,
          currentValue: thoughtLevel.currentValue,
          options: thoughtLevel.options.map((option) => ({
            value: option.id,
            name: option.name,
            description: option.description,
            available: option.available,
          })),
        } : null}
        thoughtValue={thoughtLevel?.overrideValue}
        thoughtValueLabel={thoughtLevel?.overrideValueName}
        showUnspecifiedModel={canSelectUnspecifiedModel}
        showUnspecifiedThought={thoughtLevel?.canSelectUnspecified ?? true}
        onModelChange={(value) => onModelChange?.(value)}
        onThoughtChange={(optionId, value) => onConfigOptionChange?.(optionId, value)}
      />
      {showPermissionModes ? (
        permissionModeCanBeSelected ? (
          <AcpSingleConfigMenu
            compact
            contentSide="top"
            align="start"
            triggerClassName={ACP_SESSION_COMPOSER_LAYOUT.configTriggerClassName}
            label={t('acp.permissionMode')}
            value={permissionModeOverrideId}
            valueLabel={permissionModeLabel}
            options={availablePermissionModes}
            unspecifiedLabel={t('conversation.home.unspecifiedPermissionMode')}
            showUnspecified={canSelectUnspecifiedPermissionMode}
            onValueChange={handlePermissionModeSelect}
          />
        ) : (
          <Badge variant="outline" className={cn("max-w-full gap-1.5 rounded-full bg-background/50 font-normal", ACP_SESSION_COMPOSER_LAYOUT.staticConfigClassName)}>
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
    previous.onConfigOptionChange === next.onConfigOptionChange &&
    previous.onPermissionModeChange === next.onPermissionModeChange
  );
}

export function ACPSessionHeader({
  session,
  rawActive,
  rawLoading,
  showSystemPromptAction = true,
  showRawFramesAction = true,
  directSessionHeader,
  systemPromptAvailable,
  onToggleRaw,
  onOpenSystemPrompt,
}: {
  session: AcpSessionVm;
  rawActive: boolean;
  rawLoading: boolean;
  showSystemPromptAction?: boolean;
  showRawFramesAction?: boolean;
  directSessionHeader?: AcpDirectSessionHeaderProps;
  systemPromptAvailable?: boolean;
  onToggleRaw: () => void;
  onOpenSystemPrompt: () => void;
}) {
  const { t } = useTranslation();
  const [sessionIdTooltip, dispatchSessionIdTooltip] = useReducer(
    reduceAcpSessionIdTooltipState,
    ACP_SESSION_ID_TOOLTIP_INITIAL_STATE,
  );
  const copyFeedbackTimerRef = useRef<number | null>(null);
  const appWindowActiveRef = useRef(true);
  const hasSystemPrompt =
    systemPromptAvailable ?? Boolean(session.systemPromptAppend?.trim());

  const clearCopyFeedbackTimer = useCallback(() => {
    if (copyFeedbackTimerRef.current !== null) {
      window.clearTimeout(copyFeedbackTimerRef.current);
      copyFeedbackTimerRef.current = null;
    }
  }, []);

  useEffect(() => clearCopyFeedbackTimer, [clearCopyFeedbackTimer]);

  useEffect(() => {
    const handleWindowFocus = () => {
      appWindowActiveRef.current = true;
    };
    const handleWindowBlur = () => {
      appWindowActiveRef.current = false;
      clearCopyFeedbackTimer();
      dispatchSessionIdTooltip({ type: "app-deactivated" });
    };

    appWindowActiveRef.current = document.hasFocus();
    window.addEventListener("focus", handleWindowFocus);
    window.addEventListener("blur", handleWindowBlur);
    return () => {
      window.removeEventListener("focus", handleWindowFocus);
      window.removeEventListener("blur", handleWindowBlur);
    };
  }, [clearCopyFeedbackTimer]);

  const handleCopySessionId = useCallback(async () => {
    const sessionId = session.sessionId?.trim();
    if (!sessionId) return;

    try {
      await navigator.clipboard.writeText(sessionId);
    } catch {
      return;
    }

    clearCopyFeedbackTimer();
    dispatchSessionIdTooltip({ type: "copy-succeeded" });
    copyFeedbackTimerRef.current = window.setTimeout(() => {
      dispatchSessionIdTooltip({ type: "feedback-elapsed" });
      copyFeedbackTimerRef.current = window.setTimeout(() => {
        dispatchSessionIdTooltip({ type: "close-settled" });
        copyFeedbackTimerRef.current = null;
      }, SESSION_ID_TOOLTIP_CLOSE_SETTLE_MS);
    }, SESSION_ID_COPY_FEEDBACK_MS);
  }, [clearCopyFeedbackTimer, session.sessionId]);

  const handleSessionIdTooltipAnimationEnd = useCallback((event: AnimationEvent<HTMLDivElement>) => {
    if (
      event.currentTarget.dataset.state !== "closed" ||
      sessionIdTooltip.phase === "idle"
    ) return;

    clearCopyFeedbackTimer();
    dispatchSessionIdTooltip({ type: "close-settled" });
  }, [clearCopyFeedbackTimer, sessionIdTooltip.phase]);

  const handleSessionIdTriggerDisengaged = useCallback(() => {
    if (!appWindowActiveRef.current) return;
    dispatchSessionIdTooltip({ type: "trigger-disengaged" });
  }, []);

  return (
    <div className={cn(
      "shrink-0 border-b border-border/60 bg-content-header px-5",
      directSessionHeader ? "py-1.5" : "pb-1 pt-0",
    )}>
      <div className={cn(
        "flex min-w-0 items-center",
        directSessionHeader ? "gap-1" : "gap-1.5",
      )}>
        {directSessionHeader ? (
          <EditableConversationTitle
            title={directSessionHeader.title}
            className="mr-2 min-w-0 max-w-[40%] flex-none"
            showEditIcon={false}
            onTitleChange={directSessionHeader.onTitleChange}
          />
        ) : null}
        {session.adapterIconKey ? (
          <img
            src={agentIconSrc(session.adapterIconKey)}
            alt=""
            className={agentIconClass(session.adapterIconKey, "size-3.5 shrink-0")}
          />
        ) : (
          <Bot aria-hidden="true" className="size-3.5 shrink-0 text-muted-foreground" />
        )}
        <div className="flex min-w-0 items-baseline gap-1.5">
          <span className="min-w-0 truncate text-ui-compact font-medium leading-5 text-foreground/88">
            {session.adapterDisplayName ?? session.provider}
          </span>
          {session.sessionId ? (
            <Tooltip
              open={sessionIdTooltip.open}
              onOpenChange={(open) => dispatchSessionIdTooltip({ type: "open-changed", open })}
            >
              <TooltipTrigger asChild>
                <button
                  type="button"
                  className="min-w-0 truncate rounded px-1 py-0 text-ui-micro leading-5 text-muted-foreground/82 transition-colors hover:bg-muted/45 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                  aria-label={t("acp.copySessionId")}
                  onClick={handleCopySessionId}
                  onBlur={handleSessionIdTriggerDisengaged}
                  onPointerLeave={handleSessionIdTriggerDisengaged}
                >
                  {formatAcpSessionIdForDisplay(session.sessionId)}
                </button>
              </TooltipTrigger>
              <TooltipContent
                side="top"
                onAnimationEnd={handleSessionIdTooltipAnimationEnd}
              >
                {sessionIdTooltip.phase === "idle" ? session.sessionId : t("acp.sessionIdCopied")}
              </TooltipContent>
            </Tooltip>
          ) : null}
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-1.5">
          {showSystemPromptAction ? (
            <Button
              size="sm"
              variant="outline"
              className="h-5.5 gap-1 border-border/60 bg-background/22 px-2 text-ui-micro font-normal text-foreground/80 hover:bg-background/38"
              onClick={onOpenSystemPrompt}
              disabled={!hasSystemPrompt}
            >
              <FileText className="size-3" />
              {t("acp.systemPrompt")}
            </Button>
          ) : null}
          {showRawFramesAction ? (
            <Button
              size="sm"
              variant={rawActive ? "default" : "outline"}
              className={cn(
                "h-5.5 gap-1 px-2 text-ui-micro font-normal",
                rawActive
                  ? "bg-primary/18 text-foreground hover:bg-primary/24"
                  : "border-border/60 bg-background/22 text-foreground/80 hover:bg-background/38",
              )}
              onClick={onToggleRaw}
              disabled={rawLoading}
              data-acp-raw-frames-action="true"
            >
              {rawLoading ? <Loader2 className="size-3 animate-spin" /> : null}
              {t("acp.rawFrames")}
            </Button>
          ) : null}
        </div>
      </div>
    </div>
  );
}

const SESSION_ID_COPY_FEEDBACK_MS = 1200;
const SESSION_ID_TOOLTIP_CLOSE_SETTLE_MS = 200;
const SESSION_ID_DISPLAY_PREFIX_LENGTH = 8;
const SESSION_ID_DISPLAY_SUFFIX_LENGTH = 4;

type AcpSessionIdTooltipState = {
  open: boolean;
  phase: "idle" | "copied" | "closing";
  reopenBlocked: boolean;
};

type AcpSessionIdTooltipEvent =
  | { type: "open-changed"; open: boolean }
  | { type: "copy-succeeded" }
  | { type: "feedback-elapsed" }
  | { type: "close-settled" }
  | { type: "app-deactivated" }
  | { type: "trigger-disengaged" };

const ACP_SESSION_ID_TOOLTIP_INITIAL_STATE: AcpSessionIdTooltipState = {
  open: false,
  phase: "idle",
  reopenBlocked: false,
};

export function reduceAcpSessionIdTooltipState(
  state: AcpSessionIdTooltipState,
  event: AcpSessionIdTooltipEvent,
): AcpSessionIdTooltipState {
  switch (event.type) {
    case "open-changed":
      if (event.open && (state.phase !== "idle" || state.reopenBlocked)) return state;
      return { ...state, open: event.open };
    case "copy-succeeded":
      return { open: true, phase: "copied", reopenBlocked: true };
    case "feedback-elapsed":
      return state.phase === "copied"
        ? { ...state, open: false, phase: "closing" }
        : state;
    case "close-settled":
      return {
        open: false,
        phase: "idle",
        reopenBlocked: state.reopenBlocked,
      };
    case "app-deactivated":
      return {
        open: false,
        phase: "idle",
        reopenBlocked: true,
      };
    case "trigger-disengaged":
      return state.reopenBlocked
        ? { ...state, reopenBlocked: false }
        : state;
  }
}

export function formatAcpSessionIdForDisplay(sessionId: string) {
  const compactLength =
    SESSION_ID_DISPLAY_PREFIX_LENGTH + SESSION_ID_DISPLAY_SUFFIX_LENGTH + 1;
  if (sessionId.length <= compactLength) return sessionId;

  return `${sessionId.slice(0, SESSION_ID_DISPLAY_PREFIX_LENGTH)}…${sessionId.slice(-SESSION_ID_DISPLAY_SUFFIX_LENGTH)}`;
}

export function resolveRawFramesActionActive(workspaceScoped: boolean, canvasRawActive: boolean) {
  return !workspaceScoped && canvasRawActive;
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
  if (!open) return null;
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        overlayClassName="bg-black/16 backdrop-blur-md"
        className={ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.dialogContentClassName}
      >
        <DialogHeader className={ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.headerClassName}>
          <DialogTitle className="text-base">
            {t("acp.systemPromptTitle")}
          </DialogTitle>
        </DialogHeader>
        <SystemPromptPanel prompt={prompt} options={options} />
      </DialogContent>
    </Dialog>
  );
});

export const SystemPromptPanel = memo(function SystemPromptPanel({
  prompt,
  options,
  documentKey,
  resourceKind = "system-prompt",
  emptyMessage,
}: {
  prompt?: string | null;
  options?: Array<{ attemptId: string; prompt?: string | null }>;
  documentKey?: string;
  resourceKind?: string;
  emptyMessage?: string;
}) {
  const { t } = useTranslation();
  const availableOptions = useMemo(
    () => options?.filter((option) => option.prompt?.trim()) ?? [],
    [options],
  );
  const latestAttemptId = availableOptions.at(-1)?.attemptId ?? null;
  const [selectedAttemptId, setSelectedAttemptId] = useState<string | null>(latestAttemptId);
  const [viewMode, setViewMode] = useState(loadSystemPromptViewMode);
  useEffect(() => setSelectedAttemptId(latestAttemptId), [latestAttemptId]);
  const selectedPrompt = availableOptions.find((option) => option.attemptId === selectedAttemptId)?.prompt;
  const content = (selectedPrompt ?? prompt)?.trim() || "";
  const onViewModeChange = (nextMode: SystemPromptViewMode) => {
    setViewMode(nextMode);
    saveSystemPromptViewMode(nextMode);
  };
  return (
    <div className={ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.scrollContainerClassName} data-right-workspace-resource={resourceKind}>
      <div className={ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.bodyClassName}>
        <div className={ACP_SYSTEM_PROMPT_DIALOG_LAYOUT.attemptSelectorClassName}>
          {availableOptions.length > 1 ? (
            <Select value={selectedAttemptId ?? availableOptions[0]?.attemptId} onValueChange={setSelectedAttemptId}>
              <SelectTrigger className="h-8 w-[220px] max-w-full"><SelectValue /></SelectTrigger>
              <SelectContent>
                {availableOptions.map((option) => (
                  <SelectItem value={option.attemptId} key={option.attemptId}>{option.attemptId}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : null}
        </div>
        {content ? (
          <ReadonlyMarkdownDocument
            documentKey={documentKey ?? `system-prompt:${selectedAttemptId ?? "current"}`}
            content={content}
            viewMode={viewMode}
            onViewModeChange={onViewModeChange}
          />
        ) : (
          <div className="flex h-full items-center justify-center p-5"><div className="rounded-xl border border-dashed bg-muted/10 p-6 text-sm text-muted-foreground">{emptyMessage ?? t("acp.systemPromptEmpty")}</div></div>
        )}
      </div>
    </div>
  );
});

function ReadonlyMarkdownDocument({
  documentKey,
  content,
  viewMode,
  onViewModeChange,
}: {
  documentKey: string;
  content: string;
  viewMode: SystemPromptViewMode;
  onViewModeChange: (mode: SystemPromptViewMode) => void;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const rendered = viewMode === SYSTEM_PROMPT_VIEW_MODES.rendered;
  useEffect(() => () => {
    if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
  }, []);
  const copySource = async () => {
    await navigator.clipboard.writeText(content);
    setCopied(true);
    if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
    copiedTimerRef.current = setTimeout(() => setCopied(false), 1_500);
  };
  const toggleLabel = rendered
    ? t("workspace.filesPanel.viewMarkdownSource")
    : t("acp.renderMarkdown");
  return (
    <div
      className="relative h-full min-h-0 min-w-0"
      data-readonly-markdown-mode={viewMode}
    >
      <div
        className="absolute right-2 top-2 z-20 flex items-center gap-0.5 rounded-md border border-border/50 bg-background/88 p-0.5 shadow-sm backdrop-blur"
        data-readonly-markdown-toolbar="true"
      >
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              size="icon"
              variant="ghost"
              className="size-6"
              onClick={() => void copySource()}
              aria-label={t(copied ? "acp.markdownSourceCopied" : "acp.copyMarkdownSource")}
            >
              {copied ? <Check className="size-3 text-emerald-600" /> : <Copy className="size-3" />}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{t(copied ? "acp.markdownSourceCopied" : "acp.copyMarkdownSource")}</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              size="icon"
              variant="ghost"
              className="size-6"
              onClick={() => onViewModeChange(rendered
                ? SYSTEM_PROMPT_VIEW_MODES.raw
                : SYSTEM_PROMPT_VIEW_MODES.rendered)}
              aria-label={toggleLabel}
            >
              {rendered ? <Code2 className="size-3" /> : <Eye className="size-3" />}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{toggleLabel}</TooltipContent>
        </Tooltip>
      </div>
      {rendered ? (
        <div className={goldThemedScrollbarClassName("h-full overflow-y-auto px-6 py-5 pr-12")}>
          <Markdown className="mx-auto w-full max-w-4xl" streaming={false}>{content}</Markdown>
        </div>
      ) : (
        <WorkspaceFileEditor
          documentKey={documentKey}
          value={content}
          editable={false}
          language="markdown"
          highlight
          contentRevision={0}
          target={null}
          targetRevision={0}
          onChange={() => undefined}
          onSave={() => undefined}
          initialStateJson={null}
          onPersistState={() => undefined}
          markdownMode={null}
        />
      )}
    </div>
  );
}

export function ACPMessageList({
  timeline,
  sessionStatus,
  sending,
  branchLocator,
  timelineGeneration = 0,
  streamingMarkdownItemKey = null,
}: {
  timeline: AcpTimelineItem[];
  sessionStatus: string;
  sending: boolean;
  branchLocator?: AgentTranscriptLocator;
  timelineGeneration?: number;
  streamingMarkdownItemKey?: string | null;
  onLayoutChange?: () => void;
}) {
  const active = isSessionActiveStatus(sessionStatus) || sending;
  const sessionId = timelineSessionId(timeline);
  const standaloneWindowKey = branchLocator
    ? createAcpSessionCacheKey(
        "message-list",
        branchLocator.taskId,
        branchLocator.runId,
        branchLocator.roundId,
        branchLocator.nodeId,
        branchLocator.attemptId,
        branchLocator.projectId,
        branchLocator.outerNodeId,
        branchLocator.outerAttemptId,
        branchLocator.branchId,
      )
    : "message-list:standalone";
  const timelineWindowOwner = useMemo<AcpTimelineWindowOwner>(() => ({
    eventWindowKey: standaloneWindowKey,
    sessionId,
    timelineGeneration,
  }), [sessionId, standaloneWindowKey, timelineGeneration]);
  if (timeline.length === 0) return active ? null : <EmptyAcpState />;

  const content = (
    <AcpTimelineWindowOwnerContext.Provider
      key={timelineWindowRenderScopeKey(standaloneWindowKey, sessionId)}
      value={timelineWindowOwner}
    >
      <div className="min-w-0 space-y-1">
        {timeline.map((item) => (
          <ACPTimelineItemRenderer
            key={timelineEventKey(item)}
            event={item}
            streamingMarkdownItemKey={streamingMarkdownItemKey}
          />
        ))}
      </div>
    </AcpTimelineWindowOwnerContext.Provider>
  );
  return branchLocator ? (
    <AcpBranchLocatorContext.Provider value={branchLocator}>
      {content}
    </AcpBranchLocatorContext.Provider>
  ) : content;
}

function timelineSessionId(timeline: AcpTimelineItem[]) {
  for (const item of timeline) {
    if (isActivityBatch(item) && item.sessionId) return item.sessionId;
    if (isAgentLink(item) && item.toolEvent.sessionId) return item.toolEvent.sessionId;
    if (!isActivityBatch(item) && !isAgentLink(item) && item.sessionId) return item.sessionId;
  }
  return null;
}

function EmptyAcpState() {
  const { t } = useTranslation();
  return (
    <p data-acp-empty-state="true" className="py-8 text-center text-sm text-muted-foreground">
      {t("acp.noEvents")}
    </p>
  );
}

function AcpPendingTimelineState({ label }: { label: string }) {
  return (
    <BrandLoadingState
      label={label}
      surface="transparent"
      className="min-h-[10rem]"
      logoClassName="w-14"
    />
  );
}

function AttemptSeparator({ event }: { event: AcpTimelineEvent }) {
  const { t } = useTranslation();
  const boundaryKind = stringValue(rawObject(event.raw)?.boundaryKind);
  const label = boundaryKind === "stopped"
    ? t("acp.attemptStopped")
    : boundaryKind === "continued"
      ? t("acp.attemptContinued")
      : event.title ?? event.content ?? "attempt";
  return (
    <div className="flex items-center gap-3 py-1 text-xs text-muted-foreground">
      <span className="h-px flex-1 bg-border/70" />
      <span className="rounded-full border bg-background/90 px-3 py-1 font-mono text-ui-micro uppercase tracking-[0.12em]">
        {label}
      </span>
      <span className="h-px flex-1 bg-border/70" />
    </div>
  );
}

const ACPTimelineItemRenderer = memo(function ACPTimelineItemRenderer({
  event,
  streamingMarkdownItemKey,
  messageAttachmentLocator,
  onMessageAttachmentClick,
  nested = false,
}: {
  event: AcpTimelineItem;
  streamingMarkdownItemKey?: string | null;
  messageAttachmentLocator?: MessageAttachmentLocator;
  onMessageAttachmentClick?: (att: MessageAttachmentPreview) => void;
  nested?: boolean;
}) {
  const branchLocator = useContext(AcpBranchLocatorContext);
  if (
    (event.kind === "textDelta" || event.kind === "thoughtDelta")
    && !hasVisibleAcpTextContent(event.content)
  ) return null;
  if (isAgentLink(event))
    return nested ? (
      <AgentLinkRow event={event} />
    ) : (
      <AssistantTimelineRow timestamp={event.timestamp ?? event.startedAt}>
        <AgentLinkRow event={event} />
      </AssistantTimelineRow>
    );
  if (event.kind === "attemptSeparator")
    return <AttemptSeparator event={event} />;
  if (event.kind === "contextCompaction")
    return nested ? (
      <ContextCompactionRow event={event} />
    ) : (
      <AssistantTimelineRow timestamp={event.timestamp ?? event.startedAt}>
        <ContextCompactionRow event={event} />
      </AssistantTimelineRow>
    );
  if (event.kind === "fileChangeSet")
    return nested ? (
      <TurnFileChangesCard event={event} locator={branchLocator} />
    ) : (
      <AssistantTimelineRow timestamp={event.timestamp ?? event.startedAt}>
        <TurnFileChangesCard event={event} locator={branchLocator} />
      </AssistantTimelineRow>
    );
  if (isActivityBatch(event))
    return (
      <AcpActivityBatchRow
        event={event}
        nested={nested}
      />
    );
  if (event.kind === "textDelta" || event.kind === "userTextDelta")
    return <MessageBubble event={event} streamingMarkdownItemKey={streamingMarkdownItemKey} messageAttachmentLocator={messageAttachmentLocator} onMessageAttachmentClick={onMessageAttachmentClick} nested={nested} />;
  if (event.kind === "thoughtDelta")
    return <ThoughtBlock event={event} streamingMarkdownItemKey={streamingMarkdownItemKey} nested={nested} />;
  if (event.kind === "toolCall" || event.kind === "toolCallUpdate")
    return <ToolBlock event={event} nested={nested} />;
  return null;
});

const ContextCompactionRow = memo(function ContextCompactionRow({
  event,
}: {
  event: AcpTimelineEvent;
}) {
  const { t } = useTranslation();
  const running = event.status === "running";
  const interrupted = event.status === "interrupted";
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [running]);

  const usageBefore = contextCompactionUsageBefore(event);
  const startedAt = parseAcpTimestamp(event.startedAt ?? event.timestamp);
  const endedAt = parseAcpTimestamp(event.endedAt);
  const elapsedSeconds = startedAt == null
    ? null
    : Math.max(0, Math.floor(((running ? now : (endedAt ?? startedAt)) - startedAt) / 1_000));
  const delayed = running
    && elapsedSeconds != null
    && elapsedSeconds >= CONTEXT_COMPACTION_DELAYED_AFTER_SECONDS;
  const label = running
    ? t("acp.compactionRunning")
    : interrupted
      ? t("acp.compactionInterrupted")
      : t("acp.compactionCompleted");
  const usage = usageBefore
    ? t("acp.compactionUsageBefore", usageBefore)
    : null;

  return (
    <div
      role="status"
      aria-live="polite"
      aria-atomic="true"
      className="min-w-0 py-1"
    >
      <div data-theme-role="activity" className="w-full border-l-2 border-foreground/10 px-3 py-2">
        <div className="flex min-w-0 items-center gap-2 text-sm">
          <span
            aria-hidden="true"
            className={cn(
              "flex size-5 shrink-0 items-center justify-center rounded-full text-ui-caption font-semibold",
              running && "border-2 border-gold-running/30 border-t-gold-running text-transparent animate-spin motion-reduce:animate-none",
              !running && !interrupted && "bg-emerald-500/12 text-emerald-700 dark:text-emerald-300",
              interrupted && "bg-destructive/10 text-destructive",
            )}
          >
            {running ? "" : interrupted ? "!" : "✓"}
          </span>
          <span className="min-w-0 truncate font-medium text-foreground">
            {label}
          </span>
          {elapsedSeconds != null ? (
            <span className="ml-1 shrink-0 tabular-nums text-xs text-muted-foreground">
              {formatElapsedDuration(elapsedSeconds)}
            </span>
          ) : null}
        </div>
        {usage || delayed ? (
          <div className="mt-1 flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted-foreground">
            {usage ? <span className="tabular-nums">{usage}</span> : null}
            {delayed ? <span>{t("acp.compactionDelayed")}</span> : null}
          </div>
        ) : null}
        {running ? (
          <div className="mt-2 h-0.5 w-full max-w-72 overflow-hidden rounded-full bg-primary/10">
            <div className="h-full w-1/2 animate-pulse rounded-full bg-primary/55 motion-reduce:animate-none" />
          </div>
        ) : null}
      </div>
    </div>
  );
});

const AgentLinkRow = memo(function AgentLinkRow({ event }: { event: AcpAgentLink }) {
  const { t } = useTranslation();
  const branchLocator = useContext(AcpBranchLocatorContext);
  const workspace = useOptionalRightWorkspaceCommands();
  const liveSnapshot = useConversationBranchLiveSnapshot(
    branchLocator ?? { projectId: 'unavailable', taskId: '', runId: '', roundId: '', nodeId: '', attemptId: '' },
    event.agentExecutionId,
  );
  const input = agentToolInput(event.toolEvent);
  const description = event.description ?? input.description;
  const label = description || event.title;
  const metricsSummary = agentExecutionMetricsSummary(event, t);
  const displayedStatus = resolveConversationBranchDisplayStatus(event.status, liveSnapshot.status);
  const attention = liveSnapshot.revision > 0 ? liveSnapshot.attention : event.attention;
  const statusTone = toolStatusTone(displayedStatus);
  const statusLabel = childAgentStatusLabel(displayedStatus, t);
  const canOpen = Boolean(branchLocator && workspace?.scopeKey && !event.agentExecutionId.startsWith('unresolved-'));
  const openAgent = () => {
    if (!branchLocator || !workspace?.scopeKey || !canOpen) return;
    const locator: AgentTranscriptLocator = {
      ...branchLocator,
      attemptId: event.attemptId ?? branchLocator.attemptId,
      branchId: event.agentExecutionId,
    };
    workspace.openResource({
      kind: 'agent-transcript',
      key: agentTranscriptResourceKey(locator),
      scopeKey: workspace.scopeKey,
      title: label || t('acp.subAgent'),
      description,
      status: displayedStatus ?? 'queued',
      attention,
      locator,
    });
  };
  return (
    <Button
      type="button"
      variant="ghost"
      data-agent-link-branch-id={event.agentExecutionId}
      data-agent-link-status={displayedStatus}
      disabled={!canOpen}
      className="group h-auto min-h-10 w-full min-w-0 justify-start gap-3 rounded-lg px-2 py-2 text-left font-normal hover:bg-muted/30 disabled:cursor-default disabled:opacity-100"
      onClick={openAgent}
    >
      <AcpAvatar
        tone="assistant"
        className={cn(
          'mt-0 size-7',
          attention && 'border-amber-500/45 ring-2 ring-amber-500/15',
        )}
      />
      <span className="min-w-0 flex-1">
        <span className="flex min-w-0 items-center gap-2">
          <span className="shrink-0 text-xs font-medium text-foreground">{t('acp.subAgent')}</span>
          {label ? <span className="min-w-0 truncate text-sm text-foreground/90">{label}</span> : null}
        </span>
        {metricsSummary ? <span className="mt-0.5 block truncate text-xs text-muted-foreground">{metricsSummary}</span> : null}
      </span>
      <span className={cn(
        'shrink-0 rounded-full px-2 py-0.5 text-ui-caption font-medium',
        statusTone === 'danger' && 'bg-destructive/10 text-destructive',
        statusTone === 'success' && 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300',
        statusTone === 'running' && 'bg-primary/10 text-primary',
        statusTone === 'muted' && 'bg-muted text-muted-foreground',
      )}>{statusLabel}</span>
      <ChevronDown className="size-3.5 shrink-0 -rotate-90 text-muted-foreground transition-transform group-hover:translate-x-0.5" />
    </Button>
  );
});

type AgentExecutionMetrics = Pick<
  AcpAgentExecutionVm,
  "toolCallCount" | "readFileCount" | "writtenFileCount"
>;

function agentExecutionMetricsSummary(
  execution: AgentExecutionMetrics,
  t: ReturnType<typeof useTranslation>["t"],
) {
  const parts: string[] = [];
  if (execution.toolCallCount > 0) {
    parts.push(t("acp.activityToolCount", { count: execution.toolCallCount }));
  }
  if (execution.readFileCount > 0) {
    parts.push(t("acp.activityReadFiles", { count: execution.readFileCount }));
  }
  if (execution.writtenFileCount > 0) {
    parts.push(t("acp.activityWrittenFiles", { count: execution.writtenFileCount }));
  }
  return parts.join(" · ");
}

const AgentBranchSessionSummary = memo(function AgentBranchSessionSummary({
  execution,
  status,
  elapsedSeconds,
}: {
  execution: AcpAgentExecutionVm;
  status: string;
  elapsedSeconds?: number | null;
}) {
  const { t } = useTranslation();
  const displayedStatus = status || execution.executionStatus;
  const tone = toolStatusTone(displayedStatus);
  const metrics = agentExecutionMetricsSummary(execution, t);
  return (
    <div
      className="flex min-h-8 shrink-0 min-w-0 flex-wrap items-center gap-x-3 gap-y-1 border-b border-border/45 bg-muted/10 px-5 py-1.5 text-xs text-muted-foreground"
      data-agent-branch-summary="true"
      data-agent-branch-status={displayedStatus}
      data-agent-branch-tool-count={execution.toolCallCount}
      data-agent-branch-read-file-count={execution.readFileCount}
      data-agent-branch-written-file-count={execution.writtenFileCount}
    >
      <span className="flex shrink-0 items-center gap-1.5 font-medium text-foreground/85">
        <span
          aria-hidden="true"
          className={cn(
            "size-1.5 rounded-full bg-muted-foreground/50",
            tone === "running" && "animate-pulse bg-primary motion-reduce:animate-none",
            tone === "success" && "bg-emerald-500",
            tone === "danger" && "bg-destructive",
          )}
        />
        {childAgentStatusLabel(displayedStatus, t)}
      </span>
      {elapsedSeconds != null ? (
        <span className="shrink-0 tabular-nums">
          {formatElapsedDuration(elapsedSeconds)}
        </span>
      ) : null}
      {metrics ? <span className="min-w-0 truncate">{metrics}</span> : null}
    </div>
  );
});

type AcpActivityDetailWindow = {
  ownerKey: string;
  activityEndSeq: number;
  totalEventCount: number;
  events: AcpTimelineEvent[];
  detailLoaded: boolean;
  hasMoreEarlier: boolean;
  earlierCursor: string | null;
  hasNewer: boolean;
  error: {
    scopeKey: string;
    message: string;
    cursor: string | null;
    replaceWithLatest: boolean;
  } | null;
};

type AcpDetailRequestToken = {
  requestSeq: number;
  ownerKey: string;
  scopeKey: string;
  observedRevision: number;
};

type AcpActivityDetailRequestToken = AcpDetailRequestToken & {
  anchor: { key: string; top: number } | null;
  sessionId: string;
};

type AcpToolDetailRequestToken = AcpDetailRequestToken & {
  eventId: string;
  toolCallId: string | null;
  sessionId: string;
  sourceSignature: string;
  sourceStatus: string | null;
  sourceContent: string | null;
  sourceTitle: string | null;
};

type AcpToolDetailState = {
  ownerKey: string;
  observedRevision: number;
  sourceSignature: string;
  sourceStatus: string | null;
  sourceContent: string | null;
  sourceTitle: string | null;
  event: AcpTimelineEvent;
};

const AcpActivityBatchRow = memo(function AcpActivityBatchRow({
  event,
  nested = false,
}: {
  event: AcpActivityBatch;
  nested?: boolean;
}) {
  const { t } = useTranslation();
  const branchLocator = useContext(AcpBranchLocatorContext);
  const timelineWindowOwner = useContext(AcpTimelineWindowOwnerContext);
  const contentExpansion = useOptionalChatContainerContentExpansion();
  const sessionId = timelineWindowOwner?.sessionId ?? event.sessionId ?? null;
  const ownerKey = acpDetailOwnerKey(
    branchLocator,
    timelineWindowOwner,
    sessionId,
    `activity:${event.activityStartSeq}`,
  );
  const requestScopeKey = acpDetailRequestScopeKey(
    ownerKey,
    timelineWindowOwner?.timelineGeneration ?? 0,
    event.activityEndSeq,
  );
  const [open, setOpen] = useState(false);
  const [detailWindow, setDetailWindow] = useState<AcpActivityDetailWindow>(() => (
    createAcpActivityDetailWindow(event, ownerKey)
  ));
  const activeDetailWindow = detailWindow.ownerKey === ownerKey
    && detailWindow.activityEndSeq === event.activityEndSeq
    && detailWindow.totalEventCount === event.totalEventCount
      ? detailWindow
      : syncAcpActivityDetailWindow(detailWindow, event, ownerKey);
  const [loadingRequestScope, setLoadingRequestScope] = useState<string | null>(null);
  const activeDetailRequestRef = useRef<AcpActivityDetailRequestToken | null>(null);
  const trailingDetailRequestRef = useRef<{
    cursor: string | null;
    replaceWithLatest: boolean;
  } | null>(null);
  const latestLoadDetailRef = useRef<(
    cursor: string | null,
    replaceWithLatest?: boolean,
  ) => Promise<void>>(async () => {});
  const detailRequestSeqRef = useRef(0);
  const mountedRef = useRef(true);
  const openRef = useRef(open);
  const currentRequestScopeRef = useRef(requestScopeKey);
  const currentEventRef = useRef(event);
  openRef.current = open;
  currentRequestScopeRef.current = requestScopeKey;
  currentEventRef.current = event;
  const triggerRef = useRef<HTMLButtonElement>(null);
  const detailListRef = useRef<HTMLDivElement>(null);
  const pendingDetailAnchorRef = useRef<{ key: string; top: number } | null>(null);
  const disclosureTokenRef = useRef<ChatContainerContentExpansionToken | null>(null);
  const contentExpansionRef = useRef(contentExpansion);
  contentExpansionRef.current = contentExpansion;
  const detailRequestInFlight = loadingRequestScope !== null;
  const loadingEarlier = loadingRequestScope === requestScopeKey;
  const activeDetailError = activeDetailWindow.error?.scopeKey === requestScopeKey
    ? activeDetailWindow.error
    : null;
  const summary = activityBatchSummary(event, t);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      activeDetailRequestRef.current = null;
      trailingDetailRequestRef.current = null;
      const token = disclosureTokenRef.current;
      disclosureTokenRef.current = null;
      if (token !== null) contentExpansionRef.current?.endContentExpansion(token);
    };
  }, []);
  useEffect(() => {
    setDetailWindow((current) => syncAcpActivityDetailWindow(current, event, ownerKey));
  }, [
    event.detailAvailable,
    event.earlierCursor,
    event.events,
    event.hasMoreEarlier,
    event.totalEventCount,
    ownerKey,
  ]);
  useLayoutEffect(() => {
    const anchor = pendingDetailAnchorRef.current;
    if (!anchor) return;
    pendingDetailAnchorRef.current = null;
    const item = findAcpActivityDetailItem(detailListRef.current, anchor.key);
    if (!item) return;
    contentExpansion?.compensateContentAnchor(
      item.getBoundingClientRect().top - anchor.top,
    );
  }, [activeDetailWindow.events, contentExpansion]);
  const ownsDetailRequest = (token: AcpDetailRequestToken) => {
    const active = activeDetailRequestRef.current;
    return Boolean(
      mountedRef.current
      && active
      && active.requestSeq === token.requestSeq
      && active.scopeKey === token.scopeKey
      && currentRequestScopeRef.current === token.scopeKey,
    );
  };
  const loadDetail = async (cursor: string | null, replaceWithLatest = false) => {
    if (!branchLocator || !sessionId?.trim()) return;
    const activeRequest = activeDetailRequestRef.current;
    if (activeRequest) {
      if (activeRequest.scopeKey !== requestScopeKey) {
        trailingDetailRequestRef.current = { cursor, replaceWithLatest };
      }
      return;
    }
    if (cursor === null && activeDetailWindow.detailLoaded && !replaceWithLatest) return;
    const requestEvent = event;
    const token: AcpActivityDetailRequestToken = {
      requestSeq: detailRequestSeqRef.current + 1,
      ownerKey,
      scopeKey: requestScopeKey,
      observedRevision: requestEvent.activityEndSeq,
      sessionId,
      anchor: cursor === null || replaceWithLatest
        ? null
        : captureVisibleAcpActivityDetailAnchor(
            contentExpansion?.scrollRef.current ?? null,
            detailListRef.current,
          ),
    };
    detailRequestSeqRef.current = token.requestSeq;
    activeDetailRequestRef.current = token;
    setLoadingRequestScope(token.scopeKey);
    setDetailWindow((current) => {
      const active = current.ownerKey === ownerKey
        ? current
        : createAcpActivityDetailWindow(requestEvent, ownerKey);
      return active.error === null ? active : { ...active, error: null };
    });
    try {
      const detail = await getAcpActivityDetail(
        branchLocator.projectId,
        branchLocator.taskId,
        branchLocator.runId,
        branchLocator.roundId,
        branchLocator.nodeId,
        branchLocator.attemptId,
        {
          branchId: branchLocator.branchId,
          sessionId: token.sessionId,
          activityStartSeq: requestEvent.activityStartSeq,
          activityEndSeq: requestEvent.activityEndSeq,
          earlierCursor: cursor,
          limit: ACP_ACTIVITY_DETAIL_PAGE_SIZE,
        },
        branchLocator.outerNodeId,
        branchLocator.outerAttemptId,
      );
      if (!ownsDetailRequest(token)) return;
      if (!acpActivityDetailBelongsToRequest(
        detail.items,
        token.sessionId,
        requestEvent,
      )) return;
      if (token.anchor) pendingDetailAnchorRef.current = token.anchor;
      const currentEvent = currentEventRef.current;
      setDetailWindow((current) => mergeAcpActivityDetailPage({
        current,
        event: currentEvent,
        ownerKey,
        items: detail.items.filter(isVisibleActivityAuditEvent) as AcpTimelineEvent[],
        hasMoreEarlier: detail.hasMoreEarlier,
        earlierCursor: detail.earlierCursor ?? null,
        loadingEarlierPage: cursor !== null && !replaceWithLatest,
      }));
    } catch (error) {
      if (!ownsDetailRequest(token)) return;
      setDetailWindow((current) => {
        const active = current.ownerKey === ownerKey
          ? current
          : createAcpActivityDetailWindow(currentEventRef.current, ownerKey);
        return {
          ...active,
          error: {
            scopeKey: token.scopeKey,
            message: displayAppError(t, error),
            cursor,
            replaceWithLatest,
          },
        };
      });
    } finally {
      const active = activeDetailRequestRef.current;
      if (active === token) {
        activeDetailRequestRef.current = null;
        if (mountedRef.current) setLoadingRequestScope(null);
        const trailing = trailingDetailRequestRef.current;
        trailingDetailRequestRef.current = null;
        if (mountedRef.current && openRef.current && trailing) {
          void latestLoadDetailRef.current(
            trailing.cursor,
            trailing.replaceWithLatest,
          );
        }
      }
    }
  };
  latestLoadDetailRef.current = loadDetail;
  useEffect(() => {
    if (
      !open
      || activeDetailWindow.detailLoaded
      || !event.detailAvailable
      || activeDetailError
      || activeDetailRequestRef.current?.scopeKey === requestScopeKey
    ) return;
    void loadDetail(null);
  }, [
    activeDetailWindow.detailLoaded,
    activeDetailError,
    event.detailAvailable,
    open,
    requestScopeKey,
  ]);
  const handleOpenChange = (next: boolean) => {
    openRef.current = next;
    if (!next) trailingDetailRequestRef.current = null;
    let restoringBottom = false;
    if (next) {
      disclosureTokenRef.current = contentExpansion?.beginContentExpansion() ?? null;
    } else {
      const token = disclosureTokenRef.current;
      disclosureTokenRef.current = null;
      restoringBottom = contentExpansion?.endContentExpansion(token) ?? false;
    }
    setOpen(next);
    if (next && !activeDetailWindow.detailLoaded && event.detailAvailable) {
      void loadDetail(null);
    }
    if (!next && !restoringBottom) {
      requestAnimationFrame(() => {
        triggerRef.current?.scrollIntoView?.({ block: "nearest" });
      });
    }
  };
  return (
    <AssistantTimelineRow timestamp={event.timestamp} nested={nested}>
      <Collapsible
        data-theme-role="activity"
        open={open}
        onOpenChange={handleOpenChange}
        className="min-w-0 max-w-full"
      >
        <CollapsibleTrigger asChild>
          <Button
            ref={triggerRef}
            variant="ghost"
            className="h-auto min-h-7 w-full min-w-0 justify-start gap-1.5 rounded-none bg-transparent px-1 py-0.5 text-left font-normal text-muted-foreground hover:bg-transparent hover:text-foreground focus-visible:bg-transparent focus-visible:text-foreground data-[state=open]:bg-transparent data-[state=open]:text-foreground"
          >
            {event.live ? (
              <AcpProcessingSpinner className="size-3.5" />
            ) : (
              <ChevronDown
                className={cn(
                  "size-3.5 shrink-0 transition-transform",
                  open && "rotate-180",
                )}
              />
            )}
            <span
              className={cn(
                "min-w-0 flex-1 truncate text-xs leading-5",
                event.live && "acp-activity-live-label font-medium text-foreground/85",
              )}
            >
              {summary}
            </span>
          </Button>
        </CollapsibleTrigger>
        {open ? (
          <CollapsibleContent className="min-w-0 max-w-full overflow-hidden">
            <div className="ml-2 min-w-0 max-w-full space-y-1 border-l border-border/55 py-1 pl-3">
              <div className="flex min-w-0 flex-wrap items-center gap-1">
                {activeDetailWindow.hasMoreEarlier && activeDetailWindow.earlierCursor ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-xs text-muted-foreground"
                    disabled={detailRequestInFlight}
                    onClick={() => void loadDetail(activeDetailWindow.earlierCursor)}
                  >
                    {loadingEarlier ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
                    {t("acp.activityShowEarlier", {
                      count: Math.max(0, event.totalEventCount - activeDetailWindow.events.length),
                    })}
                  </Button>
                ) : null}
                {activeDetailWindow.hasNewer ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-xs text-muted-foreground"
                    disabled={detailRequestInFlight}
                    onClick={() => void loadDetail(null, true)}
                    data-acp-activity-return-to-latest="true"
                  >
                    {loadingEarlier ? <Loader2 className="mr-1 size-3 animate-spin" /> : null}
                    {t("acp.activityReturnToLatest")}
                  </Button>
                ) : null}
              </div>
              {loadingEarlier && activeDetailWindow.events.length === 0 ? (
                <div className="flex h-8 items-center gap-2 px-2 text-xs text-muted-foreground">
                  <Loader2 className="size-3 animate-spin" />
                  {t("common.loading")}
                </div>
              ) : null}
              {activeDetailError ? (
                <div className="flex min-w-0 items-center justify-between gap-2 px-2 py-1 text-xs text-destructive">
                  <span className="min-w-0 truncate">{activeDetailError.message}</span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-7 shrink-0 px-2 text-xs"
                    disabled={detailRequestInFlight}
                    onClick={() => void loadDetail(
                      activeDetailError.cursor,
                      activeDetailError.replaceWithLatest,
                    )}
                    data-acp-activity-detail-retry="true"
                  >
                    {t("common.retry")}
                  </Button>
                </div>
              ) : null}
              <div ref={detailListRef} className="min-w-0 space-y-1">
                {activeDetailWindow.events.map((activity) => (
                  <div
                    key={timelineEventKey(activity)}
                    data-acp-activity-detail-item-key={timelineEventKey(activity)}
                  >
                    <ActivityAuditEvent event={activity} />
                  </div>
                ))}
              </div>
              <div className="flex justify-end px-1 pt-1">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="acp-activity-collapse-button h-7 gap-1.5 px-2 text-xs font-normal text-muted-foreground hover:bg-muted/40 hover:text-foreground"
                  onClick={() => handleOpenChange(false)}
                >
                  <ChevronDown className="size-3.5 rotate-180" aria-hidden="true" />
                  {t("acp.activityCollapse")}
                </Button>
              </div>
            </div>
          </CollapsibleContent>
        ) : null}
      </Collapsible>
    </AssistantTimelineRow>
  );
});

const ActivityAuditEvent = memo(function ActivityAuditEvent({
  event,
}: {
  event: AcpTimelineEvent;
}) {
  if (event.kind === "thoughtDelta") {
    return <ThoughtBlock event={event} nested compact />;
  }
  return <ToolBlock event={event} nested compact />;
});

function isVisibleActivityAuditEvent(event: Pick<AcpUiEventVm, "kind">) {
  return event.kind !== "permissionRequest" && event.kind !== "activitySummary";
}

function hasCompleteLocalActivityDetail(event: AcpActivityBatch) {
  if (!event.detailAvailable) return true;
  const localEventCount = event.events.filter(isVisibleActivityAuditEvent).length;
  return localEventCount >= event.totalEventCount;
}

function acpDetailOwnerKey(
  branchLocator: AgentTranscriptLocator | null,
  timelineWindowOwner: AcpTimelineWindowOwner | null,
  sessionId: string | null | undefined,
  itemKey: string,
) {
  const eventWindowKey = timelineWindowOwner?.eventWindowKey
    ?? (branchLocator
      ? [
          branchLocator.projectId,
          branchLocator.taskId,
          branchLocator.runId,
          branchLocator.roundId,
          branchLocator.outerNodeId ?? "root",
          branchLocator.outerAttemptId ?? "root",
          branchLocator.nodeId,
          branchLocator.attemptId,
          branchLocator.branchId,
        ].join(":" )
      : "unscoped");
  return `${timelineWindowRenderScopeKey(eventWindowKey, sessionId)}:${itemKey}`;
}

function acpDetailRequestScopeKey(
  ownerKey: string,
  timelineGeneration: number,
  observedRevision: number,
) {
  return `${ownerKey}:generation:${timelineGeneration}:revision:${observedRevision}`;
}

function createAcpActivityDetailWindow(
  event: AcpActivityBatch,
  ownerKey: string,
): AcpActivityDetailWindow {
  const events = limitAcpEvents(
    event.events.filter(isVisibleActivityAuditEvent),
    "start",
    ACP_ACTIVITY_DETAIL_WINDOW_LIMIT,
  ) as AcpTimelineEvent[];
  return {
    ownerKey,
    activityEndSeq: event.activityEndSeq,
    totalEventCount: event.totalEventCount,
    events,
    detailLoaded: hasCompleteLocalActivityDetail(event),
    hasMoreEarlier: event.hasMoreEarlier,
    earlierCursor: event.earlierCursor ?? null,
    hasNewer: false,
    error: null,
  };
}

function syncAcpActivityDetailWindow(
  current: AcpActivityDetailWindow,
  event: AcpActivityBatch,
  ownerKey: string,
): AcpActivityDetailWindow {
  if (current.ownerKey !== ownerKey) {
    return createAcpActivityDetailWindow(event, ownerKey);
  }
  // A historical detail window is an intentional reading projection. Live
  // tail changes are acknowledged by the parent timeline but are not inserted
  // into this DOM until the user explicitly returns to the latest page.
  if (current.hasNewer) {
    return {
      ...current,
      activityEndSeq: event.activityEndSeq,
      totalEventCount: event.totalEventCount,
    };
  }

  const canonicalRangeChanged = current.activityEndSeq !== event.activityEndSeq
    || current.totalEventCount !== event.totalEventCount;
  const merged = mergeAcpEvents(
    current.events,
    event.events.filter(isVisibleActivityAuditEvent),
  ) as AcpTimelineEvent[];
  const events = limitAcpEvents(
    merged,
    "start",
    ACP_ACTIVITY_DETAIL_WINDOW_LIMIT,
  ) as AcpTimelineEvent[];
  const detailLoaded = hasCompleteLocalActivityDetail(event)
    || (current.detailLoaded && !canonicalRangeChanged);
  return {
    ...current,
    activityEndSeq: event.activityEndSeq,
    totalEventCount: event.totalEventCount,
    events,
    detailLoaded,
    hasMoreEarlier: detailLoaded && !canonicalRangeChanged
      ? current.hasMoreEarlier
      : event.hasMoreEarlier,
    earlierCursor: detailLoaded && !canonicalRangeChanged
      ? current.earlierCursor
      : (event.earlierCursor ?? null),
  };
}

function mergeAcpActivityDetailPage({
  current,
  event,
  ownerKey,
  items,
  hasMoreEarlier,
  earlierCursor,
  loadingEarlierPage,
}: {
  current: AcpActivityDetailWindow;
  event: AcpActivityBatch;
  ownerKey: string;
  items: AcpTimelineEvent[];
  hasMoreEarlier: boolean;
  earlierCursor: string | null;
  loadingEarlierPage: boolean;
}): AcpActivityDetailWindow {
  const active = current.ownerKey === ownerKey
    ? current
    : createAcpActivityDetailWindow(event, ownerKey);
  if (!loadingEarlierPage) {
    const latest = mergeAcpEvents(
      items,
      event.events.filter(isVisibleActivityAuditEvent),
    ) as AcpTimelineEvent[];
    return {
      ownerKey,
      activityEndSeq: event.activityEndSeq,
      totalEventCount: event.totalEventCount,
      events: limitAcpEvents(
        latest,
        "start",
        ACP_ACTIVITY_DETAIL_WINDOW_LIMIT,
      ) as AcpTimelineEvent[],
      detailLoaded: true,
      hasMoreEarlier,
      earlierCursor,
      hasNewer: false,
      error: null,
    };
  }

  const merged = mergeAcpEvents(items, active.events) as AcpTimelineEvent[];
  const overflowed = merged.length > ACP_ACTIVITY_DETAIL_WINDOW_LIMIT;
  return {
    ...active,
    activityEndSeq: event.activityEndSeq,
    totalEventCount: event.totalEventCount,
    events: limitAcpEvents(
      merged,
      "end",
      ACP_ACTIVITY_DETAIL_WINDOW_LIMIT,
    ) as AcpTimelineEvent[],
    detailLoaded: true,
    hasMoreEarlier,
    earlierCursor,
    hasNewer: active.hasNewer || overflowed,
    error: null,
  };
}

function acpActivityDetailBelongsToRequest(
  items: AcpUiEventVm[],
  sessionId: string,
  event: AcpActivityBatch,
) {
  return items.every((item) => {
    if (item.sessionId !== sessionId) return false;
    const start = item.startedSeq ?? item.seq;
    const end = item.endedSeq ?? item.seq;
    return start >= event.activityStartSeq && end <= event.activityEndSeq;
  });
}

function captureVisibleAcpActivityDetailAnchor(
  viewport: HTMLElement | null,
  detailList: HTMLElement | null,
) {
  if (!viewport || !detailList) return null;
  const viewportBounds = viewport.getBoundingClientRect();
  for (const item of detailList.querySelectorAll<HTMLElement>(
    "[data-acp-activity-detail-item-key]",
  )) {
    const bounds = item.getBoundingClientRect();
    if (bounds.bottom < viewportBounds.top || bounds.top > viewportBounds.bottom) continue;
    const key = item.dataset.acpActivityDetailItemKey;
    if (key) return { key, top: bounds.top };
  }
  return null;
}

function findAcpActivityDetailItem(
  detailList: HTMLElement | null,
  key: string,
) {
  if (!detailList) return null;
  for (const item of detailList.querySelectorAll<HTMLElement>(
    "[data-acp-activity-detail-item-key]",
  )) {
    if (item.dataset.acpActivityDetailItemKey === key) return item;
  }
  return null;
}

function activityBatchSummary(
  batch: AcpActivityBatch,
  t: ReturnType<typeof useTranslation>["t"],
) {
  if (batch.live) return objectiveActivityLabel(batch.events.at(-1), t);
  const parts: string[] = [];
  if (batch.totalEventCount > 0) {
    parts.push(t("acp.activityRecordedCount", { count: batch.totalEventCount }));
  }
  if (batch.toolCallCount > 0) parts.push(t("acp.activityToolCount", { count: batch.toolCallCount }));
  if (batch.thoughtCount > 0) parts.push(t("acp.activityThoughtCount", { count: batch.thoughtCount }));
  if (batch.readFileCount > 0) parts.push(t("acp.activityReadFiles", { count: batch.readFileCount }));
  if (batch.writtenFileCount > 0) parts.push(t("acp.activityWrittenFiles", { count: batch.writtenFileCount }));
  return parts.join(" · ") || t("acp.activityRecorded");
}

function objectiveActivityLabel(
  event: AcpTimelineEvent | undefined,
  t: ReturnType<typeof useTranslation>["t"],
) {
  const descriptor = objectiveActivityDescriptor(event);
  if (descriptor.kind === "thought") return t("acp.activityThinking");
  const name = descriptor.name || t("acp.toolCall");
  return descriptor.parameter
    ? t("acp.activityCallingWithParameter", { name, parameter: descriptor.parameter })
    : t("acp.activityCalling", { name });
}

function objectiveActivityDescriptor(event: AcpTimelineEvent | undefined) {
  if (!event || event.kind === "thoughtDelta") {
    return { kind: "thought" as const, name: null, parameter: null };
  }
  const details = toolDetails(event, false);
  return {
    kind: "tool" as const,
    name: details.name || event.title || null,
    parameter: toolSummary(details.queryBlocks) ?? null,
  };
}

function childAgentStatusLabel(
  status: string | null | undefined,
  t: ReturnType<typeof useTranslation>["t"],
) {
  if (status === "queued") return t("acp.subAgentQueued");
  if (status === "waiting_permission") return t("acp.subAgentWaitingPermission");
  if (status === "interrupted") return t("acp.subAgentInterrupted");
  return status ? displayStatus(t, status) : t("acp.subAgentRunning");
}

const AssistantTimelineRow = memo(function AssistantTimelineRow({
  children,
  density = "single",
  nested = false,
}: {
  children: React.ReactNode;
  timestamp?: string | null;
  density?: "single" | "start" | "middle" | "end";
  nested?: boolean;
}) {
  if (nested) return <div className="min-w-0 max-w-full">{children}</div>;
  return (
    <Message
      className={cn(
        "min-w-0 items-start justify-start gap-2",
        density !== "single" && "mb-0",
      )}
    >
      <div className="w-9 shrink-0" aria-hidden="true" />
      <div className="w-full min-w-0 max-w-[82%] flex-1">{children}</div>
    </Message>
  );
});

const MessageBubble = memo(function MessageBubble({
  event,
  streamingMarkdownItemKey,
  messageAttachmentLocator,
  onMessageAttachmentClick,
  nested = false,
}: {
  event: AcpTimelineEvent;
  streamingMarkdownItemKey?: string | null;
  messageAttachmentLocator?: MessageAttachmentLocator;
  onMessageAttachmentClick?: (att: MessageAttachmentPreview) => void;
  nested?: boolean;
}) {
  const { t } = useTranslation();
  const branchLocator = useContext(AcpBranchLocatorContext);
  const workspace = useOptionalRightWorkspaceCommands();
  const isUser = event.kind === "userTextDelta";
  // Provider failures are surfaced by the session error state. A user prompt
  // itself is never an error surface: it may be retried, and colouring it red
  // briefly makes one logical prompt look like multiple contradictory states.
  const failed = !isUser && event.status === "failed";
  const retry = isUser ? promptRetryInfo(event) : null;
  const retryFooter = isUser ? promptRetryFooterKind(event) : null;
  const streamingDraft =
    !isUser && timelineEventKey(event) === streamingMarkdownItemKey;
  useEffect(() => {
    if (isUser || (event.kind !== "textDelta" && event.kind !== "thoughtDelta")) {
      return;
    }
    recordAcpStreamingDiagnostic("markdown-render", () => ({
      eventKind: event.kind,
      eventId: event.id,
      eventSeq: event.seq,
      eventEndedSeq: event.endedSeq ?? null,
      itemKey: timelineEventKey(event),
      streamingMarkdownItemKey: streamingMarkdownItemKey ?? null,
      streaming: streamingDraft,
      contentLength: event.content?.length ?? 0,
    }));
  }, [event.content?.length, event.endedSeq, event.id, event.kind, event.seq, isUser, streamingDraft, streamingMarkdownItemKey]);
  const rawAttachments = messageAttachmentPreviewsFromRaw(event.raw);
  const userQuotes = isUser ? userPromptQuotesFromRaw(event.raw) : [];
  const hasAttachments = isUser && rawAttachments.length > 0;
  const attachmentGroups = groupMessageAttachmentPreviews(rawAttachments);
  const runtimeControlParts = !isUser && !streamingDraft
    ? runtimeControlMessageParts(event)
    : { display: null, visibleText: event.content ?? "" };
  const messageText = runtimeControlParts.display
    ? runtimeControlParts.visibleText
    : (event.content ?? "");
  const showMessageBubble = streamingDraft || messageText.trim().length > 0;
  const attachmentOnly = isUser
    && hasAttachments
    && userQuotes.length === 0
    && !showMessageBubble;
  const quotableAgentMessage = !isUser && !streamingDraft && !failed && messageText.trim().length > 0;
  const openHiddenPromptSection = useCallback((request: HiddenPromptSectionOpenRequest) => {
    if (!branchLocator || !workspace?.scopeKey || event.optimistic) return;
    void workspace.openResource(createHiddenPromptSectionWorkspaceResource({
      scopeKey: workspace.scopeKey,
      title: request.label,
      locator: branchLocator,
      eventId: event.id,
      eventSeq: event.endedSeq ?? event.seq,
      partIndex: request.sourceIndex,
    }));
  }, [branchLocator, event.endedSeq, event.id, event.optimistic, event.seq, workspace]);
  const openArtifact = useCallback((name: string) => {
    if (!branchLocator || !workspace?.scopeKey) return;
    void workspace.openResource({
      kind: 'conversation-asset',
      key: conversationAssetWorkspaceResourceKey('artifact', branchLocator, name),
      scopeKey: workspace.scopeKey,
      title: name,
      description: null,
      attention: false,
      locator: branchLocator,
      assetKind: 'artifact',
      name,
    });
  }, [branchLocator, workspace]);
  return (
    <Message
      data-acp-message-row={isUser ? "user" : "assistant"}
      className={cn(
        "min-w-0 items-start gap-2 [container-type:inline-size]",
        isUser ? "justify-end" : "justify-start",
        nested && "w-full",
      )}
    >
      {!isUser && !nested ? (
        <AcpAvatarWithTime tone="assistant" timestamp={event.timestamp} />
      ) : null}
      <div
        className={cn(
          "group/message min-w-0 max-w-[var(--conversation-message-max-inline-size)] space-y-0.5",
          isUser && "flex flex-col items-end",
          nested && "w-full max-w-full",
        )}
      >
        <UserMessageQuotes quotes={userQuotes} />
        {showMessageBubble ? (
          <MessageContent
            data-agent-quotable-text={quotableAgentMessage ? "true" : undefined}
            data-agent-message-key={quotableAgentMessage ? timelineEventKey(event) : undefined}
            variant={isUser ? "user" : "assistant"}
            className={cn(
              "rounded-2xl px-4 text-sm leading-6 [overflow-wrap:anywhere]",
              isUser
                ? "w-fit max-w-full rounded-br-md py-3 shadow-none"
                : "rounded-bl-md pb-0 pt-2 shadow-none",
              failed &&
                "!border !border-destructive/40 !bg-destructive/10 !text-destructive",
            )}
          >
            {isUser ? (
              <UserMessageDisclosure>
                <HiddenPromptMessageContent
                  content={event.content ?? ""}
                  onOpenSection={branchLocator && workspace?.scopeKey && !event.optimistic
                    ? openHiddenPromptSection
                    : undefined}
                />
              </UserMessageDisclosure>
            ) : (
              <Markdown streaming={streamingDraft}>{messageText}</Markdown>
            )}
          </MessageContent>
        ) : null}
        {quotableAgentMessage ? (
          <AgentMessageCopyAction markdown={messageText} timestamp={event.timestamp} />
        ) : null}
        {runtimeControlParts.display ? (
          <RuntimeControlOutputCard
            display={runtimeControlParts.display}
            onOpenArtifact={branchLocator && workspace?.scopeKey ? openArtifact : undefined}
          />
        ) : null}
        {hasAttachments ? (
          <div
            data-acp-attachment-only={attachmentOnly ? "true" : undefined}
            className={cn(
              "flex max-w-full flex-col gap-2 px-1",
              isUser && "items-end",
              attachmentOnly && "pt-0.5",
            )}
          >
            {attachmentGroups.images.length > 0 ? (
              <div
                data-acp-attachment-row="images"
                className={cn("flex max-w-full flex-wrap gap-1.5", isUser && "justify-end")}
              >
                {attachmentGroups.images.map((att) => (
                  <MessageAttachmentPreviewButton
                    key={att.path}
                    attachment={att}
                    locator={messageAttachmentLocator}
                    onClick={onMessageAttachmentClick}
                  />
                ))}
              </div>
            ) : null}
            {attachmentGroups.files.length > 0 ? (
              <div
                data-acp-attachment-row="files"
                className={cn("flex max-w-full flex-wrap gap-1.5", isUser && "justify-end")}
              >
                {attachmentGroups.files.map((att) => (
                  <MessageAttachmentPreviewButton
                    key={att.path}
                    attachment={att}
                    locator={messageAttachmentLocator}
                    onClick={onMessageAttachmentClick}
                  />
                ))}
              </div>
            ) : null}
          </div>
        ) : null}
        {event.optimistic || failed || retry ? (
          <div
            className={cn(
              "flex px-1 text-xs text-muted-foreground",
              isUser && "justify-end text-right",
              retryFooter === "retrying" && "acp-retry-live-label",
            )}
          >
            {retryFooter === "failed" && retry ? (
              t("acp.retryFailed", { count: retry.attempt })
            ) : retryFooter === "cancelled" && retry ? (
              t("acp.retryStopped", { count: retry.attempt })
            ) : failed ? (
              t("acp.sendFailed")
            ) : retry ? (
              t("acp.retrying", { current: retry.attempt, total: retry.maxAttempts })
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

const AgentMessageCopyAction = memo(function AgentMessageCopyAction({
  markdown,
  timestamp,
}: {
  markdown: string;
  timestamp?: string | null;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => () => {
    if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
  }, []);

  const copyMarkdown = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(markdown);
      setCopied(true);
      if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
      copiedTimerRef.current = setTimeout(() => setCopied(false), 1_500);
    } catch {
      setCopied(false);
    }
  }, [markdown]);

  const label = copied ? t("acp.markdownSourceCopied") : t("acp.copyMarkdownSource");
  const detailedTime = formatAgentMessageDetailedTime(timestamp, t('conversation.runtime.justNow'));
  return (
    <MessageActions
      data-agent-message-actions="true"
      className="h-5 px-1 leading-none opacity-100 transition-opacity [@media(hover:hover)]:opacity-0 [@media(hover:hover)]:group-hover/message:opacity-100 group-focus-within/message:opacity-100"
    >
      <MessageAction tooltip={label} side="bottom">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="size-5 text-muted-foreground hover:bg-muted/45 hover:text-foreground"
          aria-label={label}
          data-agent-message-copy="true"
          onClick={() => void copyMarkdown()}
        >
          {copied ? (
            <Check className="size-3 text-emerald-600 dark:text-emerald-300" aria-hidden="true" />
          ) : (
            <Copy className="size-3" aria-hidden="true" />
          )}
        </Button>
      </MessageAction>
      <span
        className="whitespace-nowrap px-1 text-ui-micro leading-5 tabular-nums text-muted-foreground/70"
        data-agent-message-detailed-time="true"
      >
        {detailedTime}
      </span>
    </MessageActions>
  );
});

const RuntimeControlOutputCard = memo(function RuntimeControlOutputCard({
  display,
  onOpenArtifact,
}: {
  display: RuntimeControlOutputDisplay;
  onOpenArtifact?: (name: string) => void;
}) {
  const { t } = useTranslation();
  const jsonText = display.jsonText ?? "";
  const prettyJson = useMemo(() => {
    try {
      return JSON.stringify(JSON.parse(jsonText), null, 2);
    } catch {
      return jsonText;
    }
  }, [jsonText]);
  const subtitle = display.kind === "dynamic-node-completion"
    ? t("acp.runtimeControlDynamic")
    : t("acp.runtimeControlWorkflow");
  const isInvalid = display.parseStatus === "invalid";
  const Icon = isInvalid ? CircleAlert : ListTodo;
  return (
    <Collapsible
      data-theme-role="runtime-control"
      className={cn(
        "min-w-0 max-w-full overflow-hidden",
        isInvalid && "border border-destructive/25 bg-destructive/5",
      )}
    >
      <div className="flex min-w-0 items-stretch">
        <CollapsibleTrigger asChild>
          <Button
            variant="ghost"
            className={cn(
              "group h-9 min-w-0 flex-1 justify-between rounded-none px-3 py-1.5 text-left font-normal",
              isInvalid && "hover:bg-destructive/10",
            )}
          >
            <span className="flex min-w-0 items-center gap-2 overflow-hidden">
              <span
                className={cn(
                  "flex size-6 shrink-0 items-center justify-center rounded-md",
                  isInvalid
                    ? "bg-destructive/10 text-destructive"
                    : "bg-foreground/[0.06] text-foreground",
                )}
              >
                <Icon className="size-3.5" />
              </span>
              <span className="truncate text-sm font-medium text-foreground">
                {t("acp.runtimeControlTitle")}
              </span>
              <span className="shrink-0 text-xs text-muted-foreground">·</span>
              <span className="truncate text-xs text-muted-foreground">{subtitle}</span>
            </span>
            <ChevronDown className="ml-2 size-4 shrink-0 text-muted-foreground transition-transform group-data-[state=open]:rotate-180" />
          </Button>
        </CollapsibleTrigger>
        {display.artifactName && onOpenArtifact ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="h-9 min-w-0 max-w-48 shrink-0 gap-1.5 rounded-none border-l border-border/50 px-2.5 text-xs font-normal"
                onClick={() => onOpenArtifact(display.artifactName!)}
              >
                <FileText className="size-3.5 shrink-0" />
                <span className="truncate">{display.artifactName}</span>
              </Button>
            </TooltipTrigger>
            <TooltipContent className="max-w-[360px] break-all">{display.artifactName}</TooltipContent>
          </Tooltip>
        ) : null}
      </div>
      <CollapsibleContent
        className={cn(
          "border-t border-foreground/10 px-3 py-2",
          isInvalid && "border-destructive/15",
        )}
      >
        <pre className="max-h-64 min-w-0 overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-5 text-foreground [overflow-wrap:anywhere]">
          {prettyJson}
        </pre>
      </CollapsibleContent>
    </Collapsible>
  );
});

export function runtimeControlMessageParts(event: Pick<AcpUiEventVm, "content" | "raw">): RuntimeControlMessageParts {
  const display = runtimeControlOutputDisplayFromRaw(event.raw);
  const content = event.content ?? "";
  if (!display) return { display: null, visibleText: content };
  const start = numberValue(display.start);
  const end = numberValue(display.end);
  if (start == null || end == null || start < 0 || end < start || end > content.length) {
    return { display, visibleText: content };
  }
  const visibleText = `${content.slice(0, start)}${content.slice(end)}`.trim();
  return { display, visibleText };
}

function runtimeControlOutputDisplayFromRaw(raw: unknown): RuntimeControlOutputDisplay | null {
  const display = rawObject(raw)?.runtimeControlOutputDisplay;
  if (!display || typeof display !== "object" || Array.isArray(display)) return null;
  const object = display as Record<string, unknown>;
  return {
    artifactName: stringValue(object.artifactName) ?? undefined,
    kind: stringValue(object.kind) ?? undefined,
    jsonText: stringValue(object.jsonText) ?? undefined,
    start: numberValue(object.start) ?? undefined,
    end: numberValue(object.end) ?? undefined,
    jsonStart: numberValue(object.jsonStart) ?? undefined,
    jsonEnd: numberValue(object.jsonEnd) ?? undefined,
    fenced: typeof object.fenced === "boolean" ? object.fenced : undefined,
    parseStatus: stringValue(object.parseStatus) ?? undefined,
  };
}

function numberValue(value: unknown) {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export const MessageAttachmentPreviewButton = memo(function MessageAttachmentPreviewButton({
  attachment,
  locator,
  onClick,
}: {
  attachment: MessageAttachmentPreview;
  locator?: MessageAttachmentLocator;
  onClick?: (attachment: MessageAttachmentPreview) => void;
}) {
  const isImage = isImageMessageAttachment(attachment);
  const attachmentLabel = `${attachment.name} (${formatAttachmentSize(attachment.size)})`;
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);

  useEffect(() => {
    if (!isImage || !locator) {
      setPreviewSrc(null);
      return;
    }
    let cancelled = false;
    setPreviewSrc(null);
    const contentPromise = isTaskInputMessageAttachment(attachment)
      ? showConversationAttachment(locator.projectId, locator.taskId, attachment.name)
      : showConversationMessageAttachment(
          locator.projectId,
          locator.taskId,
          locator.runId,
          locator.roundId,
          locator.nodeId,
          locator.attemptId,
          attachment.name,
          attachment.path,
          locator.outerNodeId,
          locator.outerAttemptId,
        );
    contentPromise
      .then((content) => {
        if (!cancelled) setPreviewSrc(imageSrcFromContent(content));
      })
      .catch(() => {
        if (!cancelled) setPreviewSrc(null);
      });
    return () => {
      cancelled = true;
    };
  }, [attachment.name, attachment.path, isImage, locator]);

  const imageActions = useImageActions(isImage && previewSrc ? {
    name: attachment.name,
    mime: attachment.type,
    previewUrl: previewSrc,
  } : null);

  if (isImage) {
    const previewButton = (
      <button
        type="button"
        className={cn(
          "relative size-[72px] overflow-hidden rounded-lg border border-border/60 bg-card/80 text-muted-foreground shadow-sm transition-colors hover:border-primary/45 hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          imageActions.state === 'failed' && "ring-1 ring-destructive/70",
        )}
        aria-label={attachment.name}
        aria-busy={imageActions.pending || undefined}
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
        {imageActions.pending ? (
          <span className="absolute inset-0 flex items-center justify-center bg-background/65">
            <Loader2 className="size-4 animate-spin" aria-hidden="true" />
          </span>
        ) : imageActions.state === 'copied' || imageActions.state === 'saved' ? (
          <span className="absolute right-1 top-1 flex size-5 items-center justify-center rounded-full bg-background/85 text-emerald-600 shadow-sm">
            <Check className="size-3" aria-hidden="true" />
          </span>
        ) : imageActions.state === 'failed' ? (
          <span className="absolute right-1 top-1 flex size-5 items-center justify-center rounded-full bg-background/85 text-destructive shadow-sm">
            <CircleAlert className="size-3" aria-hidden="true" />
          </span>
        ) : null}
      </button>
    );
    return (
      <Tooltip>
        {previewSrc ? (
          <ImageActionsContextMenu actions={imageActions}>
            <TooltipTrigger asChild>{previewButton}</TooltipTrigger>
          </ImageActionsContextMenu>
        ) : (
          <TooltipTrigger asChild>{previewButton}</TooltipTrigger>
        )}
        <TooltipContent className="max-w-[360px] break-all">
          {imageActions.message ?? attachmentLabel}
        </TooltipContent>
        {imageActions.message ? (
          <span className="sr-only" aria-live="polite">{imageActions.message}</span>
        ) : null}
      </Tooltip>
    );
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="inline-flex h-9 w-fit max-w-full shrink-0 items-center gap-1.5 rounded-full border border-border/60 bg-card/80 px-3 text-ui-caption text-muted-foreground transition-colors hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() => onClick?.(attachment)}
        >
          <FileText className="size-3 text-muted-foreground" />
          <span className="max-w-[120px] truncate">{attachment.name}</span>
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-[360px] break-all">{attachmentLabel}</TooltipContent>
    </Tooltip>
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
  streamingMarkdownItemKey,
  nested = false,
  compact = false,
}: {
  event: AcpTimelineEvent;
  streamingMarkdownItemKey?: string | null;
  nested?: boolean;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  if (!event.content?.trim()) return null;
  const itemKey = timelineEventKey(event);
  const streaming = itemKey === streamingMarkdownItemKey;
  const duration = formatThinkingDuration(t, event.durationMs);
  return (
    <AssistantTimelineRow timestamp={event.timestamp} nested={nested}>
      <ChainOfThought
        data-theme-role={compact ? undefined : "activity"}
        className={cn(
          "min-w-0 max-w-full overflow-hidden",
          compact
            ? "px-1.5 py-1"
            : "px-2.5 py-1.5",
        )}
      >
        <ChainOfThoughtStep
          open={open}
          onOpenChange={setOpen}
        >
          <ChainOfThoughtTrigger
            leftIcon={<Clock className="size-4" />}
            className={cn(
              "w-full min-w-0 justify-between",
              compact && "text-xs",
            )}
          >
            <span className="flex min-w-0 flex-wrap items-center gap-2">
              <span className="font-medium">{t("acp.thought")}</span>
              {duration ? (
                <span
                  className={cn(
                    "text-xs tabular-nums",
                    compact
                      ? "text-muted-foreground/75"
                      : "rounded-full bg-muted px-2 py-0.5",
                  )}
                >
                  {duration}
                </span>
              ) : null}
            </span>
          </ChainOfThoughtTrigger>
          <ChainOfThoughtContent animated={false} preserveMount={streaming}>
            <div
              role="region"
              aria-label={t("acp.thought")}
              tabIndex={0}
              data-acp-thought-scroll-area="true"
              className="gold-themed-scrollbar max-h-72 min-w-0 overflow-y-auto overscroll-contain pr-2 outline-none [scrollbar-gutter:stable] focus-visible:ring-2 focus-visible:ring-ring/50"
            >
              <ChainOfThoughtItem className="min-w-0 break-words text-muted-foreground [overflow-wrap:anywhere]">
                <ChainOfThoughtText className="text-muted-foreground">
                  {event.content}
                </ChainOfThoughtText>
              </ChainOfThoughtItem>
            </div>
          </ChainOfThoughtContent>
        </ChainOfThoughtStep>
      </ChainOfThought>
    </AssistantTimelineRow>
  );
});

const ToolBlock = memo(function ToolBlock({
  event,
  nested = false,
  compact = false,
}: {
  event: AcpTimelineEvent;
  nested?: boolean;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const branchLocator = useContext(AcpBranchLocatorContext);
  const timelineWindowOwner = useContext(AcpTimelineWindowOwnerContext);
  const sessionId = timelineWindowOwner?.sessionId ?? event.sessionId ?? null;
  const logicalToolKey = event.toolCallId
    ? `tool:${event.toolCallId}`
    : `event:${event.id}`;
  const ownerKey = acpDetailOwnerKey(
    branchLocator,
    timelineWindowOwner,
    sessionId,
    logicalToolKey,
  );
  const observedRevision = timelineEventPosition(event);
  const requestScopeKey = acpDetailRequestScopeKey(
    ownerKey,
    timelineWindowOwner?.timelineGeneration ?? 0,
    observedRevision,
  );
  const [open, setOpen] = useState(false);
  const [detailState, setDetailState] = useState<AcpToolDetailState | null>(null);
  const [detailError, setDetailError] = useState<{
    scopeKey: string;
    sourceSignature: string;
    message: string;
  } | null>(null);
  const sourceSignature = useMemo(
    () => open ? toolDetailSourceSignature(event) : null,
    [event.raw, open],
  );
  const activeDetailRequestRef = useRef<AcpToolDetailRequestToken | null>(null);
  const trailingToolDetailRequestRef = useRef(false);
  const latestLoadToolDetailRef = useRef<() => void>(() => {});
  const detailRequestSeqRef = useRef(0);
  const mountedRef = useRef(true);
  const openRef = useRef(open);
  const currentRequestScopeRef = useRef(requestScopeKey);
  const currentOwnerKeyRef = useRef(ownerKey);
  const currentEffectiveSessionIdRef = useRef(sessionId);
  const currentSourceSignatureRef = useRef<string | null>(sourceSignature);
  const currentEventRef = useRef(event);
  openRef.current = open;
  currentRequestScopeRef.current = requestScopeKey;
  currentOwnerKeyRef.current = ownerKey;
  currentEffectiveSessionIdRef.current = sessionId;
  if (sourceSignature !== null) {
    currentSourceSignatureRef.current = sourceSignature;
  }
  currentEventRef.current = event;
  const activeDetailState = detailState
    && sourceSignature !== null
    && detailState.ownerKey === ownerKey
    && toolDetailSourceSnapshotMatches(detailState, event, sourceSignature)
      ? detailState
      : null;
  const detailEvent = activeDetailState
    ? mergeAcpToolDetailEnrichment(event, activeDetailState.event) as AcpTimelineEvent
    : event;
  const activeDetailError = detailError?.scopeKey === requestScopeKey
    && sourceSignature !== null
    && detailError.sourceSignature === sourceSignature
    ? detailError.message
    : null;
  const summaryDetails = toolDetails(event, false);
  const details = open ? toolDetails(detailEvent, true) : summaryDetails;
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
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      activeDetailRequestRef.current = null;
      trailingToolDetailRequestRef.current = false;
    };
  }, []);
  const ownsToolDetailRequest = (token: AcpToolDetailRequestToken) => {
    const active = activeDetailRequestRef.current;
    return Boolean(
      mountedRef.current
      && active === token
      && currentOwnerKeyRef.current === token.ownerKey
      && currentRequestScopeRef.current === token.scopeKey,
    );
  };
  const loadToolDetail = () => {
    if (
      !branchLocator
      || !sessionId?.trim()
      || sourceSignature === null
      || activeDetailState
      || !toolDetailAvailable(event)
    ) return;
    const activeRequest = activeDetailRequestRef.current;
    if (activeRequest) {
      if (
        activeRequest.scopeKey !== requestScopeKey
        || !toolDetailRequestStillMatchesEvent(
          activeRequest,
          event,
          sessionId,
          sourceSignature,
        )
      ) {
        trailingToolDetailRequestRef.current = true;
      }
      return;
    }
    const token: AcpToolDetailRequestToken = {
      requestSeq: detailRequestSeqRef.current + 1,
      ownerKey,
      scopeKey: requestScopeKey,
      observedRevision,
      eventId: event.id,
      toolCallId: event.toolCallId ?? null,
      sessionId,
      sourceSignature,
      sourceStatus: event.status ?? null,
      sourceContent: event.content ?? null,
      sourceTitle: event.title ?? null,
    };
    detailRequestSeqRef.current = token.requestSeq;
    activeDetailRequestRef.current = token;
    setDetailError(null);
    void getAcpToolDetail(
      branchLocator.projectId,
      branchLocator.taskId,
      branchLocator.runId,
      branchLocator.roundId,
      branchLocator.nodeId,
      branchLocator.attemptId,
      {
        branchId: branchLocator.branchId,
        sessionId: token.sessionId,
        eventId: event.id,
        toolCallId: event.toolCallId,
      },
      branchLocator.outerNodeId,
      branchLocator.outerAttemptId,
    ).then((detail) => {
      if (!ownsToolDetailRequest(token) || !detail.event) return;
      const currentEvent = currentEventRef.current;
      const currentSourceSignature = currentSourceSignatureRef.current;
      const detailEvent = detail.event as AcpTimelineEvent;
      if (
        currentSourceSignature === null
        || !toolDetailResponseBelongsToRequest(detailEvent, token)
        || !toolDetailRequestStillMatchesEvent(
          token,
          currentEvent,
          currentEffectiveSessionIdRef.current,
          currentSourceSignature,
        )
        || timelineEventPosition(detailEvent) < timelineEventPosition(currentEvent)
      ) return;
      const merged = mergeAcpToolDetailEnrichment(
        currentEvent,
        detailEvent,
      ) as AcpTimelineEvent;
      setDetailState({
        ownerKey,
        observedRevision: timelineEventPosition(currentEvent),
        sourceSignature: currentSourceSignature,
        sourceStatus: currentEvent.status ?? null,
        sourceContent: currentEvent.content ?? null,
        sourceTitle: currentEvent.title ?? null,
        event: merged,
      });
    }).catch((error) => {
      if (!ownsToolDetailRequest(token)) return;
      setDetailError({
        scopeKey: token.scopeKey,
        sourceSignature: token.sourceSignature,
        message: displayAppError(t, error),
      });
    }).finally(() => {
      if (activeDetailRequestRef.current === token) {
        activeDetailRequestRef.current = null;
        const loadTrailing = trailingToolDetailRequestRef.current;
        trailingToolDetailRequestRef.current = false;
        if (mountedRef.current && openRef.current && loadTrailing) {
          latestLoadToolDetailRef.current();
        }
      }
    });
  };
  latestLoadToolDetailRef.current = loadToolDetail;
  useEffect(() => {
    if (
      !open
      || activeDetailState
      || activeDetailError
      || !toolDetailAvailable(event)
    ) return;
    loadToolDetail();
  }, [
    activeDetailError,
    activeDetailState,
    event,
    open,
    requestScopeKey,
  ]);
  return (
    <AssistantTimelineRow timestamp={event.timestamp} nested={nested}>
      <div className="min-w-0 max-w-full">
        <Tool
          toolPart={toolPart}
          labels={toolLabels(t)}
          icon={<ToolIcon className="size-4" />}
          open={open}
          onOpenChange={(next) => {
            openRef.current = next;
            if (!next) trailingToolDetailRequestRef.current = false;
            setOpen(next);
          }}
          animated={false}
          variant={compact ? "audit" : "card"}
          className={compact ? "acp-activity-audit-tool" : undefined}
        />
        {activeDetailError ? (
          <div className="mt-1 flex min-w-0 items-center justify-between gap-2 px-2 text-xs text-destructive">
            <span className="min-w-0 truncate">{activeDetailError}</span>
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="h-7 shrink-0 px-2 text-xs"
              onClick={loadToolDetail}
              data-acp-tool-detail-retry="true"
            >
              {t("common.retry")}
            </Button>
          </div>
        ) : null}
      </div>
    </AssistantTimelineRow>
  );
});

function toolDetailAvailable(event: AcpTimelineEvent) {
  return goldBandConversationMeta(event)?.toolDetailAvailable === true;
}

function toolDetailSourceSignature(event: AcpTimelineEvent) {
  const raw = rawObject(event.raw);
  const toolCall = rawObject(raw?.toolCall) ?? rawObject(raw?.content) ?? raw;
  return JSON.stringify(canonicalizeToolDetailSourceValue({
    toolName: canonicalToolName(event) ?? null,
    detailAvailable: toolDetailAvailable(event),
    rawInput: toolCall?.rawInput ?? raw?.rawInput ?? null,
    input: toolCall?.input ?? raw?.input ?? null,
    locations: toolCall?.locations ?? raw?.locations ?? null,
  }));
}

function canonicalizeToolDetailSourceValue(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(canonicalizeToolDetailSourceValue);
  }
  const object = rawObject(value);
  if (!object) return value ?? null;
  return Object.keys(object)
    .sort()
    .reduce<Record<string, unknown>>((canonical, key) => {
      canonical[key] = canonicalizeToolDetailSourceValue(object[key]);
      return canonical;
    }, {});
}

function toolDetailSourceSnapshotMatches(
  state: AcpToolDetailState,
  event: AcpTimelineEvent,
  sourceSignature: string,
) {
  return state.observedRevision === timelineEventPosition(event)
    && state.sourceSignature === sourceSignature
    && state.sourceStatus === (event.status ?? null)
    && state.sourceContent === (event.content ?? null)
    && state.sourceTitle === (event.title ?? null);
}

function toolDetailRequestStillMatchesEvent(
  token: AcpToolDetailRequestToken,
  event: AcpTimelineEvent,
  effectiveSessionId: string | null,
  sourceSignature: string,
) {
  return token.observedRevision === timelineEventPosition(event)
    && token.eventId === event.id
    && token.toolCallId === (event.toolCallId ?? null)
    && token.sessionId === effectiveSessionId
    && token.sourceSignature === sourceSignature
    && token.sourceStatus === (event.status ?? null)
    && token.sourceContent === (event.content ?? null)
    && token.sourceTitle === (event.title ?? null);
}

function toolDetailResponseBelongsToRequest(
  detail: AcpTimelineEvent,
  token: AcpToolDetailRequestToken,
) {
  if (detail.sessionId !== token.sessionId) return false;
  if (token.toolCallId) return detail.toolCallId === token.toolCallId;
  return detail.id === token.eventId;
}

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

export function PermissionRequestCard({
  request,
  onSelect,
  status = "pending",
  nested = false,
}: {
  request: AcpPermissionRequestVm;
  onSelect?: (optionId: string) => void;
  status?: string | null;
  nested?: boolean;
}) {
  const { t } = useTranslation();
  const decisionSummary = permissionRequestSummary(request);
  const pending = isPendingPermissionStatus(status);
  if (!pending) return null;

  return (
    <AssistantTimelineRow nested={nested}>
      <div data-theme-role="permission-card" className="acp-permission-request-card w-full max-w-2xl overflow-hidden px-4 py-3.5">
        <div className="flex min-w-0 flex-col gap-3">
          <div className="flex min-w-0 items-center gap-3">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border/60 bg-transparent text-muted-foreground">
              <ShieldQuestion className="size-4" />
            </span>
            <div className="min-w-0">
              <div className="truncate text-ui-compact font-semibold tracking-[-0.01em] text-foreground">
                {request.title}
              </div>
              <div className="mt-0.5 truncate text-ui-caption leading-4 text-muted-foreground">
                {t("acp.permissionPending")}
              </div>
            </div>
          </div>
          {decisionSummary ? (
            <div className="ml-11 min-w-0 border-l border-border/60 py-1 pl-3">
              <div className="mb-1 text-ui-micro font-medium uppercase tracking-[0.08em] text-muted-foreground">
                {t("acp.toolParameters")}
              </div>
              <div
                data-acp-permission-summary="true"
                className="line-clamp-6 min-w-0 overflow-hidden whitespace-pre-wrap break-all font-mono text-ui-caption leading-5 text-foreground/85 [overflow-wrap:anywhere]"
              >
                {decisionSummary}
              </div>
            </div>
          ) : null}
          {onSelect ? (
            <div className="grid min-w-0 grid-cols-1 gap-2 pl-11 sm:grid-cols-2">
              {request.options.map((option) => {
              const label = option.name || option.optionId;
              const isAllowOption = option.kind.startsWith("allow");
              return (
                <Tooltip key={option.optionId}>
                  <TooltipTrigger asChild>
                    <Button
                      size="sm"
                      variant="outline"
                      className={cn(
                        "h-8 min-w-0 max-w-full justify-center rounded-lg border-border/65 bg-transparent px-3 text-xs font-medium shadow-none",
                        isAllowOption
                          ? "text-accent-foreground hover:border-accent-foreground/35 hover:bg-accent/60 hover:text-accent-foreground focus-visible:border-accent-foreground/35 focus-visible:bg-accent/60"
                          : "text-muted-foreground hover:bg-muted/45 hover:text-foreground",
                      )}
                      aria-label={label}
                      onClick={() => onSelect(option.optionId)}
                    >
                      <span className="min-w-0 truncate">{label}</span>
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent
                    sideOffset={8}
                    className="max-w-[min(30rem,calc(100vw-2rem))] whitespace-normal break-words px-3 py-2 text-left leading-5 [overflow-wrap:anywhere]"
                  >
                    {label}
                  </TooltipContent>
                </Tooltip>
              );
              })}
            </div>
          ) : null}
        </div>
      </div>
    </AssistantTimelineRow>
  );
}

function isPendingPermissionStatus(status?: string | null) {
  return !status || status.toLowerCase() === "pending";
}

export function permissionRequestSummary(request: AcpPermissionRequestVm) {
  const raw = rawObject(request.raw);
  const toolCall = rawObject(raw?.toolCall) ?? raw;
  const rawInput =
    rawObject(toolCall?.rawInput) ?? rawObject(raw?.rawInput) ?? null;
  const locations =
    arrayValue(toolCall?.locations) ?? arrayValue(raw?.locations) ?? null;
  const title = stringValue(toolCall?.title) ?? request.title;
  const description = stringValue(rawInput?.description);
  const parameterSummary = toolSummary(
    queryBlocksFromTool(title, rawInput, locations),
  );
  return [description, parameterSummary]
    .filter((value, index, values): value is string =>
      Boolean(value) && values.indexOf(value) === index,
    )
    .join(" · ") || null;
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
  const order = page?.order ?? query.order ?? "desc";
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
      order,
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
    <div className="@container/raw-frame w-full min-w-0 max-w-full space-y-3 overflow-hidden">
      <div className="rounded-2xl border border-border/60 bg-card/50 p-3 shadow-sm shadow-background/20">
        <div className="flex min-w-0 flex-col gap-3">
          <div className="relative min-w-0">
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
          <div className="flex min-w-0 flex-wrap items-center gap-2" data-raw-frame-filters="true">
            <Select
              value={query.kind ?? "all"}
              onValueChange={(value) =>
                applyQuery({
                  page: 0,
                  kind: value === "all" ? undefined : value,
                })
              }
            >
              <SelectTrigger className="h-9 w-44 max-w-full">
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
              <SelectTrigger className="h-9 w-36 max-w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("acp.rawDirectionAll")}</SelectItem>
                <SelectItem value="inbound">{t("acp.rawInbound")}</SelectItem>
                <SelectItem value="outbound">{t("acp.rawOutbound")}</SelectItem>
              </SelectContent>
            </Select>
            <Select
              value={order}
              onValueChange={(value) =>
                applyQuery({
                  page: 0,
                  order: value as AcpRawFrameOrder,
                })
              }
            >
              <SelectTrigger
                aria-label={t("acp.rawSortOrder")}
                className="h-9 w-40 max-w-full"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="desc">{t("acp.rawSortNewest")}</SelectItem>
                <SelectItem value="asc">{t("acp.rawSortOldest")}</SelectItem>
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
                {t(order === "desc" ? "acp.rawLatest" : "acp.rawEarliest")}
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
                {t(
                  order === "desc"
                    ? "acp.rawNewer"
                    : "acp.rawPreviousOlder",
                )}
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="h-8 rounded-full px-3"
                disabled={loading || !page?.hasNext}
                onClick={() => applyQuery({ page: (page?.page ?? 0) + 1 })}
              >
                {t(
                  order === "desc" ? "acp.rawOlder" : "acp.rawNextNewer",
                )}
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
      className="group w-full min-w-0 max-w-full overflow-hidden rounded-xl border border-border/60 bg-card/50 text-ui-caption leading-5 shadow-sm shadow-background/20 open:border-primary/20 open:bg-card/70 open:ring-1 open:ring-primary/10"
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
          <span className="shrink-0 rounded-full bg-muted px-2 py-0.5 text-ui-micro text-muted-foreground">
            {displayRawDirection(t, frame.direction)}
          </span>
        ) : null}
        <span className="shrink-0 rounded-full bg-primary/10 px-2 py-0.5 text-ui-micro text-primary">
          {displayRawKind(t, frame.kind)}
        </span>
        <span className="block min-w-0 flex-1 truncate text-foreground/75">
          {compact}
        </span>
        {frame.contentTruncated ? (
          <span className="shrink-0 text-ui-micro text-amber-600 dark:text-amber-300">
            truncated
          </span>
        ) : null}
      </summary>
      {isOpen ? (
        <pre
          className={cn(
            "block w-full min-w-0 max-w-full overflow-x-hidden whitespace-pre-wrap break-all border-t border-border/50 bg-background/40 px-4 py-3 font-sans text-foreground/75 outline-none [overflow-wrap:anywhere]",
            scrollable
              ? "max-h-[38rem] overflow-y-auto"
              : "overflow-y-visible",
          )}
        >
          {displayExpanded}
        </pre>
      ) : null}
    </details>
  );
});

function useSessionTimingSeconds(
  timing: AcpSessionTimingVm | null | undefined,
  fallbackSeconds: number | null,
  _active: boolean,
) {
  if (!timing) return fallbackSeconds;
  return timing.sessionElapsedSeconds;
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

function promptRetryInfo(event: AcpUiEventVm) {
  const retry = rawObject(rawObject(event.raw)?.retry);
  const attempt = numberValue(retry?.attempt);
  const maxAttempts = numberValue(retry?.maxAttempts);
  if (attempt == null || attempt < 1 || maxAttempts == null || maxAttempts < attempt) {
    return null;
  }
  return { attempt, maxAttempts };
}

export function promptRetryFooterKind(event: AcpUiEventVm) {
  if (!promptRetryInfo(event)) return null;
  if (event.status === "failed") return "failed";
  if (event.status === "cancelled") return "cancelled";
  return "retrying";
}

function isGoldBandUserPrompt(event: AcpUiEventVm) {
  return (
    event.kind === "userTextDelta" &&
    rawObject(event.raw)?.source === "goldBandPrompt"
  );
}

function isProviderHistoryUserPrompt(event: AcpUiEventVm) {
  return (
    event.kind === "userTextDelta" &&
    rawObject(event.raw)?.source === "providerHistory"
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

function isAgentLink(event: AcpTimelineItem): event is AcpAgentLink {
  return event.kind === "agentLink";
}

function isActivityBatch(event: AcpTimelineItem): event is AcpActivityBatch {
  return event.kind === "activityBatch";
}

function isAgentToolCall(event: AcpUiEventVm) {
  if (event.kind !== "toolCall" && event.kind !== "toolCallUpdate")
    return false;
  return Boolean(launchedAgentExecutionId(event));
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

function goldBandConversationMeta(event: Pick<AcpUiEventVm, "raw">) {
  const raw = rawObject(event.raw);
  const direct = rawObject(rawObject(raw?._meta)?.goldBandConversation);
  if (direct) return direct;
  const toolCall = rawObject(raw?.toolCall);
  return rawObject(rawObject(toolCall?._meta)?.goldBandConversation);
}

function launchedAgentExecutionId(event: Pick<AcpUiEventVm, "raw">) {
  return stringValue(goldBandConversationMeta(event)?.launchedAgentExecutionId);
}

function canonicalToolName(event: Pick<AcpUiEventVm, "raw">) {
  return stringValue(goldBandConversationMeta(event)?.toolName);
}

function agentToolInput(event: AcpUiEventVm) {
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

function planEntries(event: Pick<AcpUiEventVm, "raw">): AcpTodoEntry[] {
  const entries = arrayValue(rawObject(event.raw)?.entries) ?? [];
  return entries
    .map((entry) => {
      const value = rawObject(entry);
      return {
        content: stringValue(value?.content) ?? undefined,
        status: stringValue(value?.status) ?? undefined,
        priority: stringValue(value?.priority) ?? undefined,
      };
    })
    .filter((entry) => Boolean(entry.content));
}

function isTopLevelPlanEvent(
  event: AcpUiEventVm,
  _events: AcpUiEventVm[] = [event],
) {
  return event.kind === "plan";
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
  if (isAgentLink(event)) return "tool";
  if (event.kind === "thoughtDelta") return "thinking";
  if (event.kind === "toolCall" || event.kind === "toolCallUpdate")
    return "tool";
  if (event.kind === "contextCompaction" && event.status === "running")
    return "compacting";
  if (event.kind === "textDelta") return "responding";
  return "processing";
}

function processingLabel(
  t: ReturnType<typeof useTranslation>["t"],
  kind: AcpProcessingKind,
) {
  if (kind === "sending") return t("acp.sending");
  if (kind === "stopping") return t("acp.stopping");
  if (kind === "preparing-workspace")
    return t("conversation.runtime.preparingDevelopmentEnvironment");
  if (kind === "processing-workspace")
    return t("conversation.runtime.processingWorkspace");
  if (kind === "launching-next-node") return t("conversation.runtime.launchingNextNode");
  if (kind === "launching") return t("acp.launchingClaude");
  if (kind === "thinking") return t("acp.thinkingNow");
  if (kind === "tool") return t("acp.toolRunning");
  if (kind === "compacting") return t("acp.compactionRunning");
  if (kind === "responding") return t("acp.responding");
  return t("acp.processing");
}

function composerPlaceholderText(
  state: ReturnType<typeof deriveAcpRuntimeComposerState>,
  t: ReturnType<typeof useTranslation>["t"],
) {
  if (state.placeholderKind === "stopping") return t("conversation.runtime.composerStoppingPlaceholder");
  if (state.placeholderKind === "runtime-controlled") return t("conversation.runtime.composerRuntimeControlledPlaceholder");
  if (state.placeholderKind === "message") return state.message && state.message !== "runtime-error" ? state.message : t("acp.composerPlaceholder");
  return t("acp.composerPlaceholder");
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
    return permissionRequestFromEvent(event);
  }
  return null;
}

export function permissionRequestFromEvent(
  event: AcpUiEventVm,
): AcpPermissionRequestVm | null {
  if (event.kind !== "permissionRequest") return null;
  const requestId = permissionRequestIdFromEvent(event);
  const raw: Record<string, unknown> = {
    ...(rawObject(event.raw) ?? {}),
    requestId,
  };
  const conversation = rawObject(rawObject(raw._meta)?.goldBandConversation);
  return {
    kind: "permission",
    interactionId: requestId,
    turnId: stringValue(conversation?.turnId) ?? stringValue(raw.turnId) ?? null,
    promptEventId:
      stringValue(conversation?.promptEventId) ?? stringValue(raw.promptEventId) ?? null,
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

interface PendingElicitationVm {
  interactionId: string;
  message: string;
  requestedSchema: ElicitationSchema;
}

function pendingElicitationFromRequest(
  request: AcpElicitationRequestVm,
): PendingElicitationVm {
  const requestedSchema = rawObject(request.requestedSchema);
  return {
    interactionId: request.interactionId,
    message: request.message,
    requestedSchema:
      requestedSchema?.type === "object"
        ? (requestedSchema as unknown as ElicitationSchema)
        : { type: "object", properties: {} },
  };
}

function elicitationRequestFromEvent(
  event: AcpUiEventVm,
): AcpElicitationRequestVm {
  const raw = rawObject(event.raw) ?? {};
  const nestedSchema = rawObject(raw.requestedSchema);
  const requestedSchema = nestedSchema ?? (raw.type === "object" ? raw : {
    type: "object",
    properties: {},
  });
  return {
    kind: "elicitation",
    interactionId: event.id,
    turnId:
      stringValue(rawObject(rawObject(raw._meta)?.goldBandConversation)?.turnId)
      ?? stringValue(raw.turnId)
      ?? null,
    promptEventId:
      stringValue(rawObject(rawObject(raw._meta)?.goldBandConversation)?.promptEventId)
      ?? stringValue(raw.promptEventId)
      ?? null,
    message: stringValue(raw.message) ?? event.content ?? "",
    toolCallId: event.toolCallId ?? stringValue(raw.toolCallId) ?? null,
    requestedSchema,
    raw: event.raw ?? {},
  };
}

function reducePendingInteractions(
  current: AcpSessionVm["pendingInteractions"],
  events: AcpUiEventVm[],
) {
  const pending = new Map(
    current.map((interaction) => [interaction.interactionId, interaction]),
  );
  const ordered = [...events].sort(
    (left, right) => originalSeqFromAcpEvent(left) - originalSeqFromAcpEvent(right),
  );
  for (const event of ordered) {
    if (event.kind === "elicitationResponse") {
      const elicitationId =
        stringValue(rawObject(event.raw)?.elicitationId) ??
        event.id.replace(/-response$/, "");
      pending.delete(elicitationId);
      continue;
    }
    if (event.kind === "permissionRequest") {
      const request = permissionRequestFromEvent(event);
      if (!request) continue;
      if (event.status?.toLowerCase() === "pending") {
        for (const interaction of pending.values()) {
          if (interaction.kind === "permission") {
            pending.delete(interaction.interactionId);
          }
        }
        pending.set(request.interactionId, request);
      } else {
        pending.delete(request.interactionId);
      }
      continue;
    }
    if (event.kind !== "elicitationRequest") continue;
    if (event.status?.toLowerCase() !== "pending") {
      pending.delete(event.id);
      continue;
    }
    for (const interaction of pending.values()) {
      if (interaction.kind === "elicitation") {
        pending.delete(interaction.interactionId);
      }
    }
    pending.set(event.id, elicitationRequestFromEvent(event));
  }
  return [...pending.values()];
}

export function applyPendingInteractionEventsToSession(
  session: AcpSessionVm | null | undefined,
  events: AcpUiEventVm[],
): AcpSessionVm | null {
  if (!session) return session ?? null;
  const pendingInteractions = reducePendingInteractions(
    session.pendingInteractions,
    events,
  );
  return {
    ...session,
    pendingInteractions,
  };
}

type AcpPendingInteractionKind = "permission" | "elicitation";

/**
 * A bounded history window is not an authoritative source for pending UI.
 * Only infer a pending interaction from events when the window includes the
 * latest session edge and the session metadata still describes that wait.
 */
export function canInferPendingInteractionFromWindow(
  session: Pick<AcpSessionVm, "status" | "timing"> | null | undefined,
  hasNewerEvents: boolean,
  kind: AcpPendingInteractionKind,
) {
  if (
    !session ||
    hasNewerEvents ||
    !isSessionActiveStatus(session.status)
  )
    return false;
  const waitReason = session.timing?.waitReason?.trim().toLowerCase();
  if (waitReason) return waitReason === kind;
  return session.timing?.paused !== false;
}

/**
 * Scan events backward to find the latest unanswered pending elicitation.
 * The request/response events are durable interaction state, while the normal
 * AskUserQuestion tool call remains responsible for historical display.
 */
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
    // elicitation/create is a blocking interaction in ACP. Once we have
    // reached the latest elicitationRequest while scanning backward, any
    // older requests are stale and should not re-surface on reload.
    if (answeredIds.has(event.id)) return null;
    if (event.status === "pending") {
      const raw = rawObject(event.raw) ?? {};
      const requestedSchema = rawObject(raw.requestedSchema);
      const schemaSource = requestedSchema ?? raw;
      const schema: ElicitationSchema =
        schemaSource.type === "object"
          ? (schemaSource as unknown as ElicitationSchema)
          : { type: "object", properties: {} };
      return {
        interactionId: event.id,
        message: stringValue(raw.message) ?? event.content ?? "",
        requestedSchema: schema,
      };
    }
    return null;
  }
  return null;
}

export function visibleAcpBannerError(
  runtimeError: string | null | undefined,
  session: AcpSessionVm,
  events: AcpUiEventVm[],
  runtimeErrorFallback?: string | null,
  latestTurnStatus?: ConversationAttemptLifecycleVm['acp']['latestTurnStatus'],
) {
  if (runtimeError) return runtimeError;
  if (latestTurnStatus === 'completed') return null;
  if (session.diagnostics.lastError) return visibleSessionError(session, events);
  return runtimeErrorFallback ?? null;
}

export function acpSessionLoadErrorReason(
  runtimeError: string | null | undefined,
  sessionLoadError: string | null | undefined,
  session: AcpSessionVm | null | undefined,
  fallback: string,
) {
  return runtimeError ?? sessionLoadError ?? session?.diagnostics.lastError ?? fallback;
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
  if (previousItems.length === 0) {
    return nextItems.length === 0 ? previousItems : nextItems;
  }
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
  if (!previous) return next;
  if (isActivityBatch(next) || isActivityBatch(previous)) {
    if (!isActivityBatch(next) || !isActivityBatch(previous)) return next;
    const events = stabilizeTimelineItems(next.events, previous.events) as AcpTimelineEvent[];
    const live = previous.live && next.live;
    if (
      events === previous.events &&
      next.seq === previous.seq &&
      next.activityStartSeq === previous.activityStartSeq &&
      next.activityEndSeq === previous.activityEndSeq &&
      next.endedSeq === previous.endedSeq &&
      next.endedAt === previous.endedAt &&
      live === previous.live &&
      next.totalEventCount === previous.totalEventCount &&
      next.toolCallCount === previous.toolCallCount &&
      next.thoughtCount === previous.thoughtCount &&
      next.errorCount === previous.errorCount &&
      next.readFileCount === previous.readFileCount &&
      next.writtenFileCount === previous.writtenFileCount &&
      next.detailAvailable === previous.detailAvailable &&
      next.hasMoreEarlier === previous.hasMoreEarlier &&
      next.earlierCursor === previous.earlierCursor &&
      next.sessionId === previous.sessionId
    ) {
      return previous;
    }
    return { ...next, live, events };
  }
  if (isAgentLink(next) !== isAgentLink(previous)) return next;
  if (isAgentLink(next) && isAgentLink(previous)) {
    if (
      next.seq === previous.seq &&
      next.timestamp === previous.timestamp &&
      next.startedSeq === previous.startedSeq &&
      next.endedSeq === previous.endedSeq &&
      next.startedAt === previous.startedAt &&
      next.endedAt === previous.endedAt &&
      next.status === previous.status &&
      next.title === previous.title &&
      next.toolCallId === previous.toolCallId &&
      next.eventCount === previous.eventCount &&
      next.toolCallCount === previous.toolCallCount &&
      next.readFileCount === previous.readFileCount &&
      next.writtenFileCount === previous.writtenFileCount &&
      next.attention === previous.attention &&
      next.description === previous.description &&
      next.agentExecutionId === previous.agentExecutionId &&
      next.attemptId === previous.attemptId &&
      stabilizeTimelineItem(next.toolEvent, previous.toolEvent) ===
        previous.toolEvent
    ) {
      return previous;
    }
    return {
      ...next,
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

function timelineEventPosition(event: Pick<AcpUiEventVm, "seq" | "endedSeq">) {
  return event.endedSeq ?? event.seq;
}

export function timelineWindowRenderScopeKey(
  eventWindowKey: string,
  sessionId: string | null | undefined,
) {
  return `${eventWindowKey}:session:${sessionId ?? "pending"}`;
}

function timelineRenderKey(eventWindowKey: string, event: AcpTimelineItem) {
  return `${eventWindowKey}:${timelineEventKey(event)}`;
}

function latestUserPromptPosition(events: AcpUiEventVm[]) {
  let position: number | null = null;
  for (const event of events) {
    if (!isGoldBandUserPrompt(event) || isOptimisticEvent(event)) continue;
    position = Math.max(position ?? 0, timelineEventPosition(event));
  }
  return position;
}

function nextLiveStreamingMarkdownTarget(
  current: LiveStreamingMarkdownTarget | null,
  event: AcpUiEventVm,
  latestPromptPosition: number | null,
): LiveStreamingMarkdownTarget | null {
  if (event.kind === "timingUpdate") return current;
  const position = timelineEventPosition(event);
  if (event.kind === "userTextDelta") return null;
  if (event.kind === "textDelta" || event.kind === "thoughtDelta") {
    if (!hasVisibleAcpTextContent(event.content)) return current;
    if (latestPromptPosition != null && position <= latestPromptPosition) return null;
    return { key: `${event.kind}-${event.id}`, position };
  }
  return current && position < current.position ? current : null;
}

function buildAcpTimelineProjection(
  events: AcpUiEventVm[],
  sessionStatus?: string | null,
  persistedProjection?: AcpTimelineProjectionVm | null,
): AcpTimelineProjection {
  const persistedAgents = new Map(
    (persistedProjection?.agents ?? []).flatMap((agent) => [
      [agentProjectionKey(agent.agentExecutionId, agent.attemptId), agent] as const,
      [agent.agentExecutionId, agent] as const,
    ]),
  );
  const topLevelPlan = events.reduce<AcpUiEventVm | null>((latest, event) => {
    if (event.kind !== "plan") return latest;
    return !latest || timelineEventPosition(event) >= timelineEventPosition(latest)
      ? event
      : latest;
  }, null);
  const flatTimeline = buildFlatAcpTimeline(events);
  const linkedTimeline = projectAgentLinks(flatTimeline, sessionStatus, persistedAgents);
  return {
    timeline: batchAcpActivities(
      linkedTimeline,
      isSessionActiveStatus(sessionStatus),
    ),
    todoEntries: persistedProjection
      ? persistedProjection.todoEntries
      : topLevelPlan
        ? planEntries(topLevelPlan)
        : [],
  };
}

function buildAcpTimeline(events: AcpUiEventVm[]): AcpTimelineItem[] {
  return buildAcpTimelineProjection(events).timeline;
}

function buildFlatAcpTimeline(events: AcpUiEventVm[]) {
  const timeline: AcpTimelineEvent[] = [];
  const toolIndex = new Map<string, AcpTimelineEvent>();
  const seenUserPrompts = new Map<string, AcpTimelineEvent>();
  for (const event of events) {
    if (!isRenderableEvent(event)) continue;
    if (event.kind === "userTextDelta") {
      const key = userPromptDedupKey(event);
      const previousPrompt = key ? seenUserPrompts.get(key) : undefined;
      if (previousPrompt) {
        // Compatibility for historical timelines written before promptId had
        // one canonical event ID. Apply the same monotonic snapshot reducer so
        // an older physical retry event cannot reopen a settled footer.
        const merged = mergeAcpEventSnapshots(previousPrompt, event);
        if (merged !== previousPrompt) {
          previousPrompt.endedSeq = merged.endedSeq ?? previousPrompt.endedSeq;
          previousPrompt.endedAt = merged.endedAt ?? previousPrompt.endedAt;
          previousPrompt.status = merged.status ?? previousPrompt.status;
          previousPrompt.raw = mergeRaw(previousPrompt.raw, merged.raw);
        }
        continue;
      }
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
      !isAgentLink(previous) &&
      previous.kind === event.kind &&
      isMergeableDelta(event.kind) &&
      isSameDeltaStream(previous, event)
    ) {
      const merged = mergeAcpEventSnapshots(previous, event);
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
    if (event.kind === "plan") continue;
    const timelineEvent: AcpTimelineEvent = {
      ...event,
      startedAt: event.startedAt ?? event.timestamp,
      endedAt: event.endedAt ?? event.timestamp,
      startedSeq: event.startedSeq ?? originalSeqFromAcpEvent(event),
      endedSeq: event.endedSeq ?? originalSeqFromAcpEvent(event),
      optimistic: isOptimisticEvent(event),
    };
    timeline.push(timelineEvent);
    if (event.kind === "userTextDelta") {
      const key = userPromptDedupKey(event);
      if (key) seenUserPrompts.set(key, timelineEvent);
    }
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

function projectAgentLinks(
  events: AcpTimelineEvent[],
  sessionStatus?: string | null,
  persistedAgents = new Map<string, AcpAgentExecutionVm>(),
): AcpTimelineItem[] {
  return events.map((event): AcpTimelineItem => {
    const agentExecutionId = launchedAgentExecutionId(event);
    if (!agentExecutionId) return event;
    const eventAttemptId = attemptIdFromAcpEvent(event);
    const persisted = persistedAgents.get(agentProjectionKey(agentExecutionId, eventAttemptId))
      ?? persistedAgents.get(agentExecutionId);
    const status = persisted?.executionStatus
      ?? fallbackAgentExecutionStatus(sessionStatus, event.status);
    const terminal = isTerminalToolStatus(status);
    const startSeq = event.startedSeq ?? event.seq;
    const endSeq = event.endedSeq ?? event.seq;
    return {
      kind: "agentLink",
      id: `agent-link-${agentExecutionId}`,
      seq: startSeq,
      timestamp: event.startedAt ?? event.timestamp,
      startedSeq: startSeq,
      endedSeq: terminal ? endSeq : undefined,
      startedAt: event.startedAt ?? event.timestamp,
      endedAt: terminal ? event.endedAt : undefined,
      status,
      title: event.title,
      toolCallId: event.toolCallId,
      agentExecutionId,
      attemptId: persisted?.attemptId ?? eventAttemptId,
      parentAgentExecutionId: persisted?.parentAgentExecutionId,
      attention: persisted?.hasAttention ?? false,
      description: persisted?.description,
      toolEvent: event,
      eventCount: persisted?.eventCount ?? 0,
      toolCallCount: persisted?.toolCallCount ?? 0,
      readFileCount: persisted?.readFileCount ?? 0,
      writtenFileCount: persisted?.writtenFileCount ?? 0,
    };
  });
}

function fallbackAgentExecutionStatus(
  sessionStatus?: string | null,
  launchStatus?: string | null,
) {
  if (toolStatusTone(launchStatus) === "danger") return "failed";
  if (!isSessionTerminalStatus(sessionStatus)) return "queued";
  return isSessionCompletedStatus(sessionStatus) ? "completed" : "interrupted";
}

function agentProjectionKey(agentExecutionId: string, attemptId?: string | null) {
  return attemptId ? `${attemptId}:${agentExecutionId}` : agentExecutionId;
}

function isActivityEvent(item: AcpTimelineItem): item is AcpTimelineEvent {
  if (isAgentLink(item) || isActivityBatch(item)) return false;
  return (
    item.kind === "activitySummary" ||
    item.kind === "thoughtDelta" ||
    item.kind === "toolCall" ||
    item.kind === "toolCallUpdate" ||
    item.kind === "error"
  );
}

function batchAcpActivities(
  items: AcpTimelineItem[],
  sessionActive: boolean,
): AcpTimelineItem[] {
  const result: AcpTimelineItem[] = [];
  let activityEvents: AcpTimelineEvent[] = [];
  const flush = (live = false) => {
    if (activityEvents.length === 0) return;
    const first = activityEvents[0];
    const last = activityEvents[activityEvents.length - 1];
    const activityMeta = rawObject(rawObject(first.raw)?.goldBandActivity);
    const activityStartSeq = numberValue(activityMeta?.activityStartSeq)
      ?? first.startedSeq
      ?? first.seq;
    const auditEvents = activityEvents.filter((event) => event.kind !== "activitySummary");
    const retainedEvents = auditEvents.slice(-ACP_ACTIVITY_DETAIL_PAGE_SIZE);
    const toolIds = new Set(
      auditEvents
        .filter((event) => event.kind === "toolCall" || event.kind === "toolCallUpdate")
        .map((event) => event.toolCallId ?? timelineEventKey(event)),
    );
    result.push({
      kind: "activityBatch",
      id: `activity-${activityStartSeq}`,
      seq: first.startedSeq ?? first.seq,
      timestamp: first.startedAt ?? first.timestamp,
      startedSeq: first.startedSeq ?? first.seq,
      endedSeq: last.endedSeq ?? last.seq,
      startedAt: first.startedAt ?? first.timestamp,
      endedAt: last.endedAt ?? last.timestamp,
      live,
      events: retainedEvents,
      activityStartSeq,
      activityEndSeq: numberValue(activityMeta?.activityEndSeq)
        ?? last.endedSeq
        ?? last.seq,
      totalEventCount: numberValue(activityMeta?.totalEventCount) ?? auditEvents.length,
      toolCallCount: numberValue(activityMeta?.toolCallCount) ?? toolIds.size,
      thoughtCount: numberValue(activityMeta?.thoughtCount)
        ?? auditEvents.filter((event) => event.kind === "thoughtDelta").length,
      errorCount: numberValue(activityMeta?.errorCount)
        ?? auditEvents.filter((event) => event.kind === "error").length,
      readFileCount: numberValue(activityMeta?.readFileCount) ?? 0,
      writtenFileCount: numberValue(activityMeta?.writtenFileCount) ?? 0,
      detailAvailable: activityMeta?.detailAvailable === true || auditEvents.length > 0,
      hasMoreEarlier: activityMeta?.hasMoreEarlier === true || auditEvents.length > retainedEvents.length,
      earlierCursor: stringValue(activityMeta?.earlierCursor),
      sessionId: last.sessionId ?? first.sessionId ?? null,
    });
    activityEvents = [];
  };
  for (const item of items) {
    if (isActivityEvent(item)) {
      activityEvents.push(item);
      continue;
    }
    flush(false);
    result.push(item);
  }
  flush(sessionActive);
  return result;
}

function isRenderableEvent(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  if (raw?.hiddenFromChat === true) return false;
  if (
    (event.kind === "textDelta" || event.kind === "thoughtDelta")
    && !hasVisibleAcpTextContent(event.content)
  ) return false;
  if (event.kind === "permissionRequest" || event.kind === "elicitationRequest" || event.kind === "elicitationResponse") return false;
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
  if (isProviderHistoryUserPrompt(event)) return `${attemptId}:event:${event.id}`;
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

function contextCompactionData(event: AcpUiEventVm) {
  const raw = rawObject(event.raw);
  const data = rawObject(raw?.contextCompaction);
  return {
    phase: stringValue(data?.phase),
    contextUsedBefore: numberValue(data?.contextUsedBefore),
    contextSize: numberValue(data?.contextSize),
    contextUsedAfter: numberValue(data?.contextUsedAfter),
    reason: stringValue(data?.reason),
  };
}

export function contextCompactionUsageBefore(
  event: AcpUiEventVm,
): { used: string; size: string } | null {
  const data = contextCompactionData(event);
  if (data.contextUsedBefore == null) return null;
  return {
    used: formatTokenCount(data.contextUsedBefore),
    size: data.contextSize != null ? formatTokenCount(data.contextSize) : "--",
  };
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
  return mergeRawObject(previous, next);
}

export function mergeAcpEvents(previous: AcpUiEventVm[], next: AcpUiEventVm[]) {
  return mergeAcpEventWindows(previous, next, alignAcpDisplaySeq);
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

export function isAcpConversationAtBottom(
  viewportAtBottom: boolean,
  hasNewerEvents: boolean,
) {
  return viewportAtBottom && !hasNewerEvents;
}

function acpAuditSeqBounds(events: AcpUiEventVm[]) {
  if (events.length === 0) return { oldestSeq: null, newestSeq: null };
  let oldestSeq = Number.POSITIVE_INFINITY;
  let newestSeq = Number.NEGATIVE_INFINITY;
  for (const event of events) {
    const seq = originalSeqFromAcpEvent(event);
    oldestSeq = Math.min(oldestSeq, seq);
    newestSeq = Math.max(newestSeq, seq);
  }
  return { oldestSeq, newestSeq };
}

function acpPaginationSeqBounds(
  events: AcpUiEventVm[],
  attemptId?: string,
) {
  if (!attemptId) return acpAuditSeqBounds(events);
  return acpAuditSeqBounds(
    events.filter((event) => attemptIdFromAcpEvent(event) === attemptId),
  );
}

function createLiveAcpSessionShell(events: AcpUiEventVm[], status: string): AcpSessionVm {
  const first = events[0] ?? null;
  const last = events.at(-1) ?? first;
  const auditBounds = acpAuditSeqBounds(events);
  const timing = latestSessionTimingFromEvents(events);
  const session: AcpSessionVm = {
    branchId: 'root',
    parentBranchId: null,
    readOnly: false,
    sessionId: last?.sessionId ?? first?.sessionId ?? null,
    provider: "acp",
    status,
    sessionStartedAt: first?.startedAt ?? first?.timestamp ?? null,
    sessionUpdatedAt: last?.endedAt ?? last?.timestamp ?? null,
    sessionElapsedSeconds: timing?.sessionElapsedSeconds ?? calculateSessionElapsedSeconds(events, status),
    timing,
    restored: false,
    events,
    timelineProjection: null,
    eventPage: {
      loadedCount: events.length,
      total: events.length,
      oldestSeq: auditBounds.oldestSeq,
      newestSeq: auditBounds.newestSeq,
      hasOlder: false,
      hasNewer: false,
      oldestCursor: auditBounds.oldestSeq !== null
        ? formatTimelineCursor(auditBounds.oldestSeq)
        : null,
      newestCursor: auditBounds.newestSeq !== null
        ? formatTimelineCursor(auditBounds.newestSeq)
        : null,
    },
    pendingInteractions: [],
    diagnostics: {
      rawFrameCount: 0,
      eventCount: events.length,
      errorCount: 0,
    },
  };
  return applyPendingInteractionEventsToSession(session, events) ?? session;
}

function createVisibleAcpSession(
  session: AcpSessionVm,
  loadedEvents: AcpUiEventVm[],
  eventPageSize: number,
  projectionMode: "live-head" | "historical",
): AcpSessionVm {
  const visibleEvents = projectionMode === "historical"
    ? loadedEvents
    : mergeAcpEvents(session.events, loadedEvents);
  const limitedEvents = limitAcpEvents(visibleEvents, "start", eventPageSize);
  return {
    ...session,
    events: limitedEvents,
    eventPage: session.eventPage,
  };
}

function latestSessionTimingFromEvents(events: AcpUiEventVm[]): AcpSessionTimingVm | null {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const timing = events[index]?.timing;
    if (!timing) continue;
    return {
      sessionElapsedSeconds: timing.sessionElapsedSeconds,
      revision: timing.revision ?? null,
      observedAt: timing.observedAt ?? null,
      activeTurnStartedAt: timing.activeTurnStartedAt ?? null,
      activeTurnLastActivityAt: timing.activeTurnLastActivityAt ?? null,
      permissionWaitStartedAt: timing.permissionWaitStartedAt ?? null,
      userWaitStartedAt: timing.userWaitStartedAt ?? null,
      waitReason: timing.waitReason ?? null,
      paused: timing.paused,
    };
  }
  return null;
}

function latestLiveSessionTimingFromEvents(events: AcpUiEventVm[]): AcpSessionTimingVm | null {
  return latestSessionTimingFromEvents(
    events.filter((event) => {
      if (event.kind === "timingUpdate") return true;
      if (
        event.kind !== "permissionRequest" &&
        event.kind !== "elicitationRequest" &&
        event.kind !== "elicitationResponse"
      ) {
        return false;
      }
      return isVersionedLiveTimingPatch(event.timing);
    }),
  );
}

function isAgentBranchResultEvent(event: AcpUiEventVm) {
  if (!event.raw || typeof event.raw !== "object" || Array.isArray(event.raw)) return false;
  return (event.raw as Record<string, unknown>).source === "agentBranchResult";
}

function latestAgentBranchResult(events: AcpUiEventVm[]) {
  for (let index = events.length - 1; index >= 0; index -= 1) {
    if (isAgentBranchResultEvent(events[index])) return events[index];
  }
  return null;
}

function applyAgentBranchResultToSession(
  session: AcpSessionVm | null | undefined,
  result: AcpUiEventVm,
): AcpSessionVm | null {
  if (!session || !isAgentBranchResultEvent(result)) return session ?? null;
  return {
    ...session,
    status: "completed",
    pendingInteractions: [],
    sessionUpdatedAt: result.endedAt ?? result.timestamp ?? session.sessionUpdatedAt,
    timing: session.timing
      ? {
          ...session.timing,
          activeTurnStartedAt: null,
          activeTurnLastActivityAt: null,
          permissionWaitStartedAt: null,
          userWaitStartedAt: null,
          waitReason: null,
          paused: true,
        }
      : session.timing,
  };
}

function isVersionedLiveTimingPatch(timing: AcpUiEventVm["timing"]) {
  return timing?.revision != null && Boolean(timing.observedAt);
}

export function shouldAwaitTerminalAcpStop(
  session: Pick<AcpSessionVm, "sessionId" | "status"> | null | undefined,
) {
  return Boolean(session?.sessionId) && !isSessionTerminalStatus(session?.status);
}

export function planAcpStopResponse(result: {
  status?: string;
  session?: AcpSessionVm | null;
  lifecycle?: { acp: { stopping: boolean } } | null;
}) {
  const accepted = result.status === "accepted";
  return {
    accepted,
    awaitTerminal: accepted
      ? Boolean(result.lifecycle?.acp.stopping)
      : shouldAwaitTerminalAcpStop(result.session),
    sessionSnapshot: result.session ?? undefined,
  };
}

/**
 * AI-DYNAMIC stores the final control-output annotation after the last live
 * text delta. A terminal lifecycle-only notification is the bounded signal to
 * re-query the selected root session body. Direct and normal Workflow attempts
 * have no outer locator and keep their existing subscription behavior.
 */
export function shouldRefreshDynamicTerminalSessionContent(
  event: Pick<
    AcpSessionUpdatedEventVm,
    'outerNodeId' | 'outerAttemptId' | 'event' | 'session' | 'lifecycle'
  >,
  branchId: string,
) {
  return Boolean(
    branchId === 'root'
    && event.outerNodeId
    && event.outerAttemptId
    && !event.event
    && !event.session
    && event.lifecycle
    && !event.lifecycle.runtime.active
    && event.lifecycle.runtime.phase === 'terminal'
  );
}

function partitionAcpLiveTimingUpdates(events: AcpUiEventVm[]) {
  const timingUpdates: AcpUiEventVm[] = [];
  const timelineUpdates: AcpUiEventVm[] = [];
  for (const event of events) {
    if (event.kind === "timingUpdate") timingUpdates.push(event);
    else timelineUpdates.push(event);
  }
  return { timingUpdates, timelineUpdates };
}

function liveTimelineUpdatesFromEvents(events: AcpUiEventVm[]) {
  return events.filter(
    (event) =>
      isRenderableEvent(event) ||
      event.kind === "permissionRequest" ||
      event.kind === "elicitationRequest" ||
      event.kind === "elicitationResponse",
  );
}

export function projectAcpSessionControlEvents(
  session: AcpSessionVm | null | undefined,
  events: AcpUiEventVm[],
  lifecycle?: ConversationAttemptLifecycleVm | null,
): AcpSessionVm | null {
  if (!session) return session ?? null;
  const latestTiming = latestLiveSessionTimingFromEvents(events);
  const branchResult = latestAgentBranchResult(events);
  const hasUsageUpdate = events.some((event) => event.kind === "usageUpdate");
  const hasPromptInteractionLifecycleUpdate = events.some(
    (event) => event.kind === "permissionRequest"
      || event.kind === "elicitationRequest"
      || event.kind === "elicitationResponse",
  );
  let projected = latestTiming
    ? stabilizeAcpSessionTimingPatchForDisplay(session, latestTiming)
    : session;
  if (branchResult) projected = applyAgentBranchResultToSession(projected, branchResult);
  if (hasUsageUpdate) projected = projectLatestAcpUsageUpdate(projected, events);
  if (hasPromptInteractionLifecycleUpdate) {
    projected = applyPendingInteractionEventsToSession(projected, events);
  }
  return settlePendingAcpInteractionsForLifecycle(projected, lifecycle);
}

function calculateSessionElapsedSeconds(events: AcpUiEventVm[], status: string) {
  let elapsedSeconds = 0;
  let turnStartedAt: number | null = null;
  let turnLastEventAt: number | null = null;
  let sawTurn = false;

  const finishTurn = (active: boolean) => {
    if (turnStartedAt == null) return 0;
    const endAt = active ? Date.now() : (turnLastEventAt ?? turnStartedAt);
    return Math.max(0, Math.floor((endAt - turnStartedAt) / 1000));
  };

  for (const event of events) {
    const timestamp = parseAcpTimestamp(event.timestamp);
    if (isGoldBandUserPrompt(event)) {
      elapsedSeconds += finishTurn(false);
      turnStartedAt = timestamp;
      turnLastEventAt = null;
      sawTurn = timestamp != null;
      continue;
    }
    if (turnStartedAt == null || timestamp == null) continue;
    if (isSessionElapsedProgressEvent(event)) {
      turnLastEventAt = timestamp;
    }
  }

  if (!sawTurn) return null;
  return elapsedSeconds + finishTurn(isSessionActiveStatus(status));
}

function isSessionElapsedProgressEvent(event: AcpUiEventVm) {
  const sessionUpdate = stringValue(rawObject(event.raw)?.sessionUpdate);
  return ![
    "available_commands_update",
    "current_mode_update",
    "session_info_update",
  ].includes(sessionUpdate ?? "");
}

export function mergeOptimisticSession(
  session: AcpSessionVm | null | undefined,
  optimisticEvents: AcpUiEventVm[],
): AcpSessionVm | null {
  if (!session || optimisticEvents.length === 0) return session ?? null;
  const pending = optimisticEvents.filter((event) =>
    shouldMergeOptimisticEvent(session.events, event),
  );
  if (pending.length === 0) return session;
  const events = [...session.events];
  for (const event of pending) {
    const afterSeq = optimisticPromptAfterSeq(event);
    if (afterSeq === null) {
      events.push(event);
      continue;
    }
    const insertAt = events.findIndex((candidate) => (
      !isOptimisticEvent(candidate)
      && timelineEventPosition(candidate) > afterSeq
    ));
    if (insertAt < 0) events.push(event);
    else events.splice(insertAt, 0, event);
  }
  return { ...session, events };
}

function stabilizeAcpSessionTimingForDisplay(
  previous: AcpSessionVm | null | undefined,
  next: AcpSessionVm | null | undefined,
): AcpSessionVm | null {
  if (!next) return next ?? null;
  if (!previous || !isSameAcpSessionForTiming(previous, next)) return next;
  const previousSeconds = acpSessionDisplaySeconds(previous);
  const nextSeconds = acpSessionDisplaySeconds(next);
  if (shouldAcceptAcpSessionTiming(previous.timing, next.timing)) {
    return next;
  }
  if (
    hasVersionedAcpSessionTiming(previous.timing) ||
    hasVersionedAcpSessionTiming(next.timing)
  ) {
    return mergeAcpSessionWithPreviousTiming(previous, next);
  }
  if (previousSeconds == null || nextSeconds == null || nextSeconds >= previousSeconds) {
    return next;
  }
  return mergeAcpSessionWithPreviousTiming(previous, next);
}

function stabilizeAcpSessionTimingPatchForDisplay(
  previous: AcpSessionVm | null | undefined,
  timing: AcpSessionTimingVm,
): AcpSessionVm | null {
  if (!previous) return previous ?? null;
  return stabilizeAcpSessionTimingForDisplay(previous, {
    ...previous,
    sessionElapsedSeconds: timing.sessionElapsedSeconds,
    timing,
  });
}

function mergeAcpSessionWithPreviousTiming(previous: AcpSessionVm, next: AcpSessionVm) {
  const previousSeconds = acpSessionDisplaySeconds(previous);
  const nextSeconds = acpSessionDisplaySeconds(next);
  const stableSeconds = Math.max(
    previousSeconds ?? 0,
    nextSeconds ?? 0,
    next.sessionElapsedSeconds ?? 0,
  );
  return {
    ...next,
    sessionElapsedSeconds: stableSeconds,
    timing: next.timing
      ? {
          ...next.timing,
          ...(previous.timing ?? {}),
          sessionElapsedSeconds: stableSeconds,
        }
      : previous.timing
        ? {
            ...previous.timing,
            sessionElapsedSeconds: stableSeconds,
          }
        : null,
  };
}

function reconcileAcpSessionForDisplay(
  previous: AcpSessionVm | null | undefined,
  next: AcpSessionVm | null | undefined,
): AcpSessionVm | null {
  if (!next) return next ?? null;
  const timingStable = stabilizeAcpSessionTimingForDisplay(previous, next);
  return preserveAcpSessionMetadataForDisplay(previous, timingStable);
}

function reconcileCanonicalAcpSessionForDisplay(
  previous: AcpSessionVm | null | undefined,
  canonical: AcpSessionVm,
) {
  if (
    !previous
    || !isSameAcpSessionForMetadata(previous, canonical)
    || !shouldPreferObservedAcpSessionOverCanonicalResponse(canonical, previous)
  ) {
    return reconcileAcpSessionForDisplay(previous, canonical) ?? canonical;
  }
  const latestDisplay = reconcileAcpSessionForDisplay(canonical, previous)
    ?? previous;
  return {
    ...latestDisplay,
    events: mergeAcpEvents(canonical.events, previous.events),
    // Display events may include transient Router replay. Keep the page from
    // the canonical response so it can never become ACK coverage evidence.
    eventPage: canonical.eventPage,
  };
}

/**
 * A terminal ACP lifecycle settles the transient interaction projection for
 * that turn even when the session body carrying the corresponding Timeline
 * settlement has not arrived yet. Callers must pass the monotonically merged
 * lifecycle so a late terminal revision cannot clear a newer active turn.
 */
export function settlePendingAcpInteractionsForLifecycle(
  session: AcpSessionVm | null | undefined,
  lifecycle: Pick<ConversationAttemptLifecycleVm, 'acp'> | null | undefined,
): AcpSessionVm | null {
  if (!session || !isTerminalAcpLifecycle(lifecycle)) return session ?? null;
  const terminalTurnId = lifecycle?.acp.turnId ?? null;
  const pendingInteractions = session.pendingInteractions.filter((interaction) => {
    if (!terminalTurnId || !interaction.turnId) return false;
    return interaction.turnId !== terminalTurnId;
  });
  if (pendingInteractions.length === session.pendingInteractions.length) {
    return session;
  }
  return {
    ...session,
    pendingInteractions,
  };
}

function preserveAcpSessionMetadataForDisplay(
  previous: AcpSessionVm | null | undefined,
  next: AcpSessionVm | null,
): AcpSessionVm | null {
  if (!next || !previous || !isSameAcpSessionForMetadata(previous, next)) {
    return next;
  }

  const preserveSystemPrompt =
    !next.systemPromptAppend?.trim() && Boolean(previous.systemPromptAppend?.trim());
  const preserveConfig = shouldPreferAcpSessionConfig(previous.config, next.config);
  const timelineGenerationChanged = previous.eventPage.generation != null
    && next.eventPage.generation != null
    && previous.eventPage.generation !== next.eventPage.generation;
  const preserveGoldBandPrompts =
    !timelineGenerationChanged &&
    previous.events.some(isGoldBandUserPrompt) &&
    !next.events.some(isGoldBandUserPrompt);
  const pendingProjectionAdvanced = hasAdvancedAcpSessionProjection(previous, next);
  const preservePendingInteractions = shouldPreservePendingInteractions(
    previous,
    next,
    pendingProjectionAdvanced,
  );

  if (
    !preserveSystemPrompt &&
    !preserveConfig &&
    !preserveGoldBandPrompts &&
    !preservePendingInteractions
  ) {
    return next;
  }

  const events = preserveGoldBandPrompts
    ? mergeAcpEvents(
        previous.events.filter(isGoldBandUserPrompt),
        next.events,
      )
    : next.events;

  return {
    ...next,
    sessionId: next.sessionId ?? previous.sessionId,
    title: next.title ?? previous.title,
    adapterId: next.adapterId ?? previous.adapterId,
    adapterDisplayName: next.adapterDisplayName ?? previous.adapterDisplayName,
    systemPromptAppend: preserveSystemPrompt
      ? previous.systemPromptAppend
      : next.systemPromptAppend,
    config: preserveConfig
      ? mergeAcpSessionConfigForDisplay(previous.config, next.config)
      : next.config,
    pendingInteractions: preservePendingInteractions
      ? previous.pendingInteractions
      : next.pendingInteractions,
    events,
    eventPage: next.eventPage,
  };
}

function hasAdvancedAcpSessionProjection(
  previous: AcpSessionVm,
  next: AcpSessionVm,
) {
  const previousGeneration = previous.eventPage.generation;
  const nextGeneration = next.eventPage.generation;
  if (previousGeneration != null && nextGeneration != null) {
    if (nextGeneration > previousGeneration) return true;
    if (nextGeneration < previousGeneration) return false;
  } else if (previousGeneration == null && nextGeneration != null) {
    return true;
  }
  for (const [previousValue, nextValue] of [
    [previous.eventPage.coveredRevision, next.eventPage.coveredRevision],
    [previous.eventPage.newestRevision, next.eventPage.newestRevision],
    [previous.eventPage.newestSeq, next.eventPage.newestSeq],
  ] as const) {
    if (nextValue != null && (previousValue == null || nextValue > previousValue)) {
      return true;
    }
  }
  return false;
}

function shouldPreservePendingInteractions(
  previous: AcpSessionVm,
  next: AcpSessionVm,
  projectionAdvanced: boolean,
) {
  if (projectionAdvanced) return false;
  if (
    previous.pendingInteractions.length === 0
    && next.pendingInteractions.length > 0
  ) {
    return true;
  }
  if (
    previous.pendingInteractions.length === 0
    || next.pendingInteractions.length > 0
    || !isSessionActiveStatus(next.status)
  ) return false;
  const pendingIds = new Set(previous.pendingInteractions.map(
    (interaction) => interaction.interactionId,
  ));
  return !next.events.some((event) => {
    if (event.kind === "elicitationResponse") {
      const elicitationId =
        stringValue(rawObject(event.raw)?.elicitationId) ??
        event.id.replace(/-response$/, "");
      return pendingIds.has(elicitationId);
    }
    if (
      event.kind === "elicitationRequest"
      && pendingIds.has(event.id)
      && event.status?.toLowerCase() !== "pending"
    ) return true;
    if (event.kind !== "permissionRequest") return false;
    return pendingIds.has(permissionRequestIdFromEvent(event))
      && event.status?.toLowerCase() !== "pending";
  });
}

function shouldPreferAcpSessionMetadata(
  previous: AcpSessionVm,
  next: AcpSessionVm,
) {
  if (!isSameAcpSessionForMetadata(previous, next)) return false;
  if (previous.systemPromptAppend?.trim() && !next.systemPromptAppend?.trim()) {
    return true;
  }
  if (shouldPreferAcpSessionConfig(previous.config, next.config)) return true;
  return previous.events.some(isGoldBandUserPrompt) && !next.events.some(isGoldBandUserPrompt);
}

function shouldPreferAcpSessionConfig(
  previous: AcpSessionVm["config"] | null | undefined,
  next: AcpSessionVm["config"] | null | undefined,
) {
  return hasAcpSessionConfigChoicesForDisplay(previous) && !hasAcpSessionConfigChoicesForDisplay(next);
}

function mergeAcpSessionConfigForDisplay(
  previous: AcpSessionVm["config"] | null | undefined,
  next: AcpSessionVm["config"] | null | undefined,
) {
  if (!previous) return next ?? null;
  if (!next) return previous;
  return {
    ...previous,
    ...next,
    models: hasGroupedConfigChoices(next.models, "availableModels")
      ? next.models
      : previous.models,
    modes: hasGroupedConfigChoices(next.modes, "availableModes")
      ? next.modes
      : previous.modes,
    configOptions: hasAnySelectConfigChoices(next.configOptions)
      ? next.configOptions
      : previous.configOptions,
  };
}

function hasAcpSessionConfigChoicesForDisplay(
  config: AcpSessionVm["config"] | null | undefined,
) {
  if (!config) return false;
  const hasModelChoices =
    hasGroupedConfigChoices(config.models, "availableModels") ||
    hasSelectConfigChoices(config.configOptions, "model") ||
    Boolean(config.currentModelId);
  const hasModeChoices =
    hasGroupedConfigChoices(config.modes, "availableModes") ||
    hasSelectConfigChoices(config.configOptions, "mode") ||
    Boolean(config.currentModeId);
  return hasModelChoices && hasModeChoices;
}

function hasAnySelectConfigChoices(value: unknown) {
  return hasSelectConfigChoices(value, "model") || hasSelectConfigChoices(value, "mode");
}

function isSameAcpSessionForMetadata(previous: AcpSessionVm, next: AcpSessionVm) {
  if (previous.sessionId && next.sessionId && previous.sessionId !== next.sessionId) {
    return false;
  }
  return true;
}

function shouldAcceptAcpSessionTiming(
  previous: AcpSessionTimingVm | null | undefined,
  next: AcpSessionTimingVm | null | undefined,
) {
  if (!next || !previous) return true;
  const previousRevision = previous.revision ?? null;
  const nextRevision = next.revision ?? null;
  if (previousRevision != null && nextRevision != null) {
    if (nextRevision !== previousRevision) return nextRevision > previousRevision;
    const previousObservedAt = parseAcpTimestamp(previous.observedAt);
    const nextObservedAt = parseAcpTimestamp(next.observedAt);
    if (previousObservedAt != null && nextObservedAt != null) {
      return nextObservedAt >= previousObservedAt;
    }
    return true;
  }
  if (previousRevision != null && nextRevision == null) return false;
  const previousSeconds = previous.sessionElapsedSeconds;
  const nextSeconds = next.sessionElapsedSeconds;
  return previousSeconds == null || nextSeconds == null || nextSeconds >= previousSeconds;
}

function hasVersionedAcpSessionTiming(timing: AcpSessionTimingVm | null | undefined) {
  return timing?.revision != null;
}

function isSameAcpSessionForTiming(previous: AcpSessionVm, next: AcpSessionVm) {
  if (previous.sessionId && next.sessionId) {
    return previous.sessionId === next.sessionId;
  }
  return true;
}

function acpSessionDisplaySeconds(session: Pick<AcpSessionVm, "timing" | "sessionElapsedSeconds">) {
  return session.timing?.sessionElapsedSeconds ?? session.sessionElapsedSeconds ?? null;
}

function isAcpInitialSessionReady(session: AcpSessionVm) {
  return (
    hasAcpSessionMetadata({
      systemPromptAppend: session.systemPromptAppend,
      config: session.config,
    }) && session.events.some(isGoldBandUserPrompt)
  );
}

function isAcpSessionDisplayableDuringInitialLoad(session: AcpSessionVm | null | undefined) {
  return Boolean(
    session &&
    isSessionActiveStatus(session.status) &&
    liveTimelineUpdatesFromEvents(session.events).length > 0,
  );
}

function isAcpSessionReadyForInitialDisplay(session: AcpSessionVm | null | undefined) {
  return Boolean(
    session &&
    (
      (session.branchId !== 'root' && Boolean(session.branchExecution)) ||
      isAcpInitialSessionReady(session) ||
      isAcpSessionDisplayableDuringInitialLoad(session) ||
      (isSessionTerminalStatus(session.status) && session.events.length > 0)
    ),
  );
}

function logAcpSessionReady(
  source: string,
  componentInstanceId: string,
  sessionIdentity: string,
  session: AcpSessionVm | null | undefined,
  extra?: Record<string, unknown>,
) {
  if (!isAcpSessionReadyDebugEnabled()) return;
  console.info("[GoldBand][ACP session-ready]", {
    source,
    componentInstanceId,
    sessionIdentity,
    ...summarizeAcpSessionReady(session),
    ...extra,
  });
}

function logAcpSessionReadyLifecycle(
  source: string,
  componentInstanceId: string,
  sessionIdentity: string,
  extra?: Record<string, unknown>,
) {
  if (!isAcpSessionReadyDebugEnabled()) return;
  console.info("[GoldBand][ACP session-ready]", {
    source,
    componentInstanceId,
    sessionIdentity,
    ...extra,
  });
}

function isAcpSessionReadyDebugEnabled() {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem("goldBand.debug.acpSessionReady") === "1";
  } catch {
    return false;
  }
}

function createAcpChatDialogInstanceId() {
  return `acp-chat-${Date.now().toString(36)}-${Math.random()
    .toString(36)
    .slice(2, 8)}`;
}

function createAcpSessionQueryTraceId(
  componentInstanceId: string,
  branchId: string,
  refreshSeq: number,
) {
  return `${componentInstanceId}:${branchId}:${refreshSeq}`;
}

function logAcpSessionQueryTiming(
  stage: string,
  traceId: string,
  sessionIdentity: string,
  details?: Record<string, unknown>,
) {
  if (!isAcpSessionQueryTimingDebugEnabled()) return;
  console.info("[GoldBand][ACP session-query]", {
    stage,
    traceId,
    sessionIdentity,
    ...details,
  });
}

function isAcpSessionQueryTimingDebugEnabled() {
  if (typeof window === "undefined") return false;
  try {
    return window.localStorage.getItem("goldBand.debug.acpTiming") === "1";
  } catch {
    return false;
  }
}

function summarizeAcpSessionReady(session: AcpSessionVm | null | undefined) {
  const config = session?.config ?? null;
  const hasSystemPromptAppend = Boolean(session?.systemPromptAppend?.trim());
  const hasGoldBandUserPrompt = session?.events?.some(isGoldBandUserPrompt) ?? false;
  return {
    hasSession: Boolean(session),
    ready: session ? isAcpInitialSessionReady(session) : false,
    status: session?.status ?? null,
    sessionUpdatedAt: session?.sessionUpdatedAt ?? null,
    sessionId: session?.sessionId ?? null,
    adapterId: session?.adapterId ?? null,
    adapterDisplayName: session?.adapterDisplayName ?? null,
    hasSystemPromptAppend,
    systemPromptLength: session?.systemPromptAppend?.length ?? null,
    hasConfig: Boolean(config),
    currentModelId: config?.currentModelId ?? null,
    currentModelName: config?.currentModelName ?? null,
    currentModeId: config?.currentModeId ?? null,
    currentModeName: config?.currentModeName ?? null,
    hasModelChoices:
      hasGroupedConfigChoices(config?.models, "availableModels") ||
      hasSelectConfigChoices(config?.configOptions, "model") ||
      Boolean(config?.currentModelId),
    hasModeChoices:
      hasGroupedConfigChoices(config?.modes, "availableModes") ||
      hasSelectConfigChoices(config?.configOptions, "mode") ||
      Boolean(config?.currentModeId),
    configOptionCount: Array.isArray(config?.configOptions)
      ? config.configOptions.length
      : null,
    eventCount: session?.events?.length ?? null,
    hasGoldBandUserPrompt,
    timingSeconds: session?.timing?.sessionElapsedSeconds ?? session?.sessionElapsedSeconds ?? null,
    timingRevision: session?.timing?.revision ?? null,
  };
}

function hasGroupedConfigChoices(value: unknown, key: string) {
  return Boolean(
    value &&
      typeof value === "object" &&
      !Array.isArray(value) &&
      Array.isArray((value as Record<string, unknown>)[key]) &&
      ((value as Record<string, unknown>)[key] as unknown[]).length > 0,
  );
}

function hasSelectConfigChoices(value: unknown, category: string) {
  return Boolean(
    Array.isArray(value) &&
      value.some((item) => {
        if (!item || typeof item !== "object" || Array.isArray(item)) return false;
        const option = item as Record<string, unknown>;
        const matches = option.id === category || option.category === category;
        return matches && Array.isArray(option.options) && option.options.length > 0;
      }),
  );
}

function sessionsEquivalent(
  previous: AcpSessionVm | null | undefined,
  next: AcpSessionVm | null | undefined,
) {
  if (!previous || !next) return previous === next;
  if (previous.status !== next.status) return false;
  if (previous.sessionUpdatedAt !== next.sessionUpdatedAt) return false;
  if (previous.sessionElapsedSeconds !== next.sessionElapsedSeconds) return false;
  if (acpSessionTimingSignature(previous) !== acpSessionTimingSignature(next)) return false;
  if (previous.systemPromptAppend !== next.systemPromptAppend) return false;
  if (acpSessionMetadataSignature(previous) !== acpSessionMetadataSignature(next)) return false;
  return acpSessionEventsSignature(previous) === acpSessionEventsSignature(next);
}

function acpSessionTimingSignature(session: AcpSessionVm) {
  return JSON.stringify(session.timing ?? null);
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
  timelineRenderKey,
  buildAcpTimeline,
  buildAcpTimelineProjection,
  stabilizeTimelineItems,
  nextLiveStreamingMarkdownTarget,
  calculateSessionElapsedSeconds,
  createLiveAcpSessionShell,
  createVisibleAcpSession,
  acpPaginationSeqBounds,
  applyAgentBranchResultToSession,
  latestLiveSessionTimingFromEvents,
  latestSessionTimingFromEvents,
  liveTimelineUpdatesFromEvents,
  partitionAcpLiveTimingUpdates,
  queryBlocksFromTool,
  objectiveActivityDescriptor,
  isTopLevelPlanEvent,
  hasMatchingUserPrompt,
  clearPendingOptimisticPromptsAfterStop,
  reconcileAcpSessionForDisplay,
  stabilizeAcpSessionTimingForDisplay,
  stabilizeAcpSessionTimingPatchForDisplay,
  useSessionTimingSeconds,
  isAcpSessionReadyForInitialDisplay,
};

type MessageAttachmentLocator = {
  projectId: string;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
};

export function createAcpPromptId() {
  return `acp-prompt-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function optimisticUserEvent(
  content: string,
  promptId = createAcpPromptId(),
  quotes: import('@/types').UserPromptQuote[] = [],
  afterSeq: number | null = null,
  attachments: MessageAttachmentPreview[] = [],
): AcpUiEventVm {
  const createdAt = Math.floor(Date.now() / 1000);
  return {
    id: `optimistic-user-${createdAt}-${Math.random().toString(36).slice(2)}`,
    seq: Number.MAX_SAFE_INTEGER - createdAt,
    timestamp: `${createdAt}Z`,
    kind: "userTextDelta",
    content,
    status: "sending",
    raw: {
      source: "goldBandPrompt",
      optimistic: true,
      promptId,
      optimisticAfterSeq: afterSeq,
      ...(quotes.length > 0 ? { quotes } : {}),
      ...(attachments.length > 0 ? { attachments } : {}),
    },
  };
}

const acpInvisibleTextCharacterPattern = /[\s\p{Cf}]/u;

function hasVisibleAcpTextContent(content?: string | null) {
  if (!content) return false;
  for (const character of content) {
    if (!acpInvisibleTextCharacterPattern.test(character)) return true;
  }
  return false;
}

export function optimisticAttachmentPreviews(
  attachments: readonly AttachmentItem[],
  paths: readonly string[],
): MessageAttachmentPreview[] {
  return attachments.flatMap((attachment, index) => {
    const path = paths[index];
    return path
      ? [{
          name: attachment.name,
          path,
          type: attachment.mime,
          size: attachment.size,
        }]
      : [];
  });
}

function optimisticPromptAfterSeq(event: AcpUiEventVm) {
  const value = rawObject(event.raw)?.optimisticAfterSeq;
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function latestCanonicalTimelinePosition(events: AcpUiEventVm[]) {
  let position: number | null = null;
  for (const event of events) {
    if (isOptimisticEvent(event)) continue;
    position = Math.max(position ?? 0, timelineEventPosition(event));
  }
  return position;
}

function isOptimisticEvent(event: AcpUiEventVm) {
  return rawObject(event.raw)?.optimistic === true;
}

function isPendingOptimisticPrompt(event: AcpUiEventVm) {
  return isOptimisticEvent(event)
    && (event.status === "sending" || event.status === "processing");
}

function clearPendingOptimisticPromptsAfterStop(events: AcpUiEventVm[]) {
  return events.filter(
    (event) => !isPendingOptimisticPrompt(event),
  );
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

function toolDetails(event: AcpUiEventVm, includeOutput = true) {
  const raw = rawObject(event.raw);
  const toolCall = rawObject(raw?.toolCall) ?? rawObject(raw?.content) ?? raw;
  const fields = rawObject(toolCall?.fields);
  const rawInput = rawObject(toolCall?.rawInput) ?? rawObject(raw?.rawInput);
  const toolCallInput = rawObject(toolCall?.input);
  const locations =
    arrayValue(toolCall?.locations) ?? arrayValue(raw?.locations);
  const normalizedToolOutput = goldBandConversationMeta(event)?.toolOutput;
  const title = stringValue(toolCall?.title) ?? event.title;
  const normalizedToolName = canonicalToolName(event);
  const name =
    normalizedToolName ??
    parseToolTitle(title).name ??
    stringValue(toolCall?.name) ??
    title;
  const output = includeOutput
    ? cleanToolOutput(
        toolCall?.output ??
          raw?.output ??
          fields?.output ??
          normalizedToolOutput ??
          raw?.content,
      )
    : undefined;
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
  push("acp.toolSkill", stringValue(rawInput?.skill));
  push("acp.toolArguments", stringValue(rawInput?.args));
  push("acp.toolPath", firstLocationPath(locations));
  return blocks;
}

function toolSummary(blocks: Array<{ value: string }>) {
  const values = [...new Set(
    blocks.map((block) => block.value.trim()).filter(Boolean),
  )];
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
    const text = toolContentText(value[0]);
    if (text) return text;
  }
  const text = toolContentText(value);
  if (text) return text;
  return value;
}

function toolContentText(value: unknown) {
  const item = rawObject(value);
  const directText = stringValue(item?.text);
  if (directText) return directText;
  return stringValue(rawObject(item?.content)?.text);
}

function formatToolValue(value: unknown) {
  if (value === null) return "null";
  if (value === undefined) return "undefined";
  if (typeof value === "string") return value;
  if (typeof value === "object") return JSON.stringify(value, null, 2);
  return String(value);
}

function displayRawDirection(
  t: ReturnType<typeof useTranslation>["t"],
  direction?: string | null,
) {
  if (direction === "inbound") return t("acp.rawInboundFrame");
  if (direction === "outbound") return t("acp.rawOutboundFrame");
  return direction ?? t("common.unknown");
}

function createEstablishedAcpSessionShell(
  events: AcpUiEventVm[],
  status: string,
  sessionReferenceId?: string | null,
): AcpSessionVm {
  return {
    ...createLiveAcpSessionShell(events, status),
    sessionId: sessionReferenceId ?? events.at(-1)?.sessionId ?? events[0]?.sessionId ?? null,
    restored: true,
  };
}

export const ACP_RAW_SESSION_KIND_I18N_KEYS = {
  "session/new": "acp.rawKindSessionNew",
  "session/resume": "acp.rawKindSessionResume",
  "session/load": "acp.rawKindSessionLoad",
  "session/prompt": "acp.rawKindSessionPrompt",
} as const;

function rawKindOptions(t: ReturnType<typeof useTranslation>["t"]) {
  return [
    { value: "agent_message_chunk", label: t("acp.rawKindAgentMessage") },
    { value: "agent_thought_chunk", label: t("acp.rawKindThought") },
    { value: "tool_call", label: t("acp.rawKindToolCall") },
    { value: "tool_call_update", label: t("acp.rawKindToolUpdate") },
    { value: "usage_update", label: t("acp.rawKindUsage") },
    { value: "available_commands_update", label: t("acp.rawKindCommands") },
    { value: "session/prompt", label: t(ACP_RAW_SESSION_KIND_I18N_KEYS["session/prompt"]) },
    { value: "session/new", label: t(ACP_RAW_SESSION_KIND_I18N_KEYS["session/new"]) },
    { value: "session/resume", label: t(ACP_RAW_SESSION_KIND_I18N_KEYS["session/resume"]) },
    { value: "session/load", label: t(ACP_RAW_SESSION_KIND_I18N_KEYS["session/load"]) },
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
    "session/new": t(ACP_RAW_SESSION_KIND_I18N_KEYS["session/new"]),
    "session/resume": t(ACP_RAW_SESSION_KIND_I18N_KEYS["session/resume"]),
    "session/load": t(ACP_RAW_SESSION_KIND_I18N_KEYS["session/load"]),
    "session/prompt": t(ACP_RAW_SESSION_KIND_I18N_KEYS["session/prompt"]),
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
  if (page.items.length === 0) {
    return t("acp.rawMatchCount", { total: page.total });
  }
  const lineNumbers = page.items.map((item) => item.lineNumber);
  const firstLine = Math.min(...lineNumbers);
  const lastLine = Math.max(...lineNumbers);
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
  const normalized = status?.toLowerCase();
  if (!normalized) return "muted";
  if (["pending", "sending", "queued"].includes(normalized)) return "pending";
  if (["running", "in_progress", "waiting_permission"].includes(normalized)) return "running";
  if (["completed", "success", "succeeded"].includes(normalized)) return "success";
  if (["failed", "error", "cancelled", "canceled", "interrupted"].includes(normalized)) return "danger";
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
