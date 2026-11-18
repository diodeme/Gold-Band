import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';

/**
 * Fullscreen overlay for workflow graph inspection. The embedded graph keeps
 * its own zoom/direction controls; node interactions close the overlay before
 * navigating so the target view is not hidden behind the dialog.
 */
export function GraphFullscreenDialog({
  open,
  onOpenChange,
  title,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        // The graph fullscreen is an opaque work surface, not a floating
        // card. The dialog material recipe paints a translucent texture
        // (background-image) plus a backdrop blur; covering the whole
        // viewport, it blends the dark overlay behind itself — a dark,
        // grid-textured backdrop regardless of the active color scheme.
        // Inline styles override the recipe deterministically: important
        // utility classes are not reliably generated for every tool, so
        // `!bg-none` silently never shipped and the bug survived a
        // class-based fix attempt. The nested graph fills this surface: the
        // embedding blueprint card keeps its fixed height, the fullscreen
        // instance (allowFullscreen=false) switches to h-full.
        style={{
          backgroundColor: 'var(--background)',
          backgroundImage: 'none',
          backdropFilter: 'none',
        }}
        // The default zoom-in-95 open animation frames this viewport-sized
        // layer against the dark overlay (black edges during the animation,
        // amplified by slow software rendering); a plain fade keeps the open
        // transition free of the black flash.
        className="flex h-screen w-screen max-w-none flex-col gap-0 overflow-hidden rounded-none border-0 p-0 sm:max-w-none data-[state=open]:zoom-in-100 data-[state=closed]:zoom-out-100 [&>button]:z-10"
        aria-describedby={undefined}
      >
        <DialogHeader className="sr-only">
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{title}</DialogDescription>
        </DialogHeader>
        <div className="min-h-0 flex-1" data-workflow-graph-fullscreen="true">
          {open ? children : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}
