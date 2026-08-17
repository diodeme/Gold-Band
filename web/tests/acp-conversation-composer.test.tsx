/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AcpConversationComposer } from '@/components/conversation/AcpConversationComposer';
import { ACP_SESSION_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';
import '@/i18n';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

type ComposerProps = React.ComponentProps<typeof AcpConversationComposer>;

function baseProps(overrides: Partial<ComposerProps> = {}): ComposerProps {
  return {
    prompt: '',
    onPromptChange: vi.fn(),
    onSubmit: vi.fn(),
    sending: false,
    attachments: [],
    quotes: [],
    contextError: null,
    onRemoveQuote: vi.fn(),
    onRemoveAttachment: vi.fn(),
    onPreviewAttachment: vi.fn(),
    onClearAttachments: vi.fn(),
    fileError: null,
    slashCommands: [],
    slashMenuOpen: false,
    slashMenuActiveIndex: 0,
    onSlashMenuActiveIndexChange: vi.fn(),
    onSlashMenuDismiss: vi.fn(),
    onSlashMenuSelect: vi.fn(),
    textareaRef: React.createRef<HTMLTextAreaElement>(),
    committedSlashCommand: null,
    placeholder: '继续会话...',
    inputDisabled: false,
    onTextareaKeyDown: vi.fn(),
    onDragEnter: vi.fn(),
    onDragOver: vi.fn(),
    onDrop: vi.fn(),
    onPaste: vi.fn(),
    fileInputRef: React.createRef<HTMLInputElement>(),
    onFilesChange: vi.fn(),
    onPickFiles: vi.fn(),
    canStop: false,
    stopInProgress: false,
    onStop: vi.fn(),
    canSubmit: true,
    sendButtonBusy: false,
    showRuntimeContinue: false,
    runtimeContinueKind: null,
    runtimeContinueSubmitting: false,
    onRuntimeContinue: vi.fn(),
    configBar: null,
    attachedPanelVisible: true,
    integratedInfoTab: false,
    queueSubmit: true,
    ...overrides,
  };
}

describe('AcpConversationComposer', () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement('div');
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  async function renderComposer(props: Partial<ComposerProps> = {}) {
    await act(async () => root.render(<AcpConversationComposer {...baseProps(props)} />));
  }

  it('does not render an empty attachment spacer between an attached queue and the prompt input', async () => {
    await renderComposer({ attachments: [], attachedPanelVisible: true });

    const composerRoot = host.querySelector('[data-conversation-composer="acp"]');
    expect(composerRoot?.querySelector('[data-acp-composer-attachment-row="true"]')).toBeNull();
    expect(composerRoot?.querySelector('[class*="rounded-t-none"]')).toBeTruthy();
  });

  it('renders config, workflow continuation, and send in one bottom command bar', async () => {
    await renderComposer({ showRuntimeContinue: true, configBar: <span data-test-config="true">config</span> });

    const commandBar = host.querySelector('[data-acp-composer-command-bar="true"]');
    const continueButton = host.querySelector('[data-acp-continue-workflow="true"]');
    const sendButton = host.querySelector('[data-acp-send="true"]');
    const config = host.querySelector('[data-test-config="true"]');
    expect(commandBar).toBeTruthy();
    expect(continueButton).toBeTruthy();
    expect(sendButton).toBeTruthy();
    expect(commandBar?.contains(continueButton)).toBe(true);
    expect(commandBar?.contains(sendButton)).toBe(true);
    expect(commandBar?.contains(config)).toBe(true);
    expect(commandBar?.className).toBe(ACP_SESSION_COMPOSER_LAYOUT.commandBarClassName);
    expect(sendButton?.className).toContain(ACP_SESSION_COMPOSER_LAYOUT.actionButtonClassName);
  });

  it('places the localized attachment action before config and keeps the textarea user-resizable', async () => {
    await renderComposer({ configBar: <span data-test-config="true">config</span> });

    const commandBar = host.querySelector('[data-acp-composer-command-bar="true"]');
    const attachmentButton = host.querySelector<HTMLButtonElement>('button[aria-label="添加附件"]');
    const config = host.querySelector('[data-test-config="true"]');
    const textarea = host.querySelector('textarea');
    expect(attachmentButton).toBeTruthy();
    expect(config).toBeTruthy();
    expect(commandBar?.contains(attachmentButton)).toBe(true);
    expect(attachmentButton?.compareDocumentPosition(config as Node) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(textarea?.className).toContain('resize-y');
    expect(textarea?.className).toContain('min-h-12');
    expect(textarea?.style.maxHeight).toBe('320px');
  });

  it('does not reserve a standalone keyboard-hint row', async () => {
    await renderComposer();

    expect(host.textContent).not.toContain('Enter 发送');
    expect(host.textContent).not.toContain('Shift+Enter');
  });

  it('keeps a drag-selected height as the local autosize minimum', async () => {
    await renderComposer();

    const textarea = host.querySelector<HTMLTextAreaElement>('textarea');
    expect(textarea).toBeTruthy();
    let height = 48;
    vi.spyOn(textarea as HTMLTextAreaElement, 'getBoundingClientRect').mockImplementation(() => ({
      x: 0,
      y: 0,
      width: 320,
      height,
      top: 0,
      right: 320,
      bottom: height,
      left: 0,
      toJSON: () => ({}),
    }));

    await act(async () => {
      textarea?.dispatchEvent(new Event('pointerdown', { bubbles: true }));
      height = 180;
      window.dispatchEvent(new Event('pointerup'));
    });

    expect(textarea?.style.height).toBe('180px');
  });

  it('renders quotes and attachments inside the prompt input context area', async () => {
    await renderComposer({
      quotes: [{ id: 'quote-1', sourceKey: 'answer-1', text: '引用内容' }],
      attachments: [{ id: 'image-1', name: 'image.png', size: 12, mime: 'image/png', source: 'dialog', previewUrl: 'blob:image' }],
    });

    const contextArea = host.querySelector('[data-composer-context-area="true"]');
    const promptInput = host.querySelector('[data-slot="prompt-input"]');
    expect(promptInput?.classList.contains('border-0')).toBe(true);
    expect(promptInput?.classList.contains('border')).toBe(false);
    const textarea = host.querySelector('textarea');
    expect(contextArea).toBeTruthy();
    expect(promptInput).toBeTruthy();
    expect(promptInput?.contains(contextArea)).toBe(true);
    expect(promptInput?.contains(textarea)).toBe(true);
    expect(contextArea?.querySelector('[data-composer-quote-chip="true"]')).toBeTruthy();
    const imageChip = contextArea?.querySelector('[data-composer-attachment-chip="true"]');
    expect(imageChip?.querySelector('img')).toBeTruthy();
    expect(imageChip?.textContent).not.toContain('image.png');
  });

  it('replaces the textarea with a linked read-only notice for a superseded session', async () => {
    const onNavigate = vi.fn();
    const onDrop = vi.fn();
    await renderComposer({
      inputDisabled: true,
      canSubmit: false,
      queueSubmit: false,
      onDrop,
      supersededSession: {
        label: 'node-x / attempt-003',
        href: '/chat/projects/project-a/tasks/task-a/runs/run-a/rounds/round-a/nodes/node-x/attempts/attempt-003',
        onNavigate,
      },
    });

    const notice = host.querySelector('[data-acp-session-superseded="true"]');
    const link = notice?.querySelector<HTMLAnchorElement>('a');
    const attachmentButton = host.querySelector<HTMLButtonElement>('button[aria-label="添加附件"]');
    const sendButton = host.querySelector<HTMLButtonElement>('[data-acp-send="true"]');
    expect(host.querySelector('textarea')).toBeNull();
    expect(notice?.textContent).toContain('此会话已由 node-x / attempt-003 接续');
    expect(link?.getAttribute('href')).toContain('/nodes/node-x/attempts/attempt-003');
    expect(link?.className).toContain('text-link');
    expect(link?.className).toContain('text-xs');
    expect(attachmentButton?.disabled).toBe(true);
    expect(sendButton?.disabled).toBe(true);

    await act(async () => link?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 })));
    expect(onNavigate).toHaveBeenCalledTimes(1);
    await act(async () => notice?.closest('[data-attachment-dropzone]')?.dispatchEvent(new Event('drop', { bubbles: true })));
    expect(onDrop).not.toHaveBeenCalled();
  });
});
