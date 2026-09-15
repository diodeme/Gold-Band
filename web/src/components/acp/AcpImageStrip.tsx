import { memo, useEffect, useRef, useState, type RefObject } from 'react';
import { Image as ImageIcon, Loader2, RotateCw } from 'lucide-react';
import { getAcpActivityImages } from '@/api';
import { useTranslation } from 'react-i18next';
import { ScrollArea, ScrollBar } from '@/components/ui/scroll-area';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { MessageAttachmentPreviewButton } from './MessageAttachmentPreviewButton';
import { ACP_PROJECTED_IMAGE_LIMIT, acquireAcpImage, acpImageKey, loadAcpOriginalImage, type AcpImageAsset } from '@/lib/acp-image-cache';
import { useOptionalRightWorkspaceCommands, type AcpImageWorkspaceResource } from '@/components/workspace/right-workspace-context';
import { WorkspaceImageCanvas } from '@/components/workspace/files/WorkspaceImageCanvas';
import type { AcpImageRef, TurnFileLocatorVm } from '@/types';

const THUMBNAIL_SIZE = 72;
const THUMBNAIL_GAP = 8;
type PreparedImage = { asset?: AcpImageAsset; failed?: boolean };
type PreparedImages = ReadonlyMap<string, PreparedImage>;

async function decodeThumbnail(asset: AcpImageAsset) {
  const decoded = new Image();
  decoded.src = asset.url;
  await decoded.decode();
  return asset;
}

export function useAcpToolImageReadiness(locator: TurnFileLocatorVm | null, images: AcpImageRef[],
  enabled: boolean, container: RefObject<HTMLDivElement | null>) {
  const key = locator ? JSON.stringify(images.map(image => acpImageKey(locator, image, true))) : '';
  const [prepared, setPrepared] = useState<{ key: string; assets: PreparedImages } | null>(null);
  useEffect(() => {
    setPrepared(null);
    if (!enabled || !locator || images.length === 0) return;
    let cancelled = false;
    const count = Math.max(1, Math.ceil((container.current?.clientWidth ?? 0) / (THUMBNAIL_SIZE + THUMBNAIL_GAP)));
    const releases: Array<() => void> = [];
    const selected = images.slice(0, count);
    const pending = selected.map(async image => {
      const lease = acquireAcpImage(locator, image, true);
      releases.push(lease.release);
      const asset = await lease.promise;
      if (cancelled) return null;
      return decodeThumbnail(asset);
    });
    void Promise.allSettled(pending).then(results => {
      if (cancelled) return;
      const assets = new Map<string, PreparedImage>();
      results.forEach((result, index) => assets.set(acpImageKey(locator, selected[index], true),
        result.status === 'fulfilled' && result.value ? { asset: result.value } : { failed: true }));
      // Failed images remain local retryable tiles; they must not block the tool body.
      setPrepared({ key, assets });
    });
    return () => { cancelled = true; releases.forEach(release => release()); };
  // The key includes the complete locator and every immutable image reference.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled]);
  return { ready: images.length === 0 || !locator || (enabled && prepared?.key === key),
    assets: enabled && prepared?.key === key ? prepared.assets : undefined };
}

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
      void lease.promise.then(async (asset) => {
        if (cancelled) return;
        if (thumbnail) await decodeThumbnail(asset);
        if (!cancelled) setResult({ key, asset });
      })
        .catch(() => { if (!cancelled) setResult({ key, failed: true }); });
    } catch { setResult({ key, failed: true }); }
    return () => { cancelled = true; release?.(); };
  // The key contains the complete locator and immutable image identity.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, enabled, retry]);
  return { ...(enabled && result.key === key ? result : { key }), retry: () => setRetry((value) => value + 1) };
}

export const AcpImageStrip = memo(function AcpImageStrip({ images, locator, prepared }: {
  images: AcpImageRef[]; locator: TurnFileLocatorVm | null; prepared?: PreparedImages;
}) {
  const { t } = useTranslation();
  if (images.length === 0) return null;
  return (
    <ScrollArea type="hover" scrollHideDelay={120} className="my-2 w-full min-w-0 max-w-full" data-acp-image-strip="true">
      <div role="list" aria-label={t('acpImages.title')} className="flex w-max min-w-full pb-3" style={{ gap: THUMBNAIL_GAP }}>
        {images.map((image, index) => <AcpImageThumbnail key={`${image.eventId}:${image.pointer}:${image.contentHash}`}
          image={image} locator={locator} prepared={locator ? prepared?.get(acpImageKey(locator, image, true)) : undefined}
          label={t('acpImages.image', { index: index + 1 })} />)}
      </div>
      <ScrollBar orientation="horizontal" />
    </ScrollArea>
  );
});

export function AcpActivityImageStrip({ locator, start, end, generation }: {
  locator: TurnFileLocatorVm; start: number; end: number; generation?: number;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [retry, setRetry] = useState(0);
  const [result, setResult] = useState<{ key: string; images: AcpImageRef[]; pending: boolean; failed: boolean } | null>(null);
  const progress = useRef<{ key: string; images: AcpImageRef[]; after: string | null;
    generation?: number; complete: boolean } | null>(null);
  const key = JSON.stringify([locator, start, end, generation]);
  const { t } = useTranslation();
  useEffect(() => {
    if (!host.current) return;
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting));
    observer.observe(host.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    if (progress.current?.key !== key) progress.current = { key, images: [], after: null, generation, complete: false };
    const query = progress.current;
    if (query.complete) return;
    const images = query.images;
    setResult({ key, images, pending: true, failed: false });
    void (async () => {
      try {
        do {
          const page = await getAcpActivityImages({ ...locator, start, end, after: query.after, generation: query.generation });
          if (cancelled) return;
          if ((query.generation !== undefined && page.generation !== query.generation)
            || (page.nextCursor !== null && page.nextCursor === query.after)) throw new Error('acp.image-invalid-page');
          query.generation = page.generation;
          images.push(...page.images.slice(0, ACP_PROJECTED_IMAGE_LIMIT - images.length));
          query.after = page.nextCursor;
          query.complete = !query.after || images.length >= ACP_PROJECTED_IMAGE_LIMIT;
          setResult({ key, images: [...images], pending: !query.complete, failed: false });
        } while (!query.complete && !cancelled);
      } catch { if (!cancelled) setResult({ key, images, pending: false, failed: true }); }
    })();
    return () => { cancelled = true; };
  // Scope and query boundaries, rather than object identity, own this read.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, visible, retry]);
  const current = result?.key === key ? result : null;
  return <div ref={host} className="min-h-6">
    <AcpImageStrip locator={locator} images={current?.images ?? []} />
    {current?.pending ? <span role="status" className="flex items-center gap-2 text-xs text-muted-foreground"><Loader2 className="size-3 animate-spin" />{t('common.loading')}</span> : null}
    {current?.failed ? <Button variant="ghost" size="sm" onClick={() => setRetry(value => value + 1)}><RotateCw className="size-3" />{t('common.retry')}</Button> : null}
  </div>;
}

function AcpImageThumbnail({ image, locator, label, prepared }: {
  image: AcpImageRef; locator: TurnFileLocatorVm | null; label: string; prepared?: PreparedImage;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [retried, setRetried] = useState(false);
  const workspace = useOptionalRightWorkspaceCommands();
  const { t } = useTranslation();
  useEffect(() => {
    if (!host.current || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting));
    observer.observe(host.current);
    return () => observer.disconnect();
  }, []);
  const result = useAcpImage(locator, image, true, visible && (!prepared || retried));
  return (
    <div ref={host} role="listitem" className="relative shrink-0" style={{ width: THUMBNAIL_SIZE, height: THUMBNAIL_SIZE }} data-acp-image-thumbnail="true">
      <MessageAttachmentPreviewButton
        attachment={{ name: label, path: image.pointer, type: 'image/png', size: result.asset?.blob.size ?? 0 }}
        imageSource={result.asset?.url ?? prepared?.asset?.url ?? null}
        imageAsset={locator ? { name: label, mime: image.mimeType, loadOriginal: () => loadAcpOriginalImage(locator, image) } : undefined}
        onClick={() => {
          if (!locator || !workspace?.scopeKey) return;
          void workspace.openResource({ kind: 'acp-image', key: acpImageKey(locator, image, false),
            scopeKey: workspace.scopeKey, title: label, attention: false, locator, image });
        }}
      />
      {result.failed || (prepared?.failed && !retried) ? <Tooltip><TooltipTrigger asChild><Button size="icon" variant="secondary"
        className="absolute inset-0 m-auto size-7" aria-label={t('common.retry')} onClick={() => { setRetried(true); result.retry(); }}>
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
