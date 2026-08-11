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
  useTranslation: () => ({ i18n: { resolvedLanguage: 'zh-CN' } }),
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
});
