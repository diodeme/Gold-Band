/** @vitest-environment jsdom */
import React, { act, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { AcpConversationComposer, type AcpConversationComposerProps } from '@/components/conversation/AcpConversationComposer';
import { useSlashCommandController } from '@/hooks/useSlashCommandController';
import { TooltipProvider } from '@/components/ui/tooltip';

const api = vi.hoisted(() => ({
  listComposerHistory: vi.fn(async () => ({ items: [{ cursor: { generation: 1, messageId: '1', position: 1 }, textBytes: 7 }], head: null, nextCursor: null })),
  getComposerHistoryText: vi.fn(async () => ({ cursor: { generation: 1, messageId: '1', position: 1 }, text: '/review' })),
}));
vi.mock('@/api/client', () => ({ getRuntimeApi: () => api }));
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }), Trans: () => null }));
const noop = () => {};
const submit = vi.fn();
const commands = [{ name: 'review', description: 'Review', input: null }, { name: 'reset', description: 'Reset', input: null }];
const defaults: Omit<AcpConversationComposerProps, 'prompt' | 'onPromptChange'> = {
  onSubmit: submit, sending: false, attachments: [], quotes: [], contextError: null, fileError: null,
  onRemoveQuote: noop, onRemoveAttachment: noop, onPreviewAttachment: noop, onClearAttachments: noop,
  slashCommands: [], slashMenuOpen: false, slashMenuActiveIndex: 0, onSlashMenuActiveIndexChange: noop,
  onSlashMenuDismiss: noop, onSlashMenuSelect: noop, textareaRef: null, placeholder: 'Message', inputDisabled: false,
  onTextareaKeyDown: noop, onDragEnter: noop, onDragOver: noop, onDrop: noop, onPaste: noop,
  fileInputRef: null, onFilesChange: noop, onPickFiles: noop, canStop: false, stopInProgress: false, onStop: noop,
  canSubmit: true, sendButtonBusy: false, showRuntimeContinue: false, runtimeContinueKind: null,
  runtimeContinueSubmitting: false, onRuntimeContinue: noop, configBar: null, attachedPanelVisible: false,
  integratedInfoTab: false, queueSubmit: false,
  historyLocator: { projectId: 'p', taskId: 't', runId: 'r', roundId: 'round', nodeId: 'n', attemptId: 'a' },
};
function Harness({ initial = '' }: { initial?: string }) {
  const [prompt, setPrompt] = useState(initial);
  const slash = useSlashCommandController({ input: prompt, commands, onInputChange: setPrompt });
  return <TooltipProvider><AcpConversationComposer {...defaults} prompt={prompt} onPromptChange={setPrompt}
    onSubmit={() => { submit(prompt); setPrompt(''); }} slashCommands={slash.filteredCommands}
    slashMenuOpen={slash.isOpen} slashMenuActiveIndex={slash.activeIndex} onSlashMenuActiveIndexChange={slash.setActiveIndex}
    onSlashMenuDismiss={slash.dismiss} onSlashMenuSelect={slash.selectByIndex} onTextareaKeyDown={slash.onKeyDown} /></TooltipProvider>;
}
let cleanup = async () => {};
afterEach(async () => { await cleanup(); vi.clearAllMocks(); });
async function mount(initial = '') {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } });
  Element.prototype.scrollIntoView = noop;
  const host = document.createElement('div'); document.body.append(host);
  const root = createRoot(host);
  cleanup = async () => { await act(async () => root.unmount()); host.remove(); };
  await act(async () => root.render(<Harness initial={initial} />));
  const textarea = host.querySelector('textarea')!;
  const press = async (key: string) => { await act(async () => textarea.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }))); };
  return { textarea, press };
}
it('recalls slash text without opening suggestions and submits through the ordinary callback', async () => {
  const { textarea, press } = await mount();
  await press('ArrowUp'); expect(textarea.value).toBe('/review');
  expect(document.querySelector('[role="listbox"]')).toBeNull();
  await press('Enter'); expect(submit).toHaveBeenCalledWith('/review'); expect(textarea.value).toBe('');
});
it('gives an open slash menu priority over input history', async () => {
  const { textarea, press } = await mount('/');
  await press('ArrowDown');
  expect(textarea.value).toBe('/'); expect(api.listComposerHistory).not.toHaveBeenCalled();
  expect(document.querySelector('[role="option"][aria-selected="true"]')?.textContent).toContain('/reset');
  await press('Enter'); expect(submit).not.toHaveBeenCalled(); expect(textarea.value).toContain('/reset');
});

it('recalls history without a loading icon or toolbar layout changes', async () => {
  let finish!: (value: Awaited<ReturnType<typeof api.getComposerHistoryText>>) => void;
  api.getComposerHistoryText.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const { textarea, press } = await mount();
  const toolbar = document.querySelector('[data-acp-composer-command-bar]')!;
  const children = [...toolbar.children];
  await press('ArrowUp');
  expect(toolbar.querySelector('.animate-spin, [role="status"], [data-composer-history-status]')).toBeNull();
  expect([...toolbar.children]).toEqual(children);
  await act(async () => finish({ cursor: { generation: 1, messageId: '1', position: 1 }, text: '/review' }));
  expect(textarea.value).toBe('/review');
  expect([...toolbar.children]).toEqual(children);
});
