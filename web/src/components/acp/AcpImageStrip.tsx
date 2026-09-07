import { memo, useEffect, useRef, useState } from 'react';
import { Image as ImageIcon, RotateCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { ScrollArea, ScrollBar } from '@/components/ui/scroll-area';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { MessageAttachmentPreviewButton } from './MessageAttachmentPreviewButton';
import { acquireAcpImage, acpImageKey, loadAcpOriginalImage, type AcpImageAsset } from '@/lib/acp-image-cache';
import { useOptionalRightWorkspaceCommands, type AcpImageWorkspaceResource } from '@/components/workspace/right-workspace-context';
import { WorkspaceImageCanvas } from '@/components/workspace/files/WorkspaceImageCanvas';
import type { AcpImageRef, TurnFileLocatorVm } from '@/types';

function useAcpImage(locator: TurnFileLocatorVm | null, image: AcpImageRef, thumbnail: boolean, enabled: boolean) {
  const key = locator ? acpImageKey(locator, image, thumbnail) : '';
  const [result, setResult] = useState<{ key: string; asset?: AcpImageAsset; failed?: boolean }>({ key: '' });
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    if (!enabled || !locator) { setResult({ key }); return; }
    let cancelled = false;
    setResult({ key });
    let release: (() => void) | undefined;
    try {
      const lease = acquireAcpImage(locator, image, thumbnail);
      release = lease.release;
      void lease.promise.then((asset) => { if (!cancelled) setResult({ key, asset }); })
        .catch(() => { if (!cancelled) setResult({ key, failed: true }); });
    } catch { setResult({ key, failed: true }); }
    return () => { cancelled = true; release?.(); };
  // The key contains the complete locator and immutable image identity.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled, retry]);
  return { ...(enabled && result.key === key ? result : { key }), retry: () => setRetry((value) => value + 1) };
}

export const AcpImageStrip = memo(function AcpImageStrip({ images, locator }: {
  images: AcpImageRef[]; locator: TurnFileLocatorVm | null;
}) {
  const { t } = useTranslation();
  if (images.length === 0) return null;
  return (
    <ScrollArea type="hover" scrollHideDelay={120} className="my-2 w-full min-w-0 max-w-full" data-acp-image-strip="true">
      <div role="list" aria-label={t('acpImages.title')} className="flex w-max min-w-full gap-2 pb-3">
        {images.map((image, index) => <AcpImageThumbnail key={`${image.eventId}:${image.pointer}:${image.contentHash}`}
          image={image} locator={locator} label={t('acpImages.image', { index: index + 1 })} />)}
      </div>
      <ScrollBar orientation="horizontal" />
    </ScrollArea>
  );
});

function AcpImageThumbnail({ image, locator, label }: { image: AcpImageRef; locator: TurnFileLocatorVm | null; label: string }) {
  const host = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const workspace = useOptionalRightWorkspaceCommands();
  const { t } = useTranslation();
  useEffect(() => {
    if (!host.current || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting));
    observer.observe(host.current);
    return () => observer.disconnect();
  }, []);
  const result = useAcpImage(locator, image, true, visible);
  return (
    <div ref={host} role="listitem" className="relative size-[72px] shrink-0" data-acp-image-thumbnail="true">
      <MessageAttachmentPreviewButton
        attachment={{ name: label, path: image.pointer, type: 'image/png', size: result.asset?.blob.size ?? 0 }}
        imageSource={result.asset?.url ?? null}
        imageAsset={locator ? { name: label, mime: image.mimeType, loadOriginal: () => loadAcpOriginalImage(locator, image) } : undefined}
        onClick={() => {
          if (!locator || !workspace?.scopeKey) return;
          void workspace.openResource({ kind: 'acp-image', key: acpImageKey(locator, image, false),
            scopeKey: workspace.scopeKey, title: label, attention: false, locator, image });
        }}
      />
      {result.failed ? <Tooltip><TooltipTrigger asChild><Button size="icon" variant="secondary"
        className="absolute inset-0 m-auto size-7" aria-label={t('common.retry')} onClick={result.retry}>
        <RotateCw className="size-3.5" />
      </Button></TooltipTrigger><TooltipContent>{t('acpImages.failed')}</TooltipContent></Tooltip> : null}
    </div>
  );
}

export function AcpImageWorkspacePanel({ resource }: { resource: AcpImageWorkspaceResource }) {
  const { t } = useTranslation();
  const result = useAcpImage(resource.locator, resource.image, false, true);
  return <section className="flex min-h-0 flex-1 flex-col" data-acp-image-workspace="true">
    <header className="flex h-9 shrink-0 items-center gap-2 border-b border-border/60 px-3 text-xs">
      <ImageIcon className="size-3.5" /><span className="truncate">{resource.title}</span>
    </header>
    {result.asset ? <WorkspaceImageCanvas src={result.asset.url} alt={resource.title}
      imageActionAsset={{ name: resource.title, mime: result.asset.mimeType, file: result.asset.blob }} />
      : <div className="flex min-h-0 flex-1 items-center justify-center gap-2 text-sm text-muted-foreground">
        {result.failed ? t('acpImages.failed') : t('common.loading')}
        {result.failed ? <Button variant="ghost" size="sm" onClick={result.retry}>{t('common.retry')}</Button> : null}
      </div>}
  </section>;
}
