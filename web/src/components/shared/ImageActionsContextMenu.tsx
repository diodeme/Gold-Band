import type { ReactElement } from 'react';
import { Copy, Download } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from '@/components/ui/context-menu';
import type { ImageActionsController } from '@/hooks/useImageActions';

export function ImageActionsContextMenu({
  actions,
  children,
  triggerClassName,
}: {
  actions: ImageActionsController;
  children: ReactElement;
  triggerClassName?: string;
}) {
  const { t } = useTranslation();
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild={!triggerClassName} className={triggerClassName}>
        {children}
      </ContextMenuTrigger>
      <ContextMenuContent className="w-40 min-w-40 p-1">
        <ContextMenuItem disabled={actions.pending} onSelect={() => void actions.copyImage()}>
          <Copy className="size-4" />
          {t('workspace.filesPanel.copyImage')}
        </ContextMenuItem>
        <ContextMenuItem disabled={actions.pending} onSelect={() => void actions.saveImage()}>
          <Download className="size-4" />
          {t('workspace.filesPanel.saveImageAs')}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
