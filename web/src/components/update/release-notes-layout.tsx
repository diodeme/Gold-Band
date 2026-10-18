import type React from 'react';
import { DialogContent } from '@/components/ui/dialog';
import { Markdown, MarkdownImagePreviewProvider } from '@/components/prompt-kit/markdown';

/** Shared shell of the update and release-notes dialogs: fixed header/footer around scrollable notes. */
export function ReleaseNotesDialogContent({ children }: { children: React.ReactNode }) {
  return (
    <DialogContent
      // Keep focus on the dialog itself so the first focusable control in the notes (e.g. a code copy button) does not pop its tooltip.
      onOpenAutoFocus={(event) => {
        event.preventDefault();
        (event.currentTarget as HTMLElement).focus();
      }}
      className="flex max-h-[min(88vh,56rem)] w-[min(92vw,50rem)] flex-col gap-0 p-0 sm:max-w-[min(92vw,50rem)]"
    >
      {children}
    </DialogContent>
  );
}

/** Release notes are authored for GitHub Release bodies, so they render with the matching Markdown flavor. */
export function ReleaseNotesBody({ notes, emptyText }: { notes: string | null | undefined; emptyText: React.ReactNode }) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-4 text-sm" data-update-dialog-notes="true">
      {notes ? (
        <MarkdownImagePreviewProvider>
          <Markdown flavor="github-release">{notes}</Markdown>
        </MarkdownImagePreviewProvider>
      ) : (
        <p className="text-muted-foreground">{emptyText}</p>
      )}
    </div>
  );
}
