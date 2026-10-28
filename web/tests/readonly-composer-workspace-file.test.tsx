/** @vitest-environment jsdom */
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ReadOnlyExperience } from '@/components/ReadOnlyExperience';
import { AcpConversationComposer } from '@/components/conversation/AcpConversationComposer';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { ComposerWorkspaceFileRef } from '@/lib/composer-context';

const file: ComposerWorkspaceFileRef = {
  id: 'default:README.md',
  projectId: 'default',
  relativePath: 'README.md',
  name: 'README.md',
  byteLength: 12,
  mimeType: 'text/markdown',
  canonicalPath: '/default/README.md',
};

describe('read-only conversation composer', () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    vi.stubGlobal('IS_REACT_ACT_ENVIRONMENT', true);
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  it('keeps a workspace file reference visible while input stays disabled', () => {
    const noop = () => {};
    act(() => {
      root.render(
        <TooltipProvider>
          <ReadOnlyExperience.Provider value={true}>
            <AcpConversationComposer
              prompt=""
              onPromptChange={noop}
              onHistoryTextCommit={noop}
              onSubmit={noop}
              sending={false}
              attachments={[]}
              quotes={[]}
              workspaceFiles={[file]}
              contextError={null}
              onRemoveQuote={noop}
              onRemoveWorkspaceFile={noop}
              onOpenWorkspaceFile={noop}
              onRemoveAttachment={noop}
              onPreviewAttachment={noop}
              onClearAttachments={noop}
              fileError={null}
              slashGroups={[]}
              slashMenuOpen={false}
              slashMenuActiveIndex={0}
              onSlashMenuActiveIndexChange={noop}
              onSlashMenuDismiss={noop}
              onSlashMenuSelect={noop}
              textareaRef={{ current: null }}
              placeholder="disabled"
              inputDisabled={false}
              onTextareaKeyDown={noop}
              onDragEnter={noop}
              onDragOver={noop}
              onDrop={noop}
              onPaste={noop}
              fileInputRef={{ current: null }}
              onFilesChange={noop}
              onPickFiles={noop}
              canStop={false}
              stopInProgress={false}
              stopCommandInFlight={false}
              onStop={noop}
              canSubmit={false}
              canSubmitHistory={false}
              sendButtonBusy={false}
              showRuntimeContinue={false}
              runtimeContinueKind={null}
              runtimeContinueSubmitting={false}
              onRuntimeContinue={noop}
              configBar={null}
              queueSubmit={false}
            />
          </ReadOnlyExperience.Provider>
        </TooltipProvider>,
      );
    });
    expect(host.querySelector('[data-composer-workspace-file-chip]')?.textContent).toContain('README.md');
    expect((host.querySelector('textarea') as HTMLTextAreaElement).disabled).toBe(true);
  });
});
