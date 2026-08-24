/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const api = vi.hoisted(() => ({
  getGitCapability: vi.fn(),
  initializeGitRepository: vi.fn(),
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
      'conversation.gitRequirement.openSourceControl': '打开源码管理',
      'conversation.gitRequirement.checking': '检测中…',
      'conversation.gitRequirement.recheck': '重新检测',
      'sourceControl.title': '源码管理',
      'sourceControl.description': '查看和管理当前工作区的 Git 状态',
    }[key] ?? key),
  }),
}));
vi.mock('@/api', () => api);

import { GitRequirementDialog } from '@/components/git/GitRequirementDialog';
import {
  createDraftConversationWorkspaceScope,
  RightWorkspaceProvider,
  useRightWorkspace,
} from '@/components/workspace/right-workspace-context';

function WorkspaceStateProbe() {
  const workspace = useRightWorkspace();
  return (
    <div
      data-workspace-active-tab={workspace.activeTabKey ?? ''}
      data-workspace-tab-kinds={workspace.tabs.map((tab) => tab.kind).join(',')}
    />
  );
}

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

  it('orders the worktree recovery actions and opens source control in the right workspace', async () => {
    const container = document.createElement('div');
    document.body.appendChild(container);
    const root = createRoot(container);
    const onOpenChange = vi.fn();

    await act(async () => {
      root.render(
        <RightWorkspaceProvider scope={createDraftConversationWorkspaceScope('project-1')}>
          <GitRequirementDialog
            open
            projectId="project-1"
            runKind="worktree"
            initialStatus="head-required"
            onReady={() => {}}
            onUseOtherWorkflow={() => {}}
            onOpenChange={onOpenChange}
          />
          <WorkspaceStateProbe />
        </RightWorkspaceProvider>,
      );
    });

    expect(document.body.textContent).toContain('Git 仓库需要首次提交');
    expect(document.body.textContent).not.toContain('使用其他工作流');
    expect(document.body.textContent).not.toContain('打开 Git 下载页面');

    const actions = [...document.body.querySelectorAll<HTMLButtonElement>('[data-slot="dialog-footer"] button')];
    expect(actions.map((button) => button.textContent)).toEqual([
      '取消',
      '重新检测',
      '使用主工作区',
      '打开源码管理',
    ]);
    expect(actions[1]?.className).toContain('border');
    expect(actions[2]?.className).toContain('border');
    expect(actions[3]?.className).toContain('bg-primary');

    await act(async () => actions[3]?.click());

    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(document.querySelector('[data-workspace-active-tab]')?.getAttribute('data-workspace-active-tab'))
      .toBe('source-control:project-1:main');
    expect(document.querySelector('[data-workspace-tab-kinds]')?.getAttribute('data-workspace-tab-kinds'))
      .toBe('source-control');
    await act(async () => root.unmount());
  });
});
