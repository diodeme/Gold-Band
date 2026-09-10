import { useLayoutEffect, useRef } from 'react';
import index from './media/checkpoints/index.json';
import { cameraTransform, type Frame } from './timeline';
import type { ChapterId, Language, SiteTheme } from './content';

const images = import.meta.glob('./media/checkpoints/*.png', { eager: true, query: '?url', import: 'default' }) as Record<string, string>;
type Entry = { width: number; height: number; desktop: Frame; mobile: Frame; at: number };
const entries = index as Record<string, Record<string, Entry>>;
export function checkpointPoster(language: Language, chapter: ChapterId, theme: SiteTheme, checkpoint: string) {
  const name = `${language}-${chapter}${theme === 'light' ? '-light' : ''}`;
  const entry = entries[name]?.[checkpoint];
  const src = images[`./media/checkpoints/${name}-${checkpoint}.png`];
  return entry && src ? { ...entry, src } : undefined;
}
export function CheckpointPoster({ language, chapter, theme, checkpoint, mobile }: { language: Language; chapter: ChapterId; theme: SiteTheme; checkpoint: string; mobile: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const image = useRef<HTMLImageElement>(null);
  const entry = checkpointPoster(language, chapter, theme, checkpoint);
  useLayoutEffect(() => {
    if (!entry || !host.current || !image.current) return;
    const container = host.current;
    const picture = image.current;
    const resize = () => { picture.style.transform = cameraTransform(mobile ? entry.mobile : entry.desktop, entry, { width: container.clientWidth, height: container.clientHeight }); };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(container);
    return () => observer.disconnect();
  }, [entry?.src, mobile]);
  return entry ? <div ref={host} className="checkpoint-poster" data-checkpoint-poster={checkpoint} data-poster-theme={theme}><img ref={image} src={entry.src} width={entry.width} height={entry.height} alt="" style={{ width: entry.width, height: entry.height }} /></div> : null;
}
