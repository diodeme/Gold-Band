/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  getGitCapability: vi.fn(),
  initializeGitRepository: vi.fn(),
  openExternalUrl: vi.fn(),
}));

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => ({
      'common.cancel': '取消',
      'conversation.gitRequirement.repositoryTitle': '当前文件夹还不是 Git 仓库',
      'conversation.gitRequirement.headTitle': 'Git 仓库需要首次提交',
      'conversation.gitRequirement.autoTitle': 'Auto 模式需要 Git',
      'conversation.gitRequirement.workflowTitle': '此工作流需要 Git',
      'conversation.gitRequirement.worktreeTitle': '新工作树需要 Git',
      'conversation.gitRequirement.repositoryDescription': '初始化仓库后还需要在 Git 工作区完成首次提交，Gold Band 不会自动暂存或提交整个目录。',
      'conversation.gitRequirement.headDescription': '请在右侧 Git 工作区完成首次提交。Gold Band 不会替你选择或提交文件。',
      'conversation.gitRequirement.autoDescription': '安装 Git 并重新检测后即可使用 Auto 模式。',
      'conversation.gitRequirement.workflowDescription': '此工作流包含 AI-DYNAMIC，需要 Git。',
      'conversation.gitRequirement.worktreeDescription': '创建工作树需要 Git。',
      'conversation.gitRequirement.useOtherWorkflow': '使用其他工作流',
      'conversation.gitRequirement.useMainWorkspace': '使用主工作区',
      'conversation.gitRequirement.initialize': '初始化仓库',
      'conversation.gitRequirement.openDownloads': '打开 Git 下载页面',
      'conversation.gitRequirement.checking': '检测中…',
      'conversation.gitRequirement.recheck': '重新检测',
    }[key] ?? key),
  }),
}));
vi.mock('@/api', () => api);

import { GitRequirementDialog } from '@/components/git/GitRequirementDialog';

afterEach(() => {
  vi.clearAllMocks();
  document.body.innerHTML = '';
});

describe('Git requirement dialog', () => {
  it('initializes only the repository and then asks for the first commit', async () => {
    api.initializeGitRepository.mockResolvedValue({
      status: 'head-required',
      repoRoot: 'D:/repo',
      commonDir: 'D:/repo/.git',
      head: null,
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <GitRequirementDialog
          open
          projectId="project-1"
          runKind="workflow"
          initialStatus="repository-required"
          onReady={() => {}}
          onUseOtherWorkflow={() => {}}
          onOpenChange={() => {}}
        />,
      );
    });

    const initialize = [...document.body.querySelectorAll('button')]
      .find((button) => button.textContent === '初始化仓库');
    expect(initialize).toBeDefined();
    await act(async () => initialize?.click());

    expect(api.initializeGitRepository).toHaveBeenCalledWith('project-1');
    expect(document.body.textContent).toContain('Git 仓库需要首次提交');
    expect(document.body.textContent).toContain('Gold Band 不会替你选择或提交文件');
    await act(async () => root.unmount());
  });

  it('closes and resumes only after recheck reports ready', async () => {
    api.getGitCapability.mockResolvedValue({
      status: 'ready',
      repoRoot: 'D:/repo',
      commonDir: 'D:/repo/.git',
      head: 'abc123',
    });
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);
    const onReady = vi.fn();
    const onOpenChange = vi.fn();

    await act(async () => {
      root.render(
        <GitRequirementDialog
          open
          projectId="project-1"
          runKind="auto"
          initialStatus="not-installed"
          onReady={onReady}
          onUseOtherWorkflow={() => {}}
          onOpenChange={onOpenChange}
        />,
      );
    });

    const recheck = [...document.body.querySelectorAll('button')]
      .find((button) => button.textContent === '重新检测');
    await act(async () => recheck?.click());

    expect(api.getGitCapability).toHaveBeenCalledWith('project-1');
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onReady).toHaveBeenCalledOnce();
    await act(async () => root.unmount());
  });

  it('offers the main workspace instead of another workflow for worktree selection', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);

    await act(async () => {
      root.render(
        <GitRequirementDialog
          open
          projectId="project-1"
          runKind="worktree"
          initialStatus="worktree-required"
          onReady={() => {}}
          onUseOtherWorkflow={() => {}}
          onOpenChange={() => {}}
        />,
      );
    });

    expect(document.body.textContent).toContain('新工作树需要 Git');
    expect(document.body.textContent).toContain('使用主工作区');
    expect(document.body.textContent).not.toContain('使用其他工作流');
    await act(async () => root.unmount());
  });
});
