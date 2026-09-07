import { useEffect, useRef, useState } from 'react';
import Player from 'rrweb-player';
import { Loader2, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { CAPTURE, copy, mediaPath, type ChapterId, type Language } from './content';
import type { Recording } from '../../web/rrweb-demo/recording';
import 'rrweb-player/dist/style.css';

type PlayerInstance = Player & { $set(props: { width: number; height: number; maxScale: number }): void; $destroy(): void };
export default function Replay({ language, chapter, autoPlay }: { language: Language; chapter: ChapterId; autoPlay: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<'loading' | 'ready' | 'error'>('loading');
  const [retry, setRetry] = useState(0);
  const t = copy[language];
  useEffect(() => {
    const controller = new AbortController();
    let player: PlayerInstance | undefined;
    let observer: ResizeObserver | undefined;
    let resizeFrame = 0;
    const scheduleResize = () => {
      cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(() => {
        if (!host.current || !player) return;
        const fullscreen = document.fullscreenElement;
        const expanded = fullscreen && host.current.contains(fullscreen);
        const width = expanded ? fullscreen.clientWidth : host.current.clientWidth;
        const height = expanded ? Math.max(1, fullscreen.clientHeight - 80) : Math.round(width * CAPTURE.height / CAPTURE.width);
        player.$set({ width, height, maxScale: Math.min(width / CAPTURE.width, height / CAPTURE.height) });
        player.triggerResize();
      });
    };
    setState('loading');
    const pauseHidden = () => { if (document.hidden) player?.pause(); };
    document.addEventListener('visibilitychange', pauseHidden);
    document.addEventListener('fullscreenchange', scheduleResize);
    void (async () => {
      try {
        const response = await fetch(mediaPath(language, chapter, 'json'), { signal: controller.signal });
        if (!response.ok) throw new Error('recording-unavailable');
        const recording = await response.json() as Recording;
        if (!Array.isArray(recording.events) || recording.events.length < 2) throw new Error('recording-invalid');
        if (controller.signal.aborted || !host.current) return;
        const size = () => {
          const width = host.current!.clientWidth;
          return { width, height: Math.round(width * CAPTURE.height / CAPTURE.width), maxScale: width / CAPTURE.width };
        };
        player = new Player({ target: host.current, props: { events: recording.events, ...size(), autoPlay: !document.hidden && autoPlay, skipInactive: false, showController: true, speedOption: [1, 2, 4, 8] } }) as PlayerInstance;
        const controls = host.current.querySelectorAll<HTMLButtonElement>('.rr-controller button');
        controls[0]?.setAttribute('aria-label', language === 'zh' ? '播放或暂停' : 'Play or pause');
        controls[controls.length - 1]?.setAttribute('aria-label', language === 'zh' ? '全屏' : 'Fullscreen');
        const skip = host.current.querySelector('.rr-controller .label');
        if (skip) skip.textContent = language === 'zh' ? '跳过空闲' : 'Skip inactive';
        host.current.querySelector('input[type=checkbox]')?.setAttribute('aria-label', language === 'zh' ? '跳过空闲' : 'Skip inactive');
        observer = new ResizeObserver(scheduleResize);
        observer.observe(host.current);
        setState('ready');
      } catch { if (!controller.signal.aborted) setState('error'); }
    })();
    return () => {
      controller.abort(); observer?.disconnect(); cancelAnimationFrame(resizeFrame);
      document.removeEventListener('visibilitychange', pauseHidden);
      document.removeEventListener('fullscreenchange', scheduleResize);
      player?.pause(); player?.getReplayer().destroy(); player?.$destroy();
    };
  }, [language, chapter, retry, autoPlay]);
  return <div className="recording" data-player-state={state}>
    {state !== 'ready' && <div className="media-status" role="status">{state === 'loading' ? <><Loader2 className="animate-spin" />{t.loading}</> : <><span>{t.error}</span><Button variant="secondary" onClick={() => setRetry(value => value + 1)}><RotateCcw />{t.retry}</Button></>}</div>}
    <div ref={host} className="replay-host" />
  </div>;
}
