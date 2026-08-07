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
});
