import { useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from '@/components/ui/context-menu';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';
import type { GitFileChangeVm } from '@/types';

export function SourceControlDiscardMenu({ change, disabled, pending, onDiscard, children }: {
  change: GitFileChangeVm;
  disabled: boolean;
  pending: boolean;
  onDiscard: () => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const [confirm, setConfirm] = useState(false);
  return <>
    <ContextMenu>
      <ContextMenuTrigger asChild><div className="min-w-0" aria-busy={pending}>
        {children}
      </div></ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem variant="destructive" disabled={disabled || change.submodule} onSelect={() => setConfirm(true)}>{t('sourceControl.discardChange')}</ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
    <AlertDialog open={confirm} onOpenChange={setConfirm}>
      <AlertDialogContent className="sm:max-w-md">
        <AlertDialogHeader>
          <AlertDialogTitle>{t('sourceControl.discardChange')}</AlertDialogTitle>
          <AlertDialogDescription>{t('sourceControl.discardDescription')}</AlertDialogDescription>
        </AlertDialogHeader>
        <p className="min-w-0 break-all font-mono text-xs">{change.oldPath ? `${change.oldPath} → ${change.path}` : change.path}</p>
        <AlertDialogFooter>
          <AlertDialogCancel>{t('common.cancel')}</AlertDialogCancel>
          <AlertDialogAction variant="destructive" disabled={disabled || change.submodule} onClick={onDiscard}>{t('sourceControl.discardChange')}</AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </>;
}
