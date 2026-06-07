import { useCallback, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ACPChatDialog, type AcpExternalComposerState } from '@/components/acp/ACPChatDialog';
import { ConversationRunHeader } from '@/components/conversation/ConversationRunHeader';
import { ConversationSessionSwitcher } from '@/components/conversation/ConversationSessionSwitcher';
import { ConversationAssetsBar } from '@/components/conversation/ConversationAssetsBar';
import type { AcpSessionVm, AppConfigVm, ConversationRunVm, ConversationSessionLeafVm } from '../types';

interface ConversationRunPageProps {
  run: ConversationRunVm;
  appConfig: AppConfigVm;
  onRerun: () => void;
  onEditWorkflow: () => void;
  onSelectSession: (leaf: ConversationSessionLeafVm) => void;
  onSessionStopped: () => void;
  onContinueRun: () => void;
  onRepairWorkflow: () => void;
  onTitleChange?: (title: string) => void;
}

export function ConversationRunPage({
  run,
  appConfig,
  onRerun,
  onEditWorkflow,
  onSelectSession,
  onSessionStopped,
  onContinueRun,
  onRepairWorkflow,
  onTitleChange,
}: ConversationRunPageProps) {
  const { t } = useTranslation();
  const [sessionSwitcherOpen, setSessionSwitcherOpen] = useState(false);
  const [rerunConfirmOpen, setRerunConfirmOpen] = useState(false);
  const isAtBottomRef = useRef(true);

  const isRunning = run.runStatus === 'running';
  const selectedLeaf = findSelectedLeaf(run.sessionTree);

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
        <ConversationRunHeader
        run={run}
        onRerun={handleRerun}
        onEditWorkflow={onEditWorkflow}
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
