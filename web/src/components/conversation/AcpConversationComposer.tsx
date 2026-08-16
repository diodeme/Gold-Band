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
import { useTranslation } from 'react-i18next';

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
}

/**
 * Root-conversation-only ACP composer surface.
 *
 * Agent branches never mount this component. Keeping the whole prompt-kit
 * subtree behind one component boundary makes that read-only contract visible
 * in the DOM and prevents Agent Tabs from paying for input-only rendering.
 */
export function AcpConversationComposer({
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
}: AcpConversationComposerProps) {
  const { t } = useTranslation();
  return (
    <div
      data-conversation-composer="acp"
      data-attachment-dropzone="true"
      onDragEnter={onDragEnter}
      onDragOver={onDragOver}
      onDrop={onDrop}
    >
      {fileError ? (
        <div className="mb-2 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-xs text-destructive">
          {fileError}
        </div>
      ) : null}
      <SlashCommandMenu
        open={slashMenuOpen}
        commands={slashCommands}
        activeIndex={slashMenuActiveIndex}
        onActiveIndexChange={onSlashMenuActiveIndexChange}
        onDismiss={onSlashMenuDismiss}
        onSelect={onSlashMenuSelect}
      >
        <PromptInput
          value={prompt}
          onValueChange={onPromptChange}
          onSubmit={onSubmit}
          isLoading={sending}
          maxHeight={320}
          className={cn(
            'bg-card !shadow-none transition-colors',
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
          <PromptInputTextarea
            ref={textareaRef}
            className="min-h-12 text-sm leading-6 text-foreground placeholder:text-muted-foreground"
            userResizable
            valuePrefix={committedSlashCommand?.prefix}
            leadingAdornment={committedSlashCommand ? (
              <SlashCommandInputTag
                prefix={committedSlashCommand.prefix}
                description={committedSlashCommand.description}
              />
            ) : null}
            placeholder={placeholder}
            textareaDisabled={inputDisabled}
            onKeyDown={onTextareaKeyDown}
            onDragEnter={onDragEnter}
            onDragOver={onDragOver}
            onDrop={onDrop}
            onPaste={onPaste}
          />
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
                <PromptInputAction tooltip={t(runtimeContinueKind === 'recover-completed-attempt' ? 'acp.recoverWorkflow' : 'acp.continueWorkflow')}>
                  <Button
                    type="button"
                    className={ACP_SESSION_COMPOSER_LAYOUT.actionButtonClassName}
                    size="sm"
                    variant="secondary"
                    disabled={runtimeContinueSubmitting}
                    onClick={() => { void onRuntimeContinue(); }}
                    data-acp-continue-workflow="true"
                  >
                    {runtimeContinueSubmitting ? (
                      <Loader2 className="size-3.5 animate-spin" style={{ willChange: 'transform' }} />
                    ) : (
                      <Play className="size-3.5" />
                    )}
                    {runtimeContinueSubmitting
                      ? t(runtimeContinueKind === 'recover-completed-attempt' ? 'acp.recoverWorkflowStarting' : 'acp.continueWorkflowStarting')
                      : t(runtimeContinueKind === 'recover-completed-attempt' ? 'acp.recoverWorkflow' : 'acp.continueWorkflow')}
                  </Button>
                </PromptInputAction>
              ) : null}
              <PromptInputAction tooltip={queueSubmit ? t('acp.promptQueue.enqueue') : t('acp.send')}>
                <Button
                  className={ACP_SESSION_COMPOSER_LAYOUT.actionButtonClassName}
                  size="sm"
                  disabled={!canSubmit}
                  onClick={onSubmit}
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
