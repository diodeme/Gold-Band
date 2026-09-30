import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Markdown, MarkdownImagePreviewProvider } from '@/components/prompt-kit/markdown';
import type { UpdateStatusVm } from '@/types';
import { UpdateDownloadProgress } from './UpdateDownloadProgress';
import { describeUpdateError, updateActionState } from './update-state';

interface UpdateDialogProps {
  open: boolean;
  status: UpdateStatusVm;
  onOpenChange: (open: boolean) => void;
  onInstall: () => void;
}

export function UpdateDialog({ open, status, onOpenChange, onInstall }: UpdateDialogProps) {
  const { t } = useTranslation();
  const update = status.update ?? null;
  const action = updateActionState(status);
  return (
    <Dialog open={open && update !== null} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[min(88vh,56rem)] w-[min(92vw,50rem)] flex-col gap-0 p-0 sm:max-w-[min(92vw,50rem)]">
        <DialogHeader className="shrink-0 px-6 pt-5 pb-3">
          <DialogTitle>{t('settings.updater.dialog.title', { version: update?.version ?? '' })}</DialogTitle>
          <DialogDescription className="font-mono text-xs">
            {update ? `${update.currentVersion} → ${update.version}` : null}
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-4 text-sm" data-update-dialog-notes="true">
          {update?.notes ? (
            <MarkdownImagePreviewProvider>
              <Markdown>{update.notes}</Markdown>
            </MarkdownImagePreviewProvider>
          ) : (
            <p className="text-muted-foreground">{t('settings.updater.dialog.noNotes')}</p>
          )}
        </div>
        <DialogFooter className="shrink-0 items-center gap-3 px-6 pt-3 pb-5 sm:justify-between">
          <div className="min-w-0 flex-1 text-xs">
            {action === 'downloading' ? <UpdateDownloadProgress className="max-w-96" /> : null}
            {action === 'failed' && status.error ? (
              <p className="break-words text-destructive" role="alert">{describeUpdateError(t, status.error)}</p>
            ) : null}
          </div>
          <div className="flex shrink-0 gap-2">
            <DialogClose asChild>
              <Button variant="outline">{t('common.close')}</Button>
            </DialogClose>
            <Button onClick={onInstall} disabled={action === 'downloading' || action === null}>
              {action === 'downloading' ? <Loader2 className="size-4 animate-spin" /> : null}
              {t(`settings.updater.dialog.action.${action ?? 'available'}`)}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
