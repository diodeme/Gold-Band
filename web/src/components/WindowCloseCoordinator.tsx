import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { prepareAppExit } from '@/api';
import { isTauriRuntime } from '@/api/shared';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';
import { fileContentStore } from '@/components/workspace/files/file-content-store';
import {
  createWindowCloseTransaction,
  type WindowCloseSaveFailureDecision,
} from '@/lib/window-close-guard';

interface WindowCloseSaveFailureDialogProps {
  open: boolean;
  onOpenChange(open: boolean): void;
  onDecision(decision: WindowCloseSaveFailureDecision): void;
}

export function WindowCloseSaveFailureDialog({
  open,
  onOpenChange,
  onDecision,
}: WindowCloseSaveFailureDialogProps) {
  const { t } = useTranslation();

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('common.windowCloseSaveFailedTitle')}</AlertDialogTitle>
          <AlertDialogDescription>{t('common.windowCloseSaveFailedDescription')}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogAction variant="outline" onClick={() => onDecision('retry')}>
            {t('common.retrySave')}
          </AlertDialogAction>
          <AlertDialogCancel onClick={() => onDecision('cancel')}>
            {t('common.cancelClose')}
          </AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => onDecision('discard')}>
            {t('common.discardChangesAndExit')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function WindowCloseCoordinator() {
  const [saveFailureDialogOpen, setSaveFailureDialogOpen] = useState(false);
  const decisionResolverRef = useRef<((decision: WindowCloseSaveFailureDecision) => void) | null>(null);

  const resolveSaveFailureDecision = useCallback((decision: WindowCloseSaveFailureDecision) => {
    const resolve = decisionResolverRef.current;
    decisionResolverRef.current = null;
    setSaveFailureDialogOpen(false);
    resolve?.(decision);
  }, []);

  const requestSaveFailureDecision = useCallback(() => new Promise<WindowCloseSaveFailureDecision>((resolve) => {
    decisionResolverRef.current = resolve;
    setSaveFailureDialogOpen(true);
  }), []);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const appWindow = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | null = null;
    const handleCloseRequested = createWindowCloseTransaction({
      flushPendingChanges: () => fileContentStore.flushAll(),
      requestSaveFailureDecision,
      prepareAppExit,
      destroyWindow: () => appWindow.destroy(),
    });
    void appWindow.onCloseRequested(handleCloseRequested).then((dispose) => {
      if (disposed) dispose();
      else unlisten = dispose;
    });
    return () => {
      disposed = true;
      unlisten?.();
      decisionResolverRef.current?.('cancel');
      decisionResolverRef.current = null;
    };
  }, [requestSaveFailureDecision]);

  return (
    <WindowCloseSaveFailureDialog
      open={saveFailureDialogOpen}
      onOpenChange={(open) => {
        if (!open) resolveSaveFailureDecision('cancel');
      }}
      onDecision={resolveSaveFailureDecision}
    />
  );
}
