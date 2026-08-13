/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AcpConversationComposer } from '@/components/conversation/AcpConversationComposer';
import '@/i18n';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

type ComposerProps = React.ComponentProps<typeof AcpConversationComposer>;

function baseProps(overrides: Partial<ComposerProps> = {}): ComposerProps {
  return {
    prompt: '',
    onPromptChange: vi.fn(),
    onSubmit: vi.fn(),
    sending: false,
    status: null,
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
    inputHint: '',
    canStop: false,
    stopInProgress: false,
    onStop: vi.fn(),
    canSubmit: true,
    sendButtonBusy: false,
    showRuntimeContinue: false,
    runtimeContinueSubmitting: false,
    onRuntimeContinue: vi.fn(),
    configBar: null,
    attachedQueueVisible: true,
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
    await renderComposer({ attachments: [], attachedQueueVisible: true });

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
  });

  it('renders quotes and attachments inside the prompt input context area', async () => {
    await renderComposer({
      quotes: [{ id: 'quote-1', sourceKey: 'answer-1', text: '引用内容' }],
      attachments: [{ id: 'image-1', name: 'image.png', size: 12, mime: 'image/png', source: 'dialog', previewUrl: 'blob:image' }],
    });

    const contextArea = host.querySelector('[data-composer-context-area="true"]');
    const promptInput = host.querySelector('[data-slot="prompt-input"]');
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
});
