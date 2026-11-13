import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';
import type { ComposerResizeController } from '@/hooks/use-composer-resize';

/**
 * Top-edge drag handle that pins the composer input height. The handle sits on
 * the composer card's top border; dragging up grows the input, dragging back
 * to the bottom threshold and releasing returns to autosize.
 */
export function ComposerResizeHandle({ resize, className }: {
  resize: ComposerResizeController;
  className?: string;
}) {
  const { t } = useTranslation();
  return (
    <div
      role="separator"
      aria-orientation="horizontal"
      aria-label={t('composer.resizeHandle')}
      title={t('composer.resizeHandle')}
      tabIndex={0}
      data-composer-resize-handle="true"
      onPointerDown={resize.gripPointerDown}
      onKeyDown={resize.gripKeyDown}
      className={cn(
        'group/handle absolute -top-[7px] left-1/2 z-20 flex h-3.5 w-16 -translate-x-1/2 cursor-ns-resize touch-none items-center justify-center rounded-full outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1',
        className,
      )}
    >
      <span
        aria-hidden="true"
        className={cn(
          'h-[3px] w-10 rounded-full bg-border transition-colors',
          'group-hover/handle:bg-muted-foreground/60',
          'group-hover/composer:group-hover/handle:bg-muted-foreground/60',
          resize.isResizing && 'bg-muted-foreground/70',
        )}
      />
    </div>
  );
}
