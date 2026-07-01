import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Button } from '@/components/ui/button';
import { ACPChatDialog, type ACPChatDialogHandle, type AcpLifecycleSnapshot, type AcpRuntimeComposerContext } from '@/components/acp/ACPChatDialog';
import { ConversationRunHeader } from '@/components/conversation/ConversationRunHeader';
import { ConversationSessionSwitcher } from '@/components/conversation/ConversationSessionSwitcher';
import { StatusBadge } from '@/components/StatusBadge';
import { WorkflowEditor, parseWorkflowJson } from '@/components/WorkflowEditor';
import { GraphView } from '@/components/GraphView';
import { conversationAssetsForLeaf } from '@/lib/conversation-session-assets';
import { canViewConversationRuntimeWorkflow, conversationSessionLeafForGraphNode } from '@/lib/conversation-runtime-workflow';
import type { AcpSessionVm, AgentRegistryVm, AppConfigVm, ConversationRunVm, ConversationSessionLeafVm, GraphNodeVm, GraphVm, ProfileVm } from '../types';
import { getAgentRegistry, getProfiles, openInFileManager } from '@/api';

type WorkflowSheetMode = 'edit' | 'repair' | 'view';

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

function sessionBelongsToLeaf(session: AcpSessionVm | null | undefined, run: ConversationRunVm, leaf: ConversationSessionLeafVm | null) {
  if (!session || !leaf || !session.cwd) return true;
  const cwd = normalizeSessionPath(session.cwd);
  const expected = leaf.outerNodeId && leaf.outerAttemptId
    ? normalizeSessionPath(`tasks/${run.taskId}/runs/${run.runId}/rounds/${leaf.roundId}/nodes/${leaf.outerNodeId}/${leaf.outerAttemptId}/dynamic/nodes/${leaf.nodeId}/${leaf.attemptId}`)
    : normalizeSessionPath(`tasks/${run.taskId}/runs/${run.runId}/rounds/${leaf.roundId}/nodes/${leaf.nodeId}/${leaf.attemptId}`);
  return cwd.endsWith(expected);
}

function normalizeSessionPath(path: string) {
  return path.replace(/\\/g, '/').replace(/\/+/g, '/').toLowerCase();
}

interface ConversationRunPageProps {
  run: ConversationRunVm;
  appConfig: AppConfigVm;
  agentRegistry: AgentRegistryVm | null;
  onRerun: () => void;
  onEditWorkflow: () => void;
  onSaveWorkflow?: (json: string) => Promise<void>;
  onSelectSession: (leaf: ConversationSessionLeafVm, followActive?: boolean) => void;
  onLifecycleSnapshot?: (snapshot: AcpLifecycleSnapshot) => void;
  onAutoFollowChange?: (enabled: boolean) => void;
  onTitleChange?: (title: string) => void;
}

export function ConversationRunPage({
  run,
  appConfig,
  agentRegistry,
  onRerun,
  onEditWorkflow,
  onSaveWorkflow,
  onSelectSession,
  onLifecycleSnapshot,
  onAutoFollowChange,
  onTitleChange,
}: ConversationRunPageProps) {
  const { t } = useTranslation();
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
  const [sessionSwitcherOpen, setSessionSwitcherOpen] = useState(false);
  const [rerunConfirmOpen, setRerunConfirmOpen] = useState(false);
  const [workflowSheet, setWorkflowSheet] = useState<{ open: boolean; mode: WorkflowSheetMode }>({ open: false, mode: 'view' });
  const [workflowValidationRequestId, setWorkflowValidationRequestId] = useState(0);
  const [workflowAgentRegistry, setWorkflowAgentRegistry] = useState<AgentRegistryVm | null>(null);
  const [workflowProfiles, setWorkflowProfiles] = useState<ProfileVm[] | null>(null);
  const effectiveAgentRegistry = workflowAgentRegistry ?? agentRegistry;
  const effectiveProfiles = workflowProfiles ?? [];
  const isAtBottomRef = useRef(true);
  const manualAutoFollowDisabledRef = useRef(false);
  const pendingAutoFollowRestoreSessionKeyRef = useRef<string | null>(null);
  const onAutoFollowChangeRef = useRef(onAutoFollowChange);
  const headerAreaRef = useRef<HTMLDivElement>(null);
  const chatDialogRef = useRef<ACPChatDialogHandle>(null);
  const activeSessionKeys = useMemo(
    () => run.activeSessions.map((session) => activeSessionKey(session)),
    [run.activeSessions],
  );

  useEffect(() => {
    onAutoFollowChangeRef.current = onAutoFollowChange;
  }, [onAutoFollowChange]);

  useEffect(() => {
    manualAutoFollowDisabledRef.current = false;
    pendingAutoFollowRestoreSessionKeyRef.current = null;
    onAutoFollowChangeRef.current?.(true);
  }, [run.runId]);

  // Close session switcher on outside click
  useEffect(() => {
    if (!sessionSwitcherOpen) return;
    const handler = (e: MouseEvent) => {
      if (headerAreaRef.current && !headerAreaRef.current.contains(e.target as Node)) {
        setSessionSwitcherOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [sessionSwitcherOpen]);

  // Lazy-load workflow editor dependencies when opening the workflow sheet.
  const openWorkflowEditor = useCallback((mode: Exclude<WorkflowSheetMode, 'view'>) => {
    onEditWorkflow();
    const open = () => {
      setWorkflowSheet({ open: true, mode });
      if (mode === 'repair') setWorkflowValidationRequestId((value) => value + 1);
    };
    const registryPromise = effectiveAgentRegistry
      ? Promise.resolve(effectiveAgentRegistry)
      : getAgentRegistry().catch(() => ({ agents: [], supportedTypes: [] }));
    const profilesPromise = workflowProfiles
      ? Promise.resolve(workflowProfiles)
      : getProfiles().then((result) => result.profiles).catch(() => []);
    Promise.all([registryPromise, profilesPromise]).then(([registry, profiles]) => {
      setWorkflowAgentRegistry(registry);
      setWorkflowProfiles(profiles);
      open();
    });
  }, [effectiveAgentRegistry, onEditWorkflow, workflowProfiles]);

  const handleEditWorkflow = useCallback(() => {
    openWorkflowEditor('edit');
  }, [openWorkflowEditor]);

  const handleRepairWorkflow = useCallback(() => {
    openWorkflowEditor('repair');
  }, [openWorkflowEditor]);

  const handleViewWorkflow = useCallback(() => {
    setWorkflowSheet({ open: true, mode: 'view' });
  }, []);

  const handleWorkflowValidationIssues = useCallback(() => {
    setWorkflowValidationRequestId((value) => value + 1);
  }, []);

  const handleWorkflowSheetClose = useCallback(() => {
    setWorkflowSheet((current) => ({ ...current, open: false }));
  }, []);

  const handleWorkflowNodeOpenSession = useCallback((graphNode: GraphNodeVm) => {
    const leaf = conversationSessionLeafForGraphNode(run.sessionTree, graphNode);
    if (!leaf) return;
    pendingAutoFollowRestoreSessionKeyRef.current = null;
    manualAutoFollowDisabledRef.current = true;
    onAutoFollowChange?.(false);
    onSelectSession(leaf);
    setWorkflowSheet({ open: false, mode: workflowSheet.mode });
  }, [run.sessionTree, onAutoFollowChange, onSelectSession, workflowSheet.mode]);

  const isRunning = run.runStatus === 'running';
  const selectedLeaf = findSelectedLeaf(run);
  const selectedSessionKey = run.sessionTree.selectedSessionKey ?? (selectedLeaf ? leafKey(selectedLeaf) : null);
  const showLaunchingSession = isRunning && !selectedLeaf;

  const handleOpenInFileManager = useCallback(() => {
    if (!selectedLeaf) return;
    openInFileManager(
      run.projectId,
      run.taskId,
      run.runId,
      selectedLeaf.roundId,
      selectedLeaf.nodeId,
      selectedLeaf.attemptId,
      selectedLeaf.outerNodeId,
      selectedLeaf.outerAttemptId,
    );
  }, [run.projectId, run.taskId, run.runId, selectedLeaf]);

  const isAutoFollowRestorableLeaf = useCallback((leaf: ConversationSessionLeafVm | null) => {
    if (!leaf) return false;
    return activeSessionKeys.includes(leafKey(leaf)) || isRestorableRuntimeLeaf(leaf);
  }, [activeSessionKeys]);

  const handleAtBottomChange = useCallback((atBottom: boolean) => {
    isAtBottomRef.current = atBottom;
    if (!atBottom) {
      manualAutoFollowDisabledRef.current = true;
      onAutoFollowChange?.(false);
      return;
    }
    const selectedKey = run.sessionTree.selectedSessionKey ?? (selectedLeaf ? leafKey(selectedLeaf) : null);
    const restoreKey = pendingAutoFollowRestoreSessionKeyRef.current;
    const restorableSelected = isAutoFollowRestorableLeaf(selectedLeaf);
    if (selectedKey && restoreKey === selectedKey && restorableSelected) {
      pendingAutoFollowRestoreSessionKeyRef.current = null;
      manualAutoFollowDisabledRef.current = false;
      onAutoFollowChange?.(true);
      return;
    }
    if (restorableSelected) {
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
      manualAutoFollowDisabledRef.current = false;
      onAutoFollowChange?.(true);
      onSelectSession(leaf, true);
      return;
    }
    if (canRestoreAutoFollow) {
      pendingAutoFollowRestoreSessionKeyRef.current = key;
      manualAutoFollowDisabledRef.current = true;
      onAutoFollowChange?.(false);
      onSelectSession(leaf, false);
      return;
    }
    pendingAutoFollowRestoreSessionKeyRef.current = null;
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
  const selectedArtifacts = conversationAssetsForLeaf(run.artifacts, selectedLeaf);
  const selectedAttachments = conversationAssetsForLeaf(run.attachments, selectedLeaf);
  const selectedSessionDisplay = selectedLeaf?.runtimeDisplay;
  const selectedSessionErrorDetails = run.runtimeErrorMessage ?? selectedSession?.diagnostics.lastError ?? null;
  const selectedSessionPauseReason = selectedSessionDisplay?.reasonCode ?? run.pauseReason;
  const selectedSessionErrorBlocked = selectedSessionDisplay?.code === 'error-blocked';
  const selectedRuntimeErrorMessage = selectedSessionDisplay?.blockingError || selectedSessionErrorBlocked
    ? translateSelectedRuntimeError(selectedSessionDisplay?.code, run.pauseReason, selectedSessionErrorDetails)
    : null;
  const canViewWorkflow = canViewConversationRuntimeWorkflow(run, selectedLeaf);
  const runtimeComposerContext: AcpRuntimeComposerContext | undefined = selectedLeaf
    ? {
        lifecycle: selectedLeaf.lifecycle,
        runtimeStatus: selectedLeaf.lifecycle?.runtime.status ?? selectedLeaf.status,
        workflowValid: run.workflowValid,
        workflowError: t('conversation.runtime.workflowInvalid'),
        pauseMessage: translatePauseReason(selectedSessionPauseReason),
        runtimeError: selectedRuntimeErrorMessage,
        onRepair: handleRepairWorkflow,
      }
    : undefined;

  return (
    <TooltipProvider>
      <div className="flex h-full min-h-0 flex-col bg-background">
        <div ref={headerAreaRef} className="shrink-0 relative">
          <ConversationRunHeader
            run={run}
            selectedSessionLeaf={selectedLeaf}
            canViewWorkflow={canViewWorkflow}
            canEditWorkflow={run.runMode === 'workflow'}
            onRerun={handleRerun}
            onEditWorkflow={handleEditWorkflow}
            onViewWorkflow={handleViewWorkflow}
            onOpenInFileManager={handleOpenInFileManager}
            onToggleSessionSwitcher={() => setSessionSwitcherOpen((prev) => !prev)}
            sessionSwitcherOpen={sessionSwitcherOpen}
            onTitleChange={onTitleChange}
          />

          {/* Session switcher dropdown */}
          {sessionSwitcherOpen ? (
            <div className="absolute right-5 top-12 z-50">
              <ConversationSessionSwitcher
                tree={run.sessionTree}
                selectedKey={run.sessionTree.selectedSessionKey}
                onSelectSession={(leaf) => {
                  handleSessionSelection(leaf, isAutoFollowRestorableLeaf(leaf));
                  setSessionSwitcherOpen(false);
                }}
              />
            </div>
          ) : null}
        </div>

      {/* Active sessions indicator */}
      {run.activeSessions.length > 1 ? (
        <div className="shrink-0 border-b bg-muted/5 px-5 py-2">
          <div className="flex flex-wrap gap-2">
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
            ref={chatDialogRef}
            key={`${run.taskUuid ?? run.taskId}:${selectedSessionKey ?? 'empty'}`}
            session={selectedSession}
            projectId={run.projectId}
            taskId={run.taskId}
            runId={run.runId}
            roundId={selectedLeaf.roundId}
            nodeId={selectedLeaf.nodeId}
            attemptId={selectedLeaf.attemptId}
            outerNodeId={selectedLeaf.outerNodeId}
            outerAttemptId={selectedLeaf.outerAttemptId}
            eventPageSize={appConfig.acpChatEventPageSize}
            onLifecycleSnapshot={onLifecycleSnapshot}
            onAtBottomChange={handleAtBottomChange}
            allowEventOnlySessionShell={false}
            runtimeComposerContext={runtimeComposerContext}
            manualCheckPending={selectedLeaf.manualCheckPending && selectedLeaf.current}
            liveUpdatesPaused={workflowSheet.open}
            artifacts={selectedArtifacts}
            attachments={selectedAttachments}
            allArtifacts={run.artifacts}
            allAttachments={run.attachments}
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

      {/* Workflow sheet (edit / view) */}
      <WorkflowSheet
        open={workflowSheet.open}
        mode={workflowSheet.mode}
        workflowJson={run.workflowJson}
        workflowGraph={run.workflowGraph}
        agentRegistry={effectiveAgentRegistry}
        profiles={effectiveProfiles}
        workflowValid={run.workflowValid}
        workflowErrorMessage={!run.workflowValid ? t('conversation.runtime.workflowInvalid') : selectedRuntimeErrorMessage}
        validationRequestId={workflowValidationRequestId}
        onShowValidationIssues={handleWorkflowValidationIssues}
        onSave={onSaveWorkflow}
        onClose={handleWorkflowSheetClose}
        onNodeOpenSession={handleWorkflowNodeOpenSession}
        t={t}
      />
    </div>
    </TooltipProvider>
  );
}

function ConversationEmptySessionState({ label, active }: { label: string; active: boolean }) {
  return (
    <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
      <div className="flex items-center gap-2">
        {active ? (
          <span
            aria-hidden="true"
            className="size-3.5 shrink-0 animate-spin rounded-full border-2 border-primary/25 border-t-primary [animation-duration:900ms]"
          />
        ) : null}
        <span>{label}</span>
      </div>
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
    || leaf.lifecycle?.acp.active
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

// ── Workflow sheet (edit / view) ──

const WorkflowSheet = memo(function WorkflowSheet({
  open,
  mode,
  workflowJson,
  workflowGraph,
  agentRegistry,
  profiles,
  workflowValid,
  workflowErrorMessage,
  validationRequestId,
  onShowValidationIssues,
  onSave,
  onClose,
  onNodeOpenSession,
  t,
}: {
  open: boolean;
  mode: WorkflowSheetMode;
  workflowJson?: string | null;
  workflowGraph: GraphVm;
  agentRegistry: AgentRegistryVm | null;
  profiles: ProfileVm[];
  workflowValid: boolean;
  workflowErrorMessage?: string | null;
  validationRequestId: number;
  onShowValidationIssues: () => void;
  onSave?: (json: string) => Promise<void>;
  onClose: () => void;
  onNodeOpenSession?: (node: GraphNodeVm) => void;
  t: (key: string) => string;
}) {
  const workflow = useMemo(
    () => (open ? parseWorkflowJson(workflowJson) : null),
    [open, workflowJson],
  );

  if (!open) return null;

  if (mode === 'edit' || mode === 'repair') {
    const repairMode = mode === 'repair';
    return (
      <Sheet open={open} onOpenChange={(o) => { if (!o) onClose(); }}>
        <SheetContent
          className="gap-0 overflow-hidden border-border bg-card p-0 sm:max-w-5xl"
          resizeStorageKey={`conversation/workflow-${mode}`}
          closeLabel={t('common.close')}
        >
          <SheetHeader className="shrink-0 gap-3 border-b px-5 py-4 text-left">
            <div className="flex min-w-0 flex-wrap items-center gap-3">
              <SheetTitle>{repairMode ? t('workflow.repairWorkflowTitle') : t('conversation.runtime.editWorkflow')}</SheetTitle>
              {repairMode ? <StatusBadge value={workflowValid ? 'valid' : 'invalid'} label={workflowValid ? t('status.valid') : t('status.invalid')} /> : null}
            </div>
            {repairMode && workflowErrorMessage ? (
              <Button
                type="button"
                variant="link"
                size="sm"
                className="h-auto justify-start px-0 text-sm text-primary underline-offset-4 hover:underline"
                onClick={onShowValidationIssues}
              >
                {t('workflow.viewErrorReasons')}
              </Button>
            ) : null}
            {repairMode && workflowErrorMessage ? (
              <p className="text-sm text-muted-foreground">{workflowErrorMessage}</p>
            ) : null}
          </SheetHeader>
          <div className="min-h-0 flex-1 overflow-auto">
            {workflow ? (
              <WorkflowEditor
                value={workflow}
                agentRegistry={agentRegistry}
                profiles={profiles}
                validationRequestId={repairMode ? validationRequestId : 0}
                onSave={async (dsl) => {
                  if (onSave) await onSave(JSON.stringify(dsl));
                  onClose();
                }}
              />
            ) : (
              <div className="flex items-center justify-center p-12 text-sm text-muted-foreground">{t('common.empty')}</div>
            )}
          </div>
        </SheetContent>
      </Sheet>
    );
  }

  // view mode — reuse GraphView from old UI
  const hasGraph = workflowGraph.nodes.length > 0;
  return (
    <Sheet open={open} onOpenChange={(o) => { if (!o) onClose(); }}>
      <SheetContent
        className="gap-0 overflow-hidden border-border bg-card p-0 sm:max-w-6xl"
        resizeStorageKey="conversation/workflow-view"
        closeLabel={t('common.close')}
      >
        <SheetHeader className="shrink-0 border-b px-5 py-4 text-left">
          <SheetTitle>{t('conversation.runtime.viewWorkflow')}</SheetTitle>
        </SheetHeader>
        <div className="min-h-0 flex-1 p-3">
          {hasGraph ? (
            <GraphView graph={workflowGraph} variant="actual" onNodeOpenDetail={onNodeOpenSession} onNodeOpenSession={onNodeOpenSession} />
          ) : (
            <div className="flex items-center justify-center p-12 text-sm text-muted-foreground">{t('common.empty')}</div>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
});
