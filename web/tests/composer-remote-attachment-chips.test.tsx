/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ComposerContextArea } from '@/components/shared/ComposerContextArea';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

/**
 * 远程任务附件元数据 chips（输入框附件可见性）：
 * 只读展示（无移除按钮、无预览交互），随调用方传入的 remoteAttachments 渲染；
 * 空列表且无其他上下文时整个区域不渲染。
 */
describe('composer remote attachment chips', () => {
  let container: HTMLDivElement;
  let root: Root;

  afterEach(() => {
    document.body.replaceChildren();
  });

  function renderArea(props: Parameters<typeof ComposerContextArea>[0]) {
    container = document.createElement('div');
    document.body.append(container);
    root = createRoot(container);
    return act(async () => {
      root.render(
        <TooltipProvider>
          <ComposerContextArea {...props} />
        </TooltipProvider>,
      );
    });
  }

  it('renders one read-only chip per remote attachment (duplicates kept)', async () => {
    await renderArea({
      attachments: [],
      onRemoveAttachment: vi.fn(),
      onPreviewAttachment: vi.fn(),
      remoteAttachments: [
        { filename: '多值测试样本.zip', contentType: 'application/zip' },
        { filename: 'att-2', contentType: null },
        { filename: '多值测试样本.zip', contentType: null },
      ],
    });

    const chips = container.querySelectorAll('[data-composer-remote-attachment-chip]');
    expect(chips).toHaveLength(3);
    expect(chips[0].textContent).toBe('多值测试样本.zip');
    expect(chips[1].textContent).toBe('att-2');
    // 只读 chips：不携带任何按钮（移除/预览交互随本地附件，不适用于远程元数据）。
    chips.forEach((chip) => {
      expect(chip.querySelector('button')).toBeNull();
    });
  });

  it('renders nothing when remote attachments are empty and no other context exists', async () => {
    await renderArea({
      attachments: [],
      onRemoveAttachment: vi.fn(),
      onPreviewAttachment: vi.fn(),
      remoteAttachments: [],
    });

    expect(container.querySelector('[data-composer-context-area]')).toBeNull();
  });

  it('renders remote chips alongside local attachments in the same context row', async () => {
    await renderArea({
      attachments: [{
        id: 'local-1',
        name: '本地备注.txt',
        size: 4,
        mime: 'text/plain',
        source: 'dialog',
      }],
      onRemoveAttachment: vi.fn(),
      onPreviewAttachment: vi.fn(),
      remoteAttachments: [{ filename: '需求附件.zip', contentType: 'application/zip' }],
    });

    expect(container.querySelector('[data-composer-attachment-chip]')).not.toBeNull();
    const remote = container.querySelector('[data-composer-remote-attachment-chip]');
    expect(remote?.textContent).toBe('需求附件.zip');
  });

  it('disables preview and open actions for unavailable restored references', async () => {
    const onPreviewAttachment = vi.fn();
    const onOpenWorkspaceFile = vi.fn();
    await renderArea({
      attachments: [{
        id: 'missing',
        name: 'missing.png',
        size: 0,
        mime: 'image/png',
        source: 'dialog',
        unavailable: true,
      }],
      workspaceFiles: [{
        id: 'missing-file',
        source: { kind: 'workspace', projectId: 'p', relativePath: 'src/missing.ts' },
        projectId: 'p',
        relativePath: 'src/missing.ts',
        name: 'missing.ts',
        byteLength: null,
        mimeType: 'text/plain',
        unavailable: true,
      }],
      onOpenWorkspaceFile,
      onRemoveAttachment: vi.fn(),
      onRemoveWorkspaceFile: vi.fn(),
      onPreviewAttachment,
    });

    const attachment = container.querySelector<HTMLButtonElement>('[data-composer-attachment-unavailable="true"]');
    const workspaceFile = container.querySelector<HTMLButtonElement>('[data-composer-workspace-file-unavailable="true"]');
    expect(attachment?.disabled).toBe(true);
    expect(workspaceFile?.disabled).toBe(true);
    attachment?.click();
    workspaceFile?.click();
    expect(onPreviewAttachment).not.toHaveBeenCalled();
    expect(onOpenWorkspaceFile).not.toHaveBeenCalled();
  });
});
