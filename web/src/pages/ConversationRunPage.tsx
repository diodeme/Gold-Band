import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Button } from '@/components/ui/button';
import {
  ACPChatDialog,
  createAcpEventWindowCacheKey,
  hasHydratedAcpSessionContent,
  type AcpInitialSessionQueryState,
  type AcpLifecycleSnapshot,
  type AcpRuntimeComposerContext,
} from '@/components/acp/ACPChatDialog';
import { BrandLoadingState } from '@/components/BrandLoadingState';
import { ConversationRunHeader } from '@/components/conversation/ConversationRunHeader';
import { ConversationSessionSwitcher } from '@/components/conversation/ConversationSessionSwitcher';
import { useThemeWallpaperSurface } from '@/components/theme/ThemeAssetsContext';
import { confirmCloseConversationRunWorkspaceResource, ConversationRunWorkspaceResourcePanel } from '@/components/workspace/ConversationRunWorkspaceResourcePanel';
import { conversationRunWorkspaceResourceKey, useRightWorkspace, type ConversationDirectoryWorkspaceEntry, type RightWorkspaceResource } from '@/components/workspace/right-workspace-context';
import { canViewConversationRuntimeWorkflow, conversationSessionLeafForGraphNode } from '@/lib/conversation-runtime-workflow';
import { conversationPageForSession } from '@/lib/conversation-navigation';
import { findConversationLeafByKey } from '@/lib/conversation-run-snapshot';
import { acpProviderConfigCatalog } from '@/lib/acp-session-config';
import { acpRuntimeErrorBannerCopy } from '@/lib/acp-runtime-error';
import {
  conversationRunCacheKey,
  type ConversationSessionTreeExpansion,
} from '@/lib/conversation-run-cache';
import {
  isRuntimeControlledConversationLifecycle,
  isTerminalConversationSessionStatus,
  type ConversationSessionFollowMode,
} from '@/lib/conversation-session-follow';
import { pathFromRoute, taskListPage } from '@/routes';
import type { AcpSessionVm, AgentRegistryVm, AppConfigVm, ConversationRunVm, ConversationSessionLeafVm, GraphNodeVm, WorkflowModelBindings, WorkflowVm } from '../types';

function activeSessionKey(session: {
  roundId: string;
  nodeId: string;
  attemptId: string;
  outerNodeId?: string | null;
  outerAttemptId?: string | null;
}) {
  if (session.outerNodeId && session.outerAttemptId) {
    return `${session.roundId}/${session.outerNodeId}/${session.outerAttemptId}/${session.nodeId}/${session.attemptId}`;
  }
  return `${session.roundId}/${session.nodeId}/${session.attemptId}`;
}

export function sessionBelongsToLeaf(session: AcpSessionVm | null | undefined, run: ConversationRunVm, leaf: ConversationSessionLeafVm | null) {
  if (!session || !leaf) return true;
  if (session.roundId && session.nodeId && session.attemptId) {
    return session.roundId === leaf.roundId &&
      session.nodeId === leaf.nodeId &&
      session.attemptId === leaf.attemptId &&
      (session.outerNodeId ?? null) === (leaf.outerNodeId ?? null) &&
      (session.outerAttemptId ?? null) === (leaf.outerAttemptId ?? null);
  }
  if (!session.cwd) return true;
  const cwd = normalizeSessionPath(session.cwd);
  const expected = leaf.outerNodeId && leaf.outerAttemptId
    ? normalizeSessionPath(`tasks/${run.taskId}/runs/${run.runId}/rounds/${leaf.roundId}/nodes/${leaf.outerNodeId}/${leaf.outerAttemptId}/dynamic/nodes/${leaf.nodeId}/${leaf.attemptId}`)
    : normalizeSessionPath(`tasks/${run.taskId}/runs/${run.runId}/rounds/${leaf.roundId}/nodes/${leaf.nodeId}/${leaf.attemptId}`);
  return cwd.endsWith(expected);
}

export function resolveConversationContentQueryState(
  identity: string | null,
  projection: { identity: string; state: AcpInitialSessionQueryState } | null,
  hydrated: boolean,
): AcpInitialSessionQueryState {
  if (!identity) return 'success';
  if (projection?.identity === identity) return projection.state;
  return hydrated ? 'success' : 'loading';
}

export function shouldShowConversationContentLoadingState(
  queryState: AcpInitialSessionQueryState,
  selectedLeaf: Pick<ConversationSessionLeafVm, 'current'> | null,
) {
  return queryState === 'loading' && !selectedLeaf?.current;
}

function normalizeSessionPath(path: string) {
  return path.replace(/\\/g, '/').replace(/\/+/g, '/').toLowerCase();
}

interface ConversationRunPageProps {
  run: ConversationRunVm;
  taskTitle: string;
  appConfig: AppConfigVm;
  agentRegistry: AgentRegistryVm | null;
  onRerun: () => void;
  onEditWorkflow: () => void;
  onSaveWorkflow?: (json: string, modelBindings: WorkflowModelBindings) => Promise<WorkflowVm>;
  onSelectSession: (leaf: ConversationSessionLeafVm, followActive?: boolean) => void;
  onLifecycleSnapshot?: (snapshot: AcpLifecycleSnapshot) => void;
  onAutoFollowChange?: (enabled: boolean) => void;
  followMode: ConversationSessionFollowMode;
  initialSessionTreeExpansion: ConversationSessionTreeExpansion;
  onSessionTreeExpansionChange: (expansion: ConversationSessionTreeExpansion) => void;
  onTitleChange?: (title: string) => void;
}

export function ConversationRunPage({
  run,
  taskTitle,
  appConfig,
  agentRegistry,
  onRerun,
  onEditWorkflow,
  onSaveWorkflow,
  onSelectSession,
  onLifecycleSnapshot,
  onAutoFollowChange,
  followMode,
  initialSessionTreeExpansion,
  onSessionTreeExpansionChange,
  onTitleChange,
}: ConversationRunPageProps) {
  const { t } = useTranslation();
  useThemeWallpaperSurface();
  const workspace = useRightWorkspace();
  const translatePauseReason = (reason?: string | null) => {
    if (!reason) return t('conversation.runtime.sessionPaused');
    switch (reason) {
      case 'process-interrupted': return t('conversation.runtime.pauseReasonProcessInterrupted');
      case 'runtime-abnormal': return t('conversation.runtime.pauseReasonRuntimeAbnormal');
      case 'waiting-for-user-input': return t('conversation.runtime.pauseReasonWaitingForUserInput');
      default: return t('conversation.runtime.pauseReasonFallback');
    }
  };
  const translateRuntimeError = (reason?: string | null) => {
    if (reason === 'error-blocked') return t('conversation.runtime.runtimeErrorBlocked');
    return t('conversation.runtime.runError');
  };
  const translateSelectedRuntimeError = (code?: string | null, reason?: string | null, details?: string | null) => {
    let message = translateRuntimeError(reason);
    if (code === 'error-blocked' || reason === 'error-blocked') message = t('conversation.runtime.runtimeErrorBlocked');
    else if (code === 'killed') message = t('conversation.runtime.runtimeSessionKilled');
    else if (code === 'failure' || code === 'invalid') message = t('conversation.runtime.runtimeSessionFailed');
    const normalizedDetails = details?.trim();
    return normalizedDetails ? `${message}：${normalizedDetails}` : message;
  };
  const localizedRuntimeErrorMessage = acpRuntimeErrorBannerCopy(t, run.runtimeError);
  const [sessionSwitcherOpen, setSessionSwitcherOpen] = useState(false);
  const sessionTreeExpansionRunKey = conversationRunCacheKey(run)
    ?? `uncached:${run.projectId}:${run.taskId}:${run.runId}`;
  const [sessionTreeExpansionState, setSessionTreeExpansionState] = useState<{
    runKey: string;
    expansion: ConversationSessionTreeExpansion;
  }>(() => ({
    runKey: sessionTreeExpansionRunKey,
    expansion: initialSessionTreeExpansion,
  }));
  const sessionTreeExpansion = sessionTreeExpansionState.runKey === sessionTreeExpansionRunKey
    ? sessionTreeExpansionState.expansion
    : initialSessionTreeExpansion;
  const [rerunConfirmOpen, setRerunConfirmOpen] = useState(false);
  const isAtBottomRef = useRef(true);
  const manualAutoFollowDisabledRef = useRef(followMode === 'manual');
  const pendingAutoFollowRestoreSessionKeyRef = useRef<string | null>(null);
  const scrollPausedAutoFollowSessionKeyRef = useRef<string | null>(null);
  const activeSessionKeys = useMemo(
    () => run.activeSessions.map((session) => activeSessionKey(session)),
    [run.activeSessions],
  );

  useEffect(() => {
    manualAutoFollowDisabledRef.current = followMode === 'manual';
  }, [followMode]);

  useEffect(() => {
    pendingAutoFollowRestoreSessionKeyRef.current = null;
    scrollPausedAutoFollowSessionKeyRef.current = null;
  }, [run.projectId, run.runId, run.taskId]);

  const handleSessionTreeExpansionChange = useCallback((branchKey: string, open: boolean) => {
    const nextExpansion = { ...sessionTreeExpansion, [branchKey]: open };
    setSessionTreeExpansionState({
      runKey: sessionTreeExpansionRunKey,
      expansion: nextExpansion,
    });
    onSessionTreeExpansionChange(nextExpansion);
  }, [onSessionTreeExpansionChange, sessionTreeExpansion, sessionTreeExpansionRunKey]);

  const workflowLocator = useMemo(() => ({
    projectId: run.projectId,
    taskId: run.taskId,
    taskUuid: run.taskUuid,
    runId: run.runId,
  }), [run.projectId, run.runId, run.taskId, run.taskUuid]);

  const openWorkflowEditor = useCallback((mode: 'edit' | 'repair') => {
    onEditWorkflow();
    if (!workspace.scopeKey) return;
    workspace.openResource({
      kind: 'workflow-edit',
      key: conversationRunWorkspaceResourceKey('workflow-edit', workflowLocator),
      scopeKey: workspace.scopeKey,
      title: mode === 'repair' ? t('workflow.repairWorkflowTitle') : t('conversation.runtime.editWorkflow'),
      attention: false,
      mode,
      locator: workflowLocator,
    });
  }, [onEditWorkflow, t, workflowLocator, workspace.openResource, workspace.scopeKey]);

  const handleEditWorkflow = useCallback(() => {
    openWorkflowEditor('edit');
  }, [openWorkflowEditor]);

  const handleRepairWorkflow = useCallback(() => {
    openWorkflowEditor('repair');
  }, [openWorkflowEditor]);

  const handleViewWorkflow = useCallback(() => {
    if (!workspace.scopeKey) return;
    workspace.openResource({
      kind: 'workflow-view',
      key: conversationRunWorkspaceResourceKey('workflow-view', workflowLocator),
      scopeKey: workspace.scopeKey,
      title: t('conversation.runtime.viewWorkflow'),
      attention: false,
      locator: workflowLocator,
    });
  }, [t, workflowLocator, workspace.openResource, workspace.scopeKey]);

  const handleWorkflowNodeOpenSession = useCallback((graphNode: GraphNodeVm) => {
    const leaf = conversationSessionLeafForGraphNode(run.sessionTree, graphNode);
    if (!leaf) return;
    pendingAutoFollowRestoreSessionKeyRef.current = null;
    scrollPausedAutoFollowSessionKeyRef.current = null;
    manualAutoFollowDisabledRef.current = true;
    onAutoFollowChange?.(false);
    onSelectSession(leaf);
  }, [run.sessionTree, onAutoFollowChange, onSelectSession]);

  const renderWorkspaceResource = useCallback((resource: RightWorkspaceResource) => {
    if (
      resource.kind !== 'workflow-view' &&
      resource.kind !== 'workflow-edit' &&
      resource.kind !== 'system-prompt' &&
      resource.kind !== 'hidden-prompt-section' &&
      resource.kind !== 'raw-frames'
    ) return null;
    return (
      <ConversationRunWorkspaceResourcePanel
        key={resource.key}
        resource={resource}
        run={run}
        agentRegistry={agentRegistry}
        onSaveWorkflow={onSaveWorkflow}
        onNodeOpenSession={handleWorkflowNodeOpenSession}
      />
    );
  }, [agentRegistry, handleWorkflowNodeOpenSession, onSaveWorkflow, run]);

  useEffect(() => {
    const unregister = [
      workspace.registerResourceRenderer('workflow-view', renderWorkspaceResource),
      workspace.registerResourceRenderer('workflow-edit', renderWorkspaceResource),
      workspace.registerResourceRenderer('system-prompt', renderWorkspaceResource),
      workspace.registerResourceRenderer('hidden-prompt-section', renderWorkspaceResource),
      workspace.registerResourceRenderer('raw-frames', renderWorkspaceResource),
    ];
    return () => unregister.forEach((dispose) => dispose());
  }, [renderWorkspaceResource, workspace.registerResourceRenderer]);
  useEffect(
    () => workspace.registerResourceCloseResolver('workflow-edit', (resource) => confirmCloseConversationRunWorkspaceResource(
      resource,
      () => window.confirm(t('workspace.discardWorkflowChanges')),
    )),
    [t, workspace.registerResourceCloseResolver],
  );

  const isRunning = run.runStatus === 'running';
  const isDirect = run.runMode === 'direct';
  const selectedLeaf = findSelectedLeaf(run);
  const selectedSessionKey = run.sessionTree.selectedSessionKey ?? (selectedLeaf ? leafKey(selectedLeaf) : null);
  const selectedRoundId = selectedLeaf?.roundId ?? null;
  const selectedNodeId = selectedLeaf?.nodeId ?? null;
  const selectedAttemptId = selectedLeaf?.attemptId ?? null;
  const selectedOuterNodeId = selectedLeaf?.outerNodeId ?? null;
  const selectedOuterAttemptId = selectedLeaf?.outerAttemptId ?? null;
  const selectedRuntimeCode = selectedLeaf?.runtimeDisplay?.code ?? null;
  const showLaunchingSession = isRunning && !selectedLeaf;
  const selectedContentIdentity = selectedLeaf
      ? createAcpEventWindowCacheKey({
        cacheNamespace: run.taskUuid ?? `${run.projectId}:${run.taskId}`,
        projectId: run.projectId,
        taskId: run.taskId,
        runId: run.runId,
        roundId: selectedLeaf.roundId,
        nodeId: selectedLeaf.nodeId,
        attemptId: selectedLeaf.attemptId,
        outerNodeId: selectedLeaf.outerNodeId,
        outerAttemptId: selectedLeaf.outerAttemptId,
      })
    : null;
  const [contentQueryProjection, setContentQueryProjection] = useState<{
    identity: string;
    state: AcpInitialSessionQueryState;
  } | null>(null);
  const selectedContentQueryState = resolveConversationContentQueryState(
    selectedContentIdentity,
    contentQueryProjection,
    selectedContentIdentity
      ? hasHydratedAcpSessionContent(selectedContentIdentity)
      : false,
  );
  const showPageLoadingState = shouldShowConversationContentLoadingState(
    selectedContentQueryState,
    selectedLeaf,
  );
  const handleInitialSessionQueryStateChange = useCallback((state: AcpInitialSessionQueryState) => {
    if (!selectedContentIdentity) return;
    setContentQueryProjection({ identity: selectedContentIdentity, state });
  }, [selectedContentIdentity]);

  const conversationDirectoryEntry = useMemo<ConversationDirectoryWorkspaceEntry | null>(() => {
    if (!workspace.scopeKey || !selectedRoundId || !selectedNodeId || !selectedAttemptId) return null;
    return {
      kind: 'conversation-directory',
      scopeKey: workspace.scopeKey,
      title: t('workspace.runDirectory'),
      description: selectedRuntimeCode,
      attention: false,
      locator: {
        projectId: run.projectId,
        taskId: run.taskId,
        runId: run.runId,
        roundId: selectedRoundId,
        nodeId: selectedNodeId,
        attemptId: selectedAttemptId,
        outerNodeId: selectedOuterNodeId,
        outerAttemptId: selectedOuterAttemptId,
      },
    };
  }, [run.projectId, run.runId, run.taskId, selectedAttemptId, selectedNodeId, selectedOuterAttemptId, selectedOuterNodeId, selectedRoundId, selectedRuntimeCode, t, workspace.scopeKey]);

  useEffect(() => {
    workspace.setConversationDirectoryEntry(conversationDirectoryEntry);
    return () => workspace.setConversationDirectoryEntry(null);
  }, [conversationDirectoryEntry, workspace.setConversationDirectoryEntry]);

  const isAutoFollowRestorableLeaf = useCallback((leaf: ConversationSessionLeafVm | null) => {
    if (!leaf) return false;
    return isRuntimeControlledConversationLifecycle(leaf.lifecycle)
      && (activeSessionKeys.includes(leafKey(leaf)) || isRestorableRuntimeLeaf(leaf));
  }, [activeSessionKeys]);

  const handleAtBottomChange = useCallback((atBottom: boolean) => {
    isAtBottomRef.current = atBottom;
    const selectedKey = run.sessionTree.selectedSessionKey ?? (selectedLeaf ? leafKey(selectedLeaf) : null);
    if (!atBottom) {
      if (!manualAutoFollowDisabledRef.current) {
        scrollPausedAutoFollowSessionKeyRef.current = selectedKey;
      }
      manualAutoFollowDisabledRef.current = true;
      onAutoFollowChange?.(false);
      return;
    }
    const restoreKey = pendingAutoFollowRestoreSessionKeyRef.current;
    const scrollPausedKey = scrollPausedAutoFollowSessionKeyRef.current;
    const selectedDynamicLeafTerminal = Boolean(
      selectedLeaf?.outerNodeId
      && selectedLeaf.outerAttemptId
      && isTerminalConversationSessionStatus(
        selectedLeaf.lifecycle?.runtime.status ?? selectedLeaf.status,
      )
    );
    if (!isRuntimeControlledConversationLifecycle(selectedLeaf?.lifecycle)) {
      if (selectedDynamicLeafTerminal && selectedKey && scrollPausedKey === selectedKey) {
        scrollPausedAutoFollowSessionKeyRef.current = null;
        manualAutoFollowDisabledRef.current = false;
        onAutoFollowChange?.(true);
        return;
      }
      if (selectedDynamicLeafTerminal && !manualAutoFollowDisabledRef.current) {
        onAutoFollowChange?.(true);
        return;
      }
      pendingAutoFollowRestoreSessionKeyRef.current = null;
      scrollPausedAutoFollowSessionKeyRef.current = null;
      manualAutoFollowDisabledRef.current = true;
      onAutoFollowChange?.(false);
      return;
    }
    const restorableSelected = isAutoFollowRestorableLeaf(selectedLeaf);
    if (selectedKey && restoreKey === selectedKey && restorableSelected) {
      pendingAutoFollowRestoreSessionKeyRef.current = null;
      scrollPausedAutoFollowSessionKeyRef.current = null;
      manualAutoFollowDisabledRef.current = false;
      onAutoFollowChange?.(true);
      return;
    }
    if (restorableSelected) {
      scrollPausedAutoFollowSessionKeyRef.current = null;
      manualAutoFollowDisabledRef.current = false;
      onAutoFollowChange?.(true);
      return;
    }
    if (!manualAutoFollowDisabledRef.current) {
      onAutoFollowChange?.(true);
    }
  }, [isAutoFollowRestorableLeaf, onAutoFollowChange, run.sessionTree.selectedSessionKey, selectedLeaf]);

  const handleSessionSelection = useCallback((leaf: ConversationSessionLeafVm, followActive = false) => {
    const key = leafKey(leaf);
    const canRestoreAutoFollow = followActive && isAutoFollowRestorableLeaf(leaf);
    if (canRestoreAutoFollow && isAtBottomRef.current) {
      pendingAutoFollowRestoreSessionKeyRef.current = null;
      scrollPausedAutoFollowSessionKeyRef.current = null;
      manualAutoFollowDisabledRef.current = false;
      onAutoFollowChange?.(true);
      onSelectSession(leaf, true);
      return;
    }
    if (canRestoreAutoFollow) {
      pendingAutoFollowRestoreSessionKeyRef.current = key;
      scrollPausedAutoFollowSessionKeyRef.current = null;
      manualAutoFollowDisabledRef.current = true;
      onAutoFollowChange?.(false);
      onSelectSession(leaf, false);
      return;
    }
    pendingAutoFollowRestoreSessionKeyRef.current = null;
    scrollPausedAutoFollowSessionKeyRef.current = null;
    manualAutoFollowDisabledRef.current = true;
    onAutoFollowChange?.(false);
    onSelectSession(leaf, false);
  }, [isAutoFollowRestorableLeaf, onAutoFollowChange, onSelectSession]);

  const handleRerun = () => {
    if (isRunning) {
      setRerunConfirmOpen(true);
    } else {
      onRerun();
    }
  };

  const selectedSessionMatchesLeaf = sessionBelongsToLeaf(run.selectedSession, run, selectedLeaf);
  const selectedSession = selectedSessionMatchesLeaf ? run.selectedSession : null;
  const selectedProviderCatalog = useMemo(
    () => acpProviderConfigCatalog(agentRegistry, selectedSession?.provider),
    [agentRegistry, selectedSession?.provider],
  );
  const selectedSessionDisplay = selectedLeaf?.runtimeDisplay;
  const runtimeControlErrorBase = localizedRuntimeErrorMessage ?? run.runtimeErrorMessage;
  const selectedSessionRuntimeControlError = runtimeControlErrorBase && !(
    selectedLeaf?.lifecycle?.composer.mode === 'runtime-error' || selectedSessionDisplay?.code === 'error-blocked'
  )
    ? runtimeControlErrorBase
    : null;
  const selectedSessionErrorDetails = run.runtimeErrorMessage ?? selectedSession?.diagnostics.lastError ?? null;
  const selectedSessionPauseReason = selectedSessionDisplay?.reasonCode ?? run.pauseReason;
  const selectedSessionErrorBlocked = selectedSessionDisplay?.code === 'error-blocked';
  const selectedSessionRuntimeError = selectedLeaf?.lifecycle?.composer.mode === 'runtime-error' || selectedSessionErrorBlocked;
  const selectedRuntimeErrorMessage = selectedSessionRuntimeControlError
    ?? (selectedSessionRuntimeError
      ? translateSelectedRuntimeError(selectedSessionDisplay?.code, run.pauseReason, selectedSessionErrorDetails)
      : null);
  const canViewWorkflow = !isDirect && canViewConversationRuntimeWorkflow(run, selectedLeaf);
  const supersededBy = selectedLeaf?.lifecycle?.composer.supersededBy;
  const supersedingLeaf = supersededBy
    ? findConversationLeafByKey(run.sessionTree, activeSessionKey(supersededBy))
    : null;
  const supersedingPage = supersededBy ? conversationPageForSession(run, supersededBy) : null;
  const supersedingHref = supersedingPage
    ? pathFromRoute('task-orchestration', taskListPage, supersedingPage)
    : null;
  const runtimeComposerContext: AcpRuntimeComposerContext | undefined = selectedLeaf
    ? {
        isOrchestrated: run.runMode !== 'direct',
        lifecycle: selectedLeaf.lifecycle,
        promptQueueEnabled: isDirect,
        runtimeStatus: selectedLeaf.lifecycle?.runtime.status ?? selectedLeaf.status,
        workflowValid: isDirect || run.workflowValid,
        workflowError: isDirect ? undefined : t('conversation.runtime.workflowInvalid'),
        pauseMessage: isDirect ? undefined : translatePauseReason(selectedSessionPauseReason),
        runtimeError: selectedRuntimeErrorMessage,
        onRepair: handleRepairWorkflow,
        supersededSessionNavigation: supersedingHref
          ? {
              href: supersedingHref,
              onNavigate: () => {
                if (supersedingLeaf) {
                  handleSessionSelection(supersedingLeaf);
                } else {
                  window.location.assign(supersedingHref);
                }
              },
            }
          : undefined,
      }
    : undefined;

  return (
    <TooltipProvider>
      <div data-theme-wallpaper-slot="conversation" className="relative h-full min-h-0 bg-background">
      <div className={`flex h-full min-h-0 flex-col bg-transparent ${showPageLoadingState ? 'invisible' : ''}`}>
        <div className="relative shrink-0">
          {!isDirect || !selectedLeaf ? <ConversationRunHeader
            run={run}
            taskTitle={taskTitle}
            selectedSessionLeaf={selectedLeaf}
            canViewWorkflow={canViewWorkflow}
            canEditWorkflow={run.runMode === 'workflow'}
            onRerun={handleRerun}
            onEditWorkflow={handleEditWorkflow}
            onViewWorkflow={handleViewWorkflow}
            onSessionSwitcherOpenChange={setSessionSwitcherOpen}
            sessionSwitcherOpen={sessionSwitcherOpen}
            sessionSwitcher={!isDirect ? (
              <ConversationSessionSwitcher
                tree={run.sessionTree}
                selectedKey={run.sessionTree.selectedSessionKey}
                expansion={sessionTreeExpansion}
                onExpansionChange={handleSessionTreeExpansionChange}
                onSelectSession={(leaf) => {
                  handleSessionSelection(leaf, isAutoFollowRestorableLeaf(leaf));
                  setSessionSwitcherOpen(false);
                }}
              />
            ) : null}
            onTitleChange={onTitleChange}
          /> : null}
        </div>

      {/* Active sessions indicator */}
      {!isDirect && run.activeSessions.length > 1 ? (
        <div
          data-conversation-active-sessions="true"
          className="shrink-0 border-b border-border/60 bg-content-header px-5 py-1"
        >
          <div className="flex flex-wrap gap-1.5">
            {run.activeSessions.map((session) => (
              <button
                key={`${session.roundId}/${session.nodeId}/${session.attemptId}`}
                type="button"
                className="rounded-full border border-border/60 bg-card px-3 py-0.5 text-xs hover:bg-sidebar-accent"
                onClick={() => handleSessionSelection({
                  roundId: session.roundId,
                  nodeId: session.nodeId,
                  attemptId: session.attemptId,
                  outerNodeId: session.outerNodeId,
                  outerAttemptId: session.outerAttemptId,
                  pathLabel: session.pathLabel,
                  status: session.status,
                  runtimeDisplay: session.runtimeDisplay,
                  lifecycle: session.lifecycle,
                  current: true,
                  manualCheckPending: session.manualCheckPending,
                  sessionId: session.sessionId,
                  sessionEstablished: session.sessionEstablished,
                  artifactCount: 0,
                  attachmentCount: 0,
                }, true)}
              >
                <span className="font-medium">{session.pathLabel}</span>
                {session.runtimeDisplay.tone === 'running' ? (
                  <span className="ml-1.5 inline-block size-1.5 rounded-full bg-primary animate-pulse" />
                ) : null}
              </button>
            ))}
          </div>
        </div>
      ) : null}

      {/* Main chat area */}
      <div className="min-h-0 flex-1">
        {selectedLeaf ? (
          <ACPChatDialog
            key={`${run.taskUuid ?? run.taskId}:${selectedSessionKey ?? 'empty'}`}
            session={selectedSession}
            providerCatalog={selectedProviderCatalog}
            sessionEstablished={selectedLeaf.sessionEstablished}
            sessionReferenceId={selectedLeaf.sessionId}
            projectId={run.projectId}
            taskId={run.taskId}
            taskUuid={run.taskUuid}
            runId={run.runId}
            roundId={selectedLeaf.roundId}
            nodeId={selectedLeaf.nodeId}
            attemptId={selectedLeaf.attemptId}
            outerNodeId={selectedLeaf.outerNodeId}
            outerAttemptId={selectedLeaf.outerAttemptId}
            eventPageSize={appConfig.acpChatEventPageSize}
            inlineContentMaxBytes={appConfig.conversationInlineContentMaxBytes}
            turnFileCardPreviewLimit={appConfig.turnFiles.cardPreviewLimit}
            onLifecycleSnapshot={onLifecycleSnapshot}
            onAtBottomChange={handleAtBottomChange}
            onInitialSessionQueryStateChange={handleInitialSessionQueryStateChange}
            allowEventOnlySessionShell={false}
            wallpaperSurface
            worktreePath={selectedLeaf.worktreePath}
            showBranchControl
            managedWorktreeBranch={selectedLeaf.worktreeBranch}
            runtimeComposerContext={runtimeComposerContext}
            manualCheckPending={selectedLeaf.manualCheckPending && selectedLeaf.current}
            showSystemPromptAction={!isDirect}
            directSessionHeader={isDirect ? {
              title: taskTitle,
              onTitleChange,
            } : undefined}
            usageCompact
            cacheNamespace={run.taskUuid ?? `${run.projectId}:${run.taskId}`}
          />
        ) : (
          <ConversationEmptySessionState
            label={showLaunchingSession ? t('acp.launchingClaude') : t('conversation.runtime.noActiveSession')}
            active={showLaunchingSession}
          />
        )}
      </div>

      {/* Rerun confirmation dialog */}
      <AlertDialog open={rerunConfirmOpen} onOpenChange={setRerunConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('conversation.runtime.rerunConfirmTitle')}</AlertDialogTitle>
            <p className="text-sm text-muted-foreground">{t('conversation.runtime.rerunConfirmDescription')}</p>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction onClick={onRerun}>
              {t('conversation.runtime.rerunConfirmAction')}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

    </div>
      {showPageLoadingState ? (
        <BrandLoadingState
          label={t('conversation.runtime.loadingSession')}
          className="absolute inset-0 bg-background/88 backdrop-blur-sm"
        />
      ) : null}
    </div>
    </TooltipProvider>
  );
}

function ConversationEmptySessionState({ label, active }: { label: string; active: boolean }) {
  if (active) {
    return <BrandLoadingState label={label} className="bg-background/88 backdrop-blur-sm" />;
  }
  return (
    <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
      <span>{label}</span>
    </div>
  );
}

function leafKey(leaf: ConversationSessionLeafVm): string {
  if (leaf.outerNodeId && leaf.outerAttemptId) {
    return `${leaf.roundId}/${leaf.outerNodeId}/${leaf.outerAttemptId}/${leaf.nodeId}/${leaf.attemptId}`;
  }
  return `${leaf.roundId}/${leaf.nodeId}/${leaf.attemptId}`;
}

function findSelectedLeaf(run: ConversationRunVm): ConversationSessionLeafVm | null {
  return findSelectedLeafFromTree(run.sessionTree)
    ?? activeSessionToLeaf(run.activeSessions[0]);
}

function findSelectedLeafFromTree(tree: ConversationRunVm['sessionTree']): ConversationSessionLeafVm | null {
  const key = tree.selectedSessionKey;
  if (!key) return defaultSessionLeafFromTree(tree);
  for (const round of tree.rounds) {
    for (const node of round.nodes) {
      for (const attempt of node.attempts) {
        if (leafKey(attempt) === key) return attempt;
      }
      if (node.outerNodes) {
        for (const outer of node.outerNodes) {
          for (const attempt of outer.attempts) {
            if (leafKey(attempt) === key) return attempt;
          }
        }
      }
    }
  }
  return defaultSessionLeafFromTree(tree);
}

function defaultSessionLeafFromTree(tree: ConversationRunVm['sessionTree']): ConversationSessionLeafVm | null {
  let active: ConversationSessionLeafVm | null = null;
  let latest: ConversationSessionLeafVm | null = null;
  for (const round of tree.rounds) {
    for (const node of round.nodes) {
      for (const attempt of node.attempts) {
        if (attempt.current) return attempt;
        if (!active && isActiveSessionLeaf(attempt)) {
          active = attempt;
        }
        if (!latest || leafSortKey(attempt) > leafSortKey(latest)) {
          latest = attempt;
        }
      }
      for (const outer of node.outerNodes ?? []) {
        for (const attempt of outer.attempts) {
          if (attempt.current) return attempt;
          if (!active && isActiveSessionLeaf(attempt)) {
            active = attempt;
          }
          if (!latest || leafSortKey(attempt) > leafSortKey(latest)) {
            latest = attempt;
          }
        }
      }
    }
  }
  return active ?? latest;
}

function leafSortKey(leaf: ConversationSessionLeafVm): string {
  return [
    leaf.startedAt ?? leaf.finishedAt ?? '',
    leaf.roundId,
    leaf.outerNodeId ?? '',
    leaf.nodeId,
    leaf.attemptId,
  ].join('\u0000');
}

function activeSessionToLeaf(
  session: ConversationRunVm['activeSessions'][number] | undefined,
): ConversationSessionLeafVm | null {
  if (!session) return null;
  return {
    roundId: session.roundId,
    nodeId: session.nodeId,
    attemptId: session.attemptId,
    outerNodeId: session.outerNodeId,
    outerAttemptId: session.outerAttemptId,
    pathLabel: session.pathLabel,
    status: session.status,
    outcome: null,
    runtimeDisplay: session.runtimeDisplay,
    lifecycle: session.lifecycle,
    current: true,
    manualCheckPending: session.manualCheckPending,
    startedAt: session.startedAt,
    finishedAt: null,
    sessionId: session.sessionId,
    artifactCount: 0,
    attachmentCount: 0,
  };
}

function isRestorableRuntimeLeaf(leaf: ConversationSessionLeafVm) {
  return Boolean(
    leaf.lifecycle?.runtime.active
    || leaf.lifecycle?.acp.liveTurnActivity !== 'idle'
    || leaf.lifecycle?.acp.stopping,
  ) || isActiveSessionStatus(leaf.status) || (leaf.current && !isTerminalSessionStatus(leaf.status));
}

function isActiveSessionLeaf(leaf: ConversationSessionLeafVm) {
  return Boolean(leaf.manualCheckPending) || isRestorableRuntimeLeaf(leaf);
}

function normalizeSessionStatus(status?: string | null) {
  return status?.trim().toLowerCase().replace(/_/g, '-') ?? '';
}

function isActiveSessionStatus(status?: string | null) {
  return ['pending', 'ready', 'running', 'in-progress', 'active', 'sending', 'cancelling', 'cancel-requested'].includes(normalizeSessionStatus(status));
}

function isTerminalSessionStatus(status?: string | null) {
  return ['completed', 'complete', 'success', 'failed', 'failure', 'error', 'killed', 'cancelled', 'canceled'].includes(normalizeSessionStatus(status));
}
