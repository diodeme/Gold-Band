import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { Button } from '@/components/ui/button';
import { ACPChatDialog, type ACPChatDialogHandle, type AcpExternalComposerState } from '@/components/acp/ACPChatDialog';
import { ConversationRunHeader } from '@/components/conversation/ConversationRunHeader';
import { ConversationSessionSwitcher } from '@/components/conversation/ConversationSessionSwitcher';
import { ConversationAssetsBar } from '@/components/conversation/ConversationAssetsBar';
import { StatusBadge } from '@/components/StatusBadge';
import { WorkflowEditor, parseWorkflowJson } from '@/components/WorkflowEditor';
import { GraphView } from '@/components/GraphView';
import type { AcpSessionVm, AgentRegistryVm, AppConfigVm, ConversationRunVm, ConversationSessionLeafVm, GraphNodeVm, GraphVm, ProfileVm } from '../types';
import { getAgentRegistry, getProfiles, openInFileManager } from '@/api';

type WorkflowSheetMode = 'edit' | 'repair' | 'view';

interface ConversationRunPageProps {
  run: ConversationRunVm;
  appConfig: AppConfigVm;
  agentRegistry: AgentRegistryVm | null;
  onRerun: () => void;
  onEditWorkflow: () => void;
  onSaveWorkflow?: (json: string) => Promise<void>;
  onSelectSession: (leaf: ConversationSessionLeafVm) => void;
  onSessionStopped: () => void;
  onContinueRun: () => void;
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
  onSessionStopped,
  onContinueRun,
  onTitleChange,
}: ConversationRunPageProps) {
  const { t } = useTranslation();
  const translatePauseReason = (reason?: string | null) => {
    if (!reason) return t('conversation.runtime.sessionPaused');
    switch (reason) {
      case 'process-interrupted': return t('conversation.runtime.pauseReasonProcessInterrupted');
      case 'waiting-for-user-input': return t('conversation.runtime.pauseReasonWaitingForUserInput');
      default: return t('conversation.runtime.pauseReasonFallback');
    }
  };
  const translateRuntimeError = (reason?: string | null) => {
    if (reason === 'error-blocked') return t('conversation.runtime.runtimeErrorBlocked');
    return t('conversation.runtime.runError');
  };
  const translateSelectedRuntimeError = (code?: string | null, reason?: string | null) => {
    if (code === 'error-blocked' || reason === 'error-blocked') return t('conversation.runtime.runtimeErrorBlocked');
    if (code === 'killed') return t('conversation.runtime.runtimeSessionKilled');
    if (code === 'failure' || code === 'invalid') return t('conversation.runtime.runtimeSessionFailed');
    return translateRuntimeError(reason);
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
  const headerAreaRef = useRef<HTMLDivElement>(null);
  const chatDialogRef = useRef<ACPChatDialogHandle>(null);

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

  const handleWorkflowNodeOpenSession = useCallback((graphNode: GraphNodeVm) => {
    const nodeId = graphNode.nodeId;
    if (!nodeId) return;
    for (let r = run.sessionTree.rounds.length - 1; r >= 0; r--) {
      const round = run.sessionTree.rounds[r];
      for (const node of round.nodes) {
        if (node.nodeId === nodeId && node.attempts.length > 0) {
          onSelectSession(node.attempts[node.attempts.length - 1]);
          setWorkflowSheet({ open: false, mode: workflowSheet.mode });
          return;
        }
      }
    }
  }, [run.sessionTree, onSelectSession, workflowSheet.mode]);

  const isRunning = run.runStatus === 'running';
  const selectedLeaf = findSelectedLeaf(run.sessionTree);

  const handleOpenInFileManager = useCallback(() => {
    if (!selectedLeaf) return;
    openInFileManager(
      run.taskId,
      run.runId,
      selectedLeaf.roundId,
      selectedLeaf.nodeId,
      selectedLeaf.attemptId,
      selectedLeaf.outerNodeId,
      selectedLeaf.outerAttemptId,
    );
  }, [run.taskId, run.runId, selectedLeaf]);

  const handleAtBottomChange = useCallback((atBottom: boolean) => {
    isAtBottomRef.current = atBottom;
  }, []);

  const handleSessionStopped = useCallback(() => {
    onSessionStopped();
    if (isAtBottomRef.current && run.activeSessions.length > 1) {
      // Find the next running session to auto-switch
      const currentKey = run.sessionTree.selectedSessionKey;
      const nextActive = run.activeSessions.find(
        (s) => `${s.roundId}/${s.nodeId}/${s.attemptId}` !== currentKey,
      );
      if (nextActive) {
        onSelectSession({
          roundId: nextActive.roundId,
          nodeId: nextActive.nodeId,
          attemptId: nextActive.attemptId,
          outerNodeId: nextActive.outerNodeId,
          outerAttemptId: nextActive.outerAttemptId,
          pathLabel: nextActive.pathLabel,
          status: nextActive.status,
          runtimeDisplay: nextActive.runtimeDisplay,
          current: true,
          artifactCount: 0,
          attachmentCount: 0,
        });
      }
    }
  }, [onSessionStopped, run.activeSessions, run.sessionTree.selectedSessionKey, onSelectSession]);

  const handleRerun = () => {
    if (isRunning) {
      setRerunConfirmOpen(true);
    } else {
      onRerun();
    }
  };

  // Composer state is driven by the currently selected session, not the run.
  const selectedSessionDisplay = selectedLeaf?.runtimeDisplay;
  const selectedSessionErrorBlocked = selectedSessionDisplay?.code === 'error-blocked';
  const selectedSessionPaused = selectedSessionDisplay?.code === 'paused';
  const selectedSessionFailed = selectedSessionDisplay?.tone === 'danger'
    && selectedSessionDisplay?.terminal;
  // Workflow invalidity only blocks runtime-continue (handled by backend); it doesn't lock
  // an already-completed session's chat composer.
  const externalComposerState: AcpExternalComposerState | undefined =
    selectedSessionFailed || selectedSessionErrorBlocked
      ? { kind: 'runtime-error', errorMessage: translateSelectedRuntimeError(selectedSessionDisplay?.code, run.pauseReason), onRepair: handleRepairWorkflow }
      : run.resumable && !run.workflowValid
          ? { kind: 'invalid-workflow', workflowError: t('conversation.runtime.workflowInvalid'), onRepair: handleRepairWorkflow }
          : selectedSessionPaused
            ? { kind: 'paused', message: translatePauseReason(run.pauseReason), onContinue: onContinueRun }
            : undefined;

  return (
    <TooltipProvider>
      <div className="flex h-full min-h-0 flex-col bg-background">
        <div ref={headerAreaRef} className="shrink-0 relative">
          <ConversationRunHeader
            run={run}
            selectedSessionLeaf={selectedLeaf}
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
                  onSelectSession(leaf);
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
                onClick={() => onSelectSession({
                  roundId: session.roundId,
                  nodeId: session.nodeId,
                  attemptId: session.attemptId,
                  outerNodeId: session.outerNodeId,
                  outerAttemptId: session.outerAttemptId,
                  pathLabel: session.pathLabel,
                  status: session.status,
                  runtimeDisplay: session.runtimeDisplay,
                  current: true,
                  artifactCount: 0,
                  attachmentCount: 0,
                })}
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
            key={run.sessionTree.selectedSessionKey ?? 'empty'}
            session={run.selectedSession}
            taskId={run.taskId}
            runId={run.runId}
            roundId={selectedLeaf.roundId}
            nodeId={selectedLeaf.nodeId}
            attemptId={selectedLeaf.attemptId}
            outerNodeId={selectedLeaf.outerNodeId}
            outerAttemptId={selectedLeaf.outerAttemptId}
            eventPageSize={appConfig.acpChatEventPageSize}
            onSessionStopped={handleSessionStopped}
            onAtBottomChange={handleAtBottomChange}
            externalComposerState={externalComposerState}
            artifacts={run.artifacts}
            attachments={run.attachments}
            usageCompact
          />
        ) : (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            {t('conversation.runtime.noActiveSession')}
          </div>
        )}
      </div>

      {/* Assets bar */}
      <ConversationAssetsBar
        artifacts={run.artifacts}
        attachments={run.attachments}
        inputAttachments={run.inputAttachments}
        onOpenArtifact={(a) => chatDialogRef.current?.openArtifactsDialog(a)}
        onOpenAttachment={(a) => chatDialogRef.current?.openArtifactsDialog(a)}
      />

      {/* Rerun confirmation dialog */}
      <AlertDialog open={rerunConfirmOpen} onOpenChange={setRerunConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t('conversation.runtime.rerunConfirmTitle')}</AlertDialogTitle>
            <p className="text-sm text-muted-foreground">{t('conversation.runtime.rerunConfirmDescription')}</p>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t('common.close')}</AlertDialogCancel>
            <AlertDialogAction onClick={() => { setRerunConfirmOpen(false); onRerun(); }}>
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
        workflowErrorMessage={!run.workflowValid ? t('conversation.runtime.workflowInvalid') : translateSelectedRuntimeError(selectedSessionDisplay?.code, run.pauseReason)}
        validationRequestId={workflowValidationRequestId}
        onShowValidationIssues={() => setWorkflowValidationRequestId((value) => value + 1)}
        onSave={onSaveWorkflow}
        onClose={() => setWorkflowSheet({ open: false, mode: workflowSheet.mode })}
        onNodeOpenSession={handleWorkflowNodeOpenSession}
        t={t}
      />
    </div>
    </TooltipProvider>
  );
}

function leafKey(leaf: ConversationSessionLeafVm): string {
  if (leaf.outerNodeId && leaf.outerAttemptId) {
    return `${leaf.roundId}/${leaf.outerNodeId}/${leaf.outerAttemptId}/${leaf.nodeId}/${leaf.attemptId}`;
  }
  return `${leaf.roundId}/${leaf.nodeId}/${leaf.attemptId}`;
}

function findSelectedLeaf(tree: ConversationRunVm['sessionTree']): ConversationSessionLeafVm | null {
  const key = tree.selectedSessionKey;
  if (!key) return null;
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
  return null;
}

// ── Workflow sheet (edit / view) ──

function WorkflowSheet({
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
  const workflow = parseWorkflowJson(workflowJson);

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
}
