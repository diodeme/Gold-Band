import {
  CircleStop,
  Loader2,
  Paperclip,
  Play,
  Send,
} from 'lucide-react';
import type {
  ChangeEventHandler,
  ClipboardEventHandler,
  DragEventHandler,
  KeyboardEventHandler,
  ReactNode,
  Ref,
} from 'react';
import { useMemo } from 'react';
import { getRuntimeApi } from '@/api/client';
import { useComposerHistory } from '@/hooks/useComposerHistory';
import type { ComposerHistoryLocator } from '@/lib/composer-history';
import { Trans, useTranslation } from 'react-i18next';
import { useReadOnlyExperience } from '@/components/ReadOnlyExperience';

import { SlashCommandInputTag } from '@/components/conversation/SlashCommandInputTag';
import { SlashCommandMenu } from '@/components/conversation/SlashCommandMenu';
import {
  PromptInput,
  PromptInputAction,
  PromptInputActions,
  PromptInputTextarea,
} from '@/components/prompt-kit/prompt-input';
import { ComposerContextArea } from '@/components/shared/ComposerContextArea';
import { Button } from '@/components/ui/button';
import type { AttachmentItem } from '@/lib/attachment-service';
import type { ComposerQuote } from '@/lib/composer-context';
import type { AcpCommandItemVm } from '@/types';
import { cn } from '@/lib/utils';
import { ACP_SESSION_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';

export interface AcpConversationComposerProps {
  historyLocator?: ComposerHistoryLocator | null;
  prompt: string;
  onPromptChange: (value: string) => void;
  onSubmit: () => void;
  sending: boolean;
  attachments: AttachmentItem[];
  quotes: readonly ComposerQuote[];
  contextError: string | null;
  onRemoveQuote: (id: string) => void;
  onRemoveAttachment: (id: string) => void;
  onPreviewAttachment: (item: AttachmentItem) => void;
  onClearAttachments: () => void;
  fileError: string | null;
  slashCommands: readonly AcpCommandItemVm[];
  slashMenuOpen: boolean;
  slashMenuActiveIndex: number;
  onSlashMenuActiveIndexChange: (index: number) => void;
  onSlashMenuDismiss: () => void;
  onSlashMenuSelect: (index: number) => void;
  textareaRef: Ref<HTMLTextAreaElement>;
  committedSlashCommand?: {
    prefix: string;
    description: string;
  } | null;
  placeholder: string;
  inputDisabled: boolean;
  onTextareaKeyDown: KeyboardEventHandler<HTMLTextAreaElement>;
  onDragEnter: DragEventHandler<HTMLElement>;
  onDragOver: DragEventHandler<HTMLElement>;
  onDrop: DragEventHandler<HTMLElement>;
  onPaste: ClipboardEventHandler<HTMLTextAreaElement>;
  fileInputRef: Ref<HTMLInputElement>;
  onFilesChange: ChangeEventHandler<HTMLInputElement>;
  onPickFiles: () => void | Promise<void>;
  canStop: boolean;
  stopInProgress: boolean;
  onStop: () => void | Promise<void>;
  canSubmit: boolean;
  sendButtonBusy: boolean;
  showRuntimeContinue: boolean;
  runtimeContinueKind: 'continue-current-attempt' | 'recover-completed-attempt' | null;
  runtimeContinueSubmitting: boolean;
  onRuntimeContinue: () => void | Promise<void>;
  configBar: ReactNode;
  attachedPanelVisible: boolean;
  integratedInfoTab: boolean;
  queueSubmit: boolean;
  supersededSession?: {
    label: string;
    href: string;
    onNavigate: () => void;
  } | null;
}

/**
 * Root-conversation-only ACP composer surface.
 *
 * Agent branches never mount this component. Keeping the whole prompt-kit
 * subtree behind one component boundary makes that read-only contract visible
 * in the DOM and prevents Agent Tabs from paying for input-only rendering.
 */
export function AcpConversationComposer(props: AcpConversationComposerProps) {
  const readOnly = useReadOnlyExperience();
  const { t } = useTranslation();
  if (!readOnly) return <AcpConversationComposerContent {...props} />;
  const noop = () => {};
  return <AcpConversationComposerContent {...props}
    prompt="" onPromptChange={noop} onSubmit={noop} sending={false}
    attachments={[]} quotes={[]} contextError={null} fileError={null}
    slashCommands={[]} slashMenuOpen={false} committedSlashCommand={null}
    placeholder={t('demo.inputDisabled')} inputDisabled={true}
    onTextareaKeyDown={noop} onDragEnter={(event) => event.preventDefault()}
    onDragOver={(event) => event.preventDefault()} onDrop={(event) => event.preventDefault()}
    onPaste={(event) => event.preventDefault()} onFilesChange={noop} onPickFiles={noop}
    canStop={false} canSubmit={false} sendButtonBusy={false} showRuntimeContinue={false}
    configBar={null} supersededSession={null} />;
}

function AcpConversationComposerContent({
  historyLocator,
  prompt,
  onPromptChange,
  onSubmit,
  sending,
  attachments,
  quotes,
  contextError,
  onRemoveQuote,
  onRemoveAttachment,
  onPreviewAttachment,
  onClearAttachments,
  fileError,
  slashCommands,
  slashMenuOpen,
  slashMenuActiveIndex,
  onSlashMenuActiveIndexChange,
  onSlashMenuDismiss,
  onSlashMenuSelect,
  textareaRef,
  committedSlashCommand,
  placeholder,
  inputDisabled,
  onTextareaKeyDown,
  onDragEnter,
  onDragOver,
  onDrop,
  onPaste,
  fileInputRef,
  onFilesChange,
  onPickFiles,
  canStop,
  stopInProgress,
  onStop,
  canSubmit,
  sendButtonBusy,
  showRuntimeContinue,
  runtimeContinueKind,
  runtimeContinueSubmitting,
  onRuntimeContinue,
  configBar,
  attachedPanelVisible,
  integratedInfoTab,
  queueSubmit,
  supersededSession,
}: AcpConversationComposerProps) {
  const { t } = useTranslation();
  const historyScope = JSON.stringify(historyLocator ?? null);
  const historySource = useMemo(() => {
    const locator = JSON.parse(historyScope) as ComposerHistoryLocator | null;
    return locator ? {
      list: (query: import('@/lib/composer-history').HistoryQuery) => getRuntimeApi().listComposerHistory(locator, query),
      text: (cursor: import('@/lib/composer-history').HistoryCursor) => getRuntimeApi().getComposerHistoryText(locator, cursor),
    } : null;
  }, [historyScope]);
  const history = useComposerHistory({
    scope: historyScope, source: historySource, input: prompt,
    occupied: attachments.length > 0 || quotes.length > 0,
    disabled: inputDisabled, onChange: onPromptChange,
  });
  const submit = () => { history.reset(); onSubmit(); };
  const continueAndSend = runtimeContinueKind === 'continue-current-attempt' && canSubmit;
  const runtimeContinueLabel = runtimeContinueKind === 'recover-completed-attempt'
    ? t('acp.recoverWorkflow')
    : continueAndSend
      ? t('acp.continueAndSend')
      : t('acp.continueWorkflow');
  const runtimeContinueHint = runtimeContinueKind === 'recover-completed-attempt'
    ? t('acp.recoverWorkflow')
    : continueAndSend
      ? t('acp.continueAndSendHint')
      : t('acp.continueWorkflowHint');
  return (
    <div
      data-conversation-composer="acp"
      data-attachment-dropzone="true"
      onDragEnter={inputDisabled ? undefined : onDragEnter}
      onDragOver={inputDisabled ? undefined : onDragOver}
      onDrop={inputDisabled ? undefined : onDrop}
    >
      {fileError ? (
        <div className="mb-2 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
          {fileError}
        </div>
      ) : null}
      <SlashCommandMenu
        open={slashMenuOpen && !history.browsing}
        commands={slashCommands}
        activeIndex={slashMenuActiveIndex}
        onActiveIndexChange={onSlashMenuActiveIndexChange}
        onDismiss={onSlashMenuDismiss}
        onSelect={onSlashMenuSelect}
      >
        <PromptInput
          value={prompt}
          onValueChange={history.onChange}
          onSubmit={submit}
          isLoading={sending}
          maxHeight={320}
          className={cn(
            'bg-card !shadow-none transition-colors',
            ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName,
            ACP_SESSION_COMPOSER_LAYOUT.promptInputClassName,
            attachedPanelVisible ? 'rounded-t-none rounded-b-2xl' : 'rounded-2xl',
            integratedInfoTab && !attachedPanelVisible && 'rounded-tl-none',
          )}
        >
          <ComposerContextArea
            quotes={quotes}
            attachments={attachments}
            error={contextError}
            onRemoveQuote={onRemoveQuote}
            onRemoveAttachment={onRemoveAttachment}
            onPreviewAttachment={onPreviewAttachment}
          />
          {supersededSession ? (
            <div
              className="flex min-h-12 items-center px-3 py-2 text-sm leading-6 text-muted-foreground"
              role="status"
              data-acp-session-superseded="true"
            >
              <Trans
                i18nKey="acp.sessionSuperseded"
                values={{ target: supersededSession.label }}
                components={{
                  attempt: (
                    <a
                      href={supersededSession.href}
                      className="rounded-sm text-xs font-medium text-link underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                      onClick={(event) => {
                        if (
                          event.button !== 0 ||
                          event.metaKey ||
                          event.ctrlKey ||
                          event.shiftKey ||
                          event.altKey
                        ) return;
                        event.preventDefault();
                        supersededSession.onNavigate();
                      }}
                    />
                  ),
                }}
              />
            </div>
          ) : (
            <PromptInputTextarea
              ref={textareaRef}
              className={ACP_SESSION_COMPOSER_LAYOUT.textareaClassName}
              valuePrefix={history.browsing ? undefined : committedSlashCommand?.prefix}
              leadingAdornment={committedSlashCommand && !history.browsing ? (
                <SlashCommandInputTag
                  prefix={committedSlashCommand.prefix}
                  description={committedSlashCommand.description}
                />
              ) : null}
              placeholder={placeholder}
              textareaDisabled={inputDisabled}
              onKeyDown={(event) => {
                const composing = history.isComposing() || event.nativeEvent.isComposing || event.keyCode === 229;
                const modified = event.altKey || event.ctrlKey || event.metaKey || event.shiftKey;
                if (!history.browsing && !composing && !modified) onTextareaKeyDown(event);
                history.onKeyDown(event);
              }}
              onCompositionStart={history.onCompositionStart}
              onCompositionEnd={history.onCompositionEnd}
              onDragEnter={inputDisabled ? undefined : onDragEnter}
              onDragOver={inputDisabled ? undefined : onDragOver}
              onDrop={inputDisabled ? undefined : onDrop}
              onPaste={inputDisabled ? undefined : onPaste}
            />
          )}
          {history.error ? <div role="alert" className="px-2.5 text-xs text-destructive">{t('acp.composerHistoryError')}</div> : null}
          <div className={ACP_SESSION_COMPOSER_LAYOUT.commandBarClassName} data-acp-composer-command-bar="true">
            <div className={ACP_SESSION_COMPOSER_LAYOUT.leadingActionsClassName}>
              <input
                ref={fileInputRef}
                type="file"
                multiple
                className="hidden"
                onChange={onFilesChange}
              />
              <PromptInputAction tooltip={t('acp.attachHint')}>
                <Button
                  className="size-7 rounded-full"
                  size="icon"
                  variant="ghost"
                  disabled={inputDisabled}
                  aria-label={t('acp.attachHint')}
                  onClick={() => { void onPickFiles(); }}
                >
                  <Paperclip className="size-3.5" />
                </Button>
              </PromptInputAction>
              <div className="min-w-0 flex-1">{configBar}</div>
            </div>
            <PromptInputActions className={ACP_SESSION_COMPOSER_LAYOUT.trailingActionsClassName}>
              {canStop ? (
                <PromptInputAction tooltip={t('acp.stopHint')}>
                  <Button
                    className={ACP_SESSION_COMPOSER_LAYOUT.actionButtonClassName}
                    size="sm"
                    variant="secondary"
                    disabled={stopInProgress}
                    onClick={() => { void onStop(); }}
                  >
                    {stopInProgress ? (
                      <Loader2 className="size-3.5 animate-spin" style={{ willChange: 'transform' }} />
                    ) : (
                      <CircleStop className="size-3.5" />
                    )}
                    {stopInProgress ? t('acp.stopping') : t('acp.stop')}
                  </Button>
                </PromptInputAction>
              ) : null}
              {showRuntimeContinue ? (
                <PromptInputAction tooltip={runtimeContinueHint}>
                  <Button
                    type="button"
                    className={ACP_SESSION_COMPOSER_LAYOUT.actionButtonClassName}
                    size="sm"
                    variant="secondary"
                    disabled={runtimeContinueSubmitting}
                    aria-label={runtimeContinueHint}
                    onClick={() => { history.reset(); void onRuntimeContinue(); }}
                    data-acp-continue-workflow="true"
                  >
                    {runtimeContinueSubmitting ? (
                      <Loader2 className="size-3.5 animate-spin" style={{ willChange: 'transform' }} />
                    ) : (
                      <Play className="size-3.5" />
                    )}
                    {runtimeContinueSubmitting
                      ? t(runtimeContinueKind === 'recover-completed-attempt' ? 'acp.recoverWorkflowStarting' : 'acp.continueWorkflowStarting')
                      : runtimeContinueLabel}
                  </Button>
                </PromptInputAction>
              ) : null}
              <PromptInputAction tooltip={queueSubmit ? t('acp.promptQueue.enqueue') : t('acp.sendMessage')}>
                <Button
                  className={ACP_SESSION_COMPOSER_LAYOUT.actionButtonClassName}
                  size="sm"
                  disabled={!canSubmit}
                  aria-label={queueSubmit ? t('acp.promptQueue.enqueue') : t('acp.sendMessage')}
                  onClick={submit}
                  data-acp-send="true"
                >
                  {sendButtonBusy ? (
                    <Loader2 className="size-3.5 animate-spin" style={{ willChange: 'transform' }} />
                  ) : (
                    <Send className="size-3.5" />
                  )}
                  {queueSubmit ? t('acp.promptQueue.enqueue') : t('acp.send')}
                </Button>
              </PromptInputAction>
            </PromptInputActions>
          </div>
        </PromptInput>
      </SlashCommandMenu>
    </div>
  );
}
