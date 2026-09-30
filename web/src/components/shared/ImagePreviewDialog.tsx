import { useTranslation } from 'react-i18next';
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog';

export interface ImagePreviewTarget {
  src: string;
  alt: string;
}

export function ImagePreviewDialog({ image, onClose }: { image: ImagePreviewTarget | null; onClose: () => void }) {
  const { t } = useTranslation();
  return (
    <Dialog open={!!image} onOpenChange={(open) => { if (!open) onClose(); }}>
      <DialogContent
        showCloseButton={false}
        overlayClassName="bg-black/70"
        className="!w-auto !max-w-[calc(100vw-4rem)] !gap-0 border-0 bg-transparent p-0 shadow-none sm:!max-w-[calc(100vw-4rem)]"
      >
        <DialogTitle className="sr-only">{image?.alt || t('common.imagePreview')}</DialogTitle>
        {image ? (
          <img
            src={image.src}
            alt={image.alt}
            draggable={false}
            className="block max-h-[calc(100vh-4rem)] max-w-[calc(100vw-4rem)] object-contain"
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
