import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { getGitCapability, initializeGitRepository, openExternalUrl } from '@/api';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import type { GitCapabilityVm } from '@/types';

const GIT_DOWNLOAD_URL = 'https://git-scm.com/downloads';

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
        <DialogFooter className="flex-col-reverse gap-2 sm:flex-row sm:flex-wrap sm:justify-end">
          <Button variant="ghost" onClick={() => onOpenChange(false)}>{t('common.cancel')}</Button>
          <Button variant="outline" onClick={onUseOtherWorkflow}>
            {runKind === 'worktree'
              ? t('conversation.gitRequirement.useMainWorkspace')
              : t('conversation.gitRequirement.useOtherWorkflow')}
          </Button>
          {repositoryRequired ? (
            <Button onClick={() => void initialize()} disabled={checking}>{t('conversation.gitRequirement.initialize')}</Button>
          ) : (
            <>
              <Button variant="outline" onClick={() => void openExternalUrl(GIT_DOWNLOAD_URL)}>
                {t('conversation.gitRequirement.openDownloads')}
              </Button>
              <Button onClick={() => void recheck()} disabled={checking}>
                {checking ? t('conversation.gitRequirement.checking') : t('conversation.gitRequirement.recheck')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
