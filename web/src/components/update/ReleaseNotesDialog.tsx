import { useEffect, useState } from 'react';
import { ExternalLink } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { openExternalUrl } from '@/api';
import { Button } from '@/components/ui/button';
import { Dialog, DialogClose, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { ReleaseNotesBody, ReleaseNotesDialogContent } from './release-notes-layout';
import { loadReleaseNotes, releaseNotesVersion } from './release-notes-source';

type ReleaseNotesState =
  | { status: 'loading' }
  | { status: 'ready'; locale: string; notes: string | null }
  | { status: 'error'; locale: string };

interface ReleaseNotesDialogProps {
  open: boolean;
  appName: string;
  /** Channel-configured page listing all releases; the "more" action is hidden when empty. */
  releaseNotesUrl: string;
  onOpenChange: (open: boolean) => void;
}

export function ReleaseNotesDialog({ open, appName, releaseNotesUrl, onOpenChange }: ReleaseNotesDialogProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.language;
  const [state, setState] = useState<ReleaseNotesState>({ status: 'loading' });
  const current: ReleaseNotesState = state.status !== 'loading' && state.locale !== locale ? { status: 'loading' } : state;

  useEffect(() => {
    if (!open) return undefined;
    let active = true;
    loadReleaseNotes(locale).then(
      (notes) => { if (active) setState({ status: 'ready', locale, notes }); },
      () => { if (active) setState({ status: 'error', locale }); },
    );
    return () => { active = false; };
  }, [open, locale]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <ReleaseNotesDialogContent>
        <DialogHeader className="shrink-0 px-6 pt-5 pb-3">
          <DialogTitle>{t('settings.updater.releaseNotes.title', { appName, version: releaseNotesVersion })}</DialogTitle>
          <DialogDescription className="sr-only">{t('common.releaseNotes')}</DialogDescription>
        </DialogHeader>
        {current.status === 'ready' ? (
          <ReleaseNotesBody notes={current.notes} emptyText={t('settings.updater.releaseNotes.empty')} />
        ) : (
          <ReleaseNotesBody
            notes={null}
            emptyText={current.status === 'loading' ? t('common.loading') : t('settings.updater.releaseNotes.loadFailed')}
          />
        )}
        <DialogFooter className="shrink-0 gap-2 px-6 pt-3 pb-5">
          {releaseNotesUrl ? (
            <Button variant="outline" onClick={() => void openExternalUrl(releaseNotesUrl)}>
              <ExternalLink className="size-4" />
              {t('settings.updater.releaseNotes.more')}
            </Button>
          ) : null}
          <DialogClose asChild>
            <Button>{t('common.close')}</Button>
          </DialogClose>
        </DialogFooter>
      </ReleaseNotesDialogContent>
    </Dialog>
  );
}
