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
  runKind: 'auto' | 'workflow';
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
  const { i18n } = useTranslation();
  const zh = i18n.resolvedLanguage?.toLowerCase().startsWith('zh') ?? true;
  const [status, setStatus] = useState(initialStatus);
  const [checking, setChecking] = useState(false);

  const repositoryRequired = status === 'repository-required';
  const headRequired = status === 'head-required';
  const title = repositoryRequired
    ? (zh ? '当前文件夹还不是 Git 仓库' : 'This folder is not a Git repository')
    : headRequired
      ? (zh ? 'Git 仓库需要首次提交' : 'An initial Git commit is required')
      : runKind === 'auto'
        ? (zh ? 'Auto 模式需要 Git' : 'Auto mode requires Git')
        : (zh ? '此工作流需要 Git' : 'This workflow requires Git');
  const description = repositoryRequired
    ? (zh ? '初始化仓库后还需要在 Git 工作区完成首次提交，Gold Band 不会自动暂存或提交整个目录。' : 'After initialization, create the first commit in the Git workspace. Gold Band will not stage or commit the whole folder automatically.')
    : headRequired
      ? (zh ? '请在右侧 Git 工作区完成首次提交。Gold Band 不会替你选择或提交文件。' : 'Create the first commit in the Git workspace. Gold Band will not choose or commit files for you.')
      : runKind === 'auto'
        ? (zh ? '安装 Git 并重新检测后即可使用 Auto 模式。你也可以选择其他不包含 AI-DYNAMIC 的工作流。' : 'Install Git and check again to use Auto mode, or choose a workflow without AI-DYNAMIC.')
        : (zh ? '此工作流包含 AI-DYNAMIC，需要 Git、有效 HEAD 和 worktree 能力才能运行。' : 'This workflow contains AI-DYNAMIC and requires Git, a valid HEAD, and worktree support.');

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
          <Button variant="ghost" onClick={() => onOpenChange(false)}>{zh ? '取消' : 'Cancel'}</Button>
          <Button variant="outline" onClick={onUseOtherWorkflow}>{zh ? '使用其他工作流' : 'Use another workflow'}</Button>
          {repositoryRequired ? (
            <Button onClick={() => void initialize()} disabled={checking}>{zh ? '初始化仓库' : 'Initialize repository'}</Button>
          ) : (
            <>
              <Button variant="outline" onClick={() => void openExternalUrl(GIT_DOWNLOAD_URL)}>
                {zh ? '打开 Git 下载页面' : 'Open Git downloads'}
              </Button>
              <Button onClick={() => void recheck()} disabled={checking}>
                {checking ? (zh ? '检测中…' : 'Checking…') : (zh ? '重新检测' : 'Check again')}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
