import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet';
import { ACPChatDialog, type ACPChatDialogHandle, type AcpExternalComposerState } from '@/components/acp/ACPChatDialog';
import { ConversationRunHeader } from '@/components/conversation/ConversationRunHeader';
import { ConversationSessionSwitcher } from '@/components/conversation/ConversationSessionSwitcher';
import { ConversationAssetsBar } from '@/components/conversation/ConversationAssetsBar';
import { WorkflowEditor, parseWorkflowJson } from '@/components/WorkflowEditor';
import { GraphView } from '@/components/GraphView';
import type { AcpSessionVm, AgentRegistryVm, AppConfigVm, ConversationRunVm, ConversationSessionLeafVm, GraphNodeVm, GraphVm, ProfileVm } from '../types';
import { getAgentRegistry, getProfiles, openInFileManager } from '@/api';

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
  onRepairWorkflow: () => void;
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
  onRepairWorkflow,
  onTitleChange,
}: ConversationRunPageProps) {
  const { t } = useTranslation();
  const [sessionSwitcherOpen, setSessionSwitcherOpen] = useState(false);
  const [rerunConfirmOpen, setRerunConfirmOpen] = useState(false);
  const [workflowSheet, setWorkflowSheet] = useState<{ open: boolean; mode: 'edit' | 'view' }>({ open: false, mode: 'view' });
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

  // Lazy-load workflow editor dependencies when opening the edit workflow sheet
  const handleEditWorkflow = useCallback(() => {
    const open = () => setWorkflowSheet({ open: true, mode: 'edit' });
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
  }, [effectiveAgentRegistry, workflowProfiles]);

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

  const externalComposerState: AcpExternalComposerState | undefined =
    !run.workflowValid
      ? { kind: 'invalid-workflow', workflowError: t('conversation.runtime.workflowInvalid') }
      : run.runOutcome === 'failure'
        ? { kind: 'runtime-error', errorMessage: run.pauseReason ?? t('conversation.runtime.runError'), onRepair: onRepairWorkflow }
        : undefined;

  return (
    <TooltipProvider>
      <div className="flex h-full min-h-0 flex-col bg-background">
        <div ref={headerAreaRef} className="shrink-0 relative">
          <ConversationRunHeader
            run={run}
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
                  current: true,
                  artifactCount: 0,
                  attachmentCount: 0,
                })}
              >
                <span className="font-medium">{session.pathLabel}</span>
                {session.status === 'running' ? (
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

function WorkflowSheet({ open, mode, workflowJson, workflowGraph, agentRegistry, profiles, onSave, onClose, onNodeOpenSession, t }: { open: boolean; mode: 'edit' | 'view'; workflowJson?: string | null; workflowGraph: GraphVm; agentRegistry: AgentRegistryVm | null; profiles: ProfileVm[]; onSave?: (json: string) => Promise<void>; onClose: () => void; onNodeOpenSession?: (node: GraphNodeVm) => void; t: (key: string) => string }) {
  const workflow = parseWorkflowJson(workflowJson);

  if (mode === 'edit') {
    return (
      <Sheet open={open} onOpenChange={(o) => { if (!o) onClose(); }}>
        <SheetContent
          className="gap-0 overflow-hidden border-border bg-card p-0 sm:max-w-5xl"
          resizeStorageKey="conversation/workflow-edit"
          closeLabel={t('common.close')}
        >
          <SheetHeader className="shrink-0 border-b px-5 py-4 text-left">
            <SheetTitle>{t('conversation.runtime.editWorkflow')}</SheetTitle>
          </SheetHeader>
          <div className="min-h-0 flex-1 overflow-auto">
            {workflow ? (
              <WorkflowEditor
                value={workflow}
                agentRegistry={agentRegistry}
                profiles={profiles}
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
