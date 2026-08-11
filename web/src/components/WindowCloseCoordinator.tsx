import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { completeMainWindowClose, resolveAppExit, subscribeAppExitRequested } from '@/api';
import { isTauriRuntime } from '@/api/shared';
import type { DesktopPlatform } from '@/types';
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
  action: 'close' | 'exit';
  onOpenChange(open: boolean): void;
  onDecision(decision: WindowCloseSaveFailureDecision): void;
}

export function WindowCloseSaveFailureDialog({
  open,
  action,
  onOpenChange,
  onDecision,
}: WindowCloseSaveFailureDialogProps) {
  const { t } = useTranslation();

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('common.windowCloseSaveFailedTitle')}</AlertDialogTitle>
          <AlertDialogDescription>
            {t(action === 'close' ? 'common.windowCloseSaveFailedDescriptionClose' : 'common.windowCloseSaveFailedDescription')}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogAction variant="outline" onClick={() => onDecision('retry')}>
            {t('common.retrySave')}
          </AlertDialogAction>
          <AlertDialogCancel onClick={() => onDecision('cancel')}>
            {t('common.cancelClose')}
          </AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => onDecision('discard')}>
            {t(action === 'close' ? 'common.discardChangesAndClose' : 'common.discardChangesAndExit')}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function WindowCloseCoordinator({ platform }: { platform?: DesktopPlatform | null }) {
  const [saveFailureDialogOpen, setSaveFailureDialogOpen] = useState(false);
  const [pendingAction, setPendingAction] = useState<'close' | 'exit'>('exit');
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
    let unlistenClose: (() => void) | null = null;
    let unlistenExit: (() => void) | null = null;
    const handleCloseRequested = createWindowCloseTransaction({
      flushPendingChanges: () => fileContentStore.flushAll(),
      requestSaveFailureDecision,
      completeClose: completeMainWindowClose,
    });
    void appWindow.onCloseRequested((event) => {
      setPendingAction(platform === 'macos' ? 'close' : 'exit');
      return handleCloseRequested(event);
    }).then((dispose) => {
      if (disposed) dispose();
      else unlistenClose = dispose;
    });
    void subscribeAppExitRequested(async ({ requestId }) => {
      setPendingAction('exit');
      const proceed = await handleCloseRequested.prepareToClose();
      await resolveAppExit({ requestId, decision: proceed ? 'proceed' : 'cancel' }).catch(() => {});
    }).then((dispose) => {
      if (disposed) dispose();
      else unlistenExit = dispose;
    });
    return () => {
      disposed = true;
      unlistenClose?.();
      unlistenExit?.();
      decisionResolverRef.current?.('cancel');
      decisionResolverRef.current = null;
    };
  }, [platform, requestSaveFailureDecision]);

  useEffect(() => {
    setPendingAction(platform === 'macos' ? 'close' : 'exit');
  }, [platform]);

  return (
    <WindowCloseSaveFailureDialog
      open={saveFailureDialogOpen}
      action={pendingAction}
      onOpenChange={(open) => {
        if (!open) resolveSaveFailureDecision('cancel');
      }}
      onDecision={resolveSaveFailureDecision}
    />
  );
}
