import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { getGitCapability, initializeGitRepository } from '@/api';
import { Button } from '@/components/ui/button';
import { sourceControlWorkspaceResourceKey, useOptionalRightWorkspaceCommands } from '@/components/workspace/right-workspace-context';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { GitCapabilityVm } from '@/types';

interface GitRequirementDialogProps {
  open: boolean;
  projectId?: string | null;
  runKind: 'auto' | 'workflow' | 'worktree';
  initialStatus: GitCapabilityVm['status'];
  onReady: () => void | Promise<void>;
  onUseOtherWorkflow: () => void;
  onOpenChange: (open: boolean) => void;
}

export function GitRequirementDialog({
  open,
  projectId,
  runKind,
  initialStatus,
  onReady,
  onUseOtherWorkflow,
  onOpenChange,
}: GitRequirementDialogProps) {
  const { t } = useTranslation();
  const workspaceCommands = useOptionalRightWorkspaceCommands();
  const [status, setStatus] = useState(initialStatus);
  const [checking, setChecking] = useState(false);

  const repositoryRequired = status === 'repository-required';
  const headRequired = status === 'head-required';
  const title = repositoryRequired
    ? t('conversation.gitRequirement.repositoryTitle')
    : headRequired
      ? t('conversation.gitRequirement.headTitle')
      : runKind === 'worktree'
        ? t('conversation.gitRequirement.worktreeTitle')
      : runKind === 'auto'
        ? t('conversation.gitRequirement.autoTitle')
        : t('conversation.gitRequirement.workflowTitle');
  const description = repositoryRequired
    ? t('conversation.gitRequirement.repositoryDescription')
    : headRequired
      ? t('conversation.gitRequirement.headDescription')
      : runKind === 'worktree'
        ? t('conversation.gitRequirement.worktreeDescription')
      : runKind === 'auto'
        ? t('conversation.gitRequirement.autoDescription')
        : t('conversation.gitRequirement.workflowDescription');

  function openSourceControl() {
    if (!projectId || !workspaceCommands?.scopeKey || workspaceCommands.projectId !== projectId) return;
    void workspaceCommands.openResource({
      kind: 'source-control',
      key: sourceControlWorkspaceResourceKey(projectId),
      scopeKey: workspaceCommands.scopeKey,
      projectId,
      title: t('sourceControl.title'),
      description: t('sourceControl.description'),
      attention: false,
    });
    onOpenChange(false);
  }

  async function recheck() {
    setChecking(true);
    try {
      const capability = await getGitCapability(projectId);
      setStatus(capability.status);
      if (capability.status === 'ready') {
        onOpenChange(false);
        await onReady();
      }
    } finally {
      setChecking(false);
    }
  }

  async function initialize() {
    setChecking(true);
    try {
      const capability = await initializeGitRepository(projectId);
      setStatus(capability.status);
    } finally {
      setChecking(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <DialogFooter className="flex-col gap-2 sm:flex-row sm:flex-wrap sm:justify-end">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>{t('common.cancel')}</Button>
          {repositoryRequired ? (
            <>
              <Button variant="outline" onClick={onUseOtherWorkflow}>
                {runKind === 'worktree'
                  ? t('conversation.gitRequirement.useMainWorkspace')
                  : t('conversation.gitRequirement.useOtherWorkflow')}
              </Button>
              <Button onClick={() => void initialize()} disabled={checking}>{t('conversation.gitRequirement.initialize')}</Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={() => void recheck()} disabled={checking}>
                {checking ? t('conversation.gitRequirement.checking') : t('conversation.gitRequirement.recheck')}
              </Button>
              <Button variant="outline" onClick={onUseOtherWorkflow}>
                {runKind === 'worktree'
                  ? t('conversation.gitRequirement.useMainWorkspace')
                  : t('conversation.gitRequirement.useOtherWorkflow')}
              </Button>
              {projectId && workspaceCommands?.scopeKey && workspaceCommands.projectId === projectId ? (
                <Button onClick={openSourceControl}>
                  {t('conversation.gitRequirement.openSourceControl')}
                </Button>
              ) : null}
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
