import { useEffect, useRef, useState } from 'react';
import { Replayer } from 'rrweb';
import type { eventWithTime } from '@rrweb/types';
import { Loader2, Pause, Play, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { copy, type ChapterId, type Language } from './content';
import { cameraAt, ReplayController, type ReplayView } from './replay-controller';
import { loadSceneEvents, loadSceneManifest } from './replay-assets';
import { MOBILE_REPLAY_WIDTH, playbackPosition, selectSceneAsset, type SceneAsset } from './replay-model';
import 'rrweb/dist/style.css';

export function createSceneEngine(data: unknown, source: SceneAsset, root: HTMLDivElement) {
  // Virtual fast-forward drops reused nodes when these scenes return to chat.
  return new Replayer(data as eventWithTime[], { root, showWarning: false, showDebug: false, skipInactive: false, mouseTail: false, UNSAFE_replayCanvas: false, useVirtualDom: source.scene !== 'before' && source.scene !== 'personalize',
    insertStyleRules: source.scene === 'before' ? ['html.rrweb-paused :is([data-state="open"], [data-state="delayed-open"], [data-state="instant-open"]), html.rrweb-paused :is([data-state="open"], [data-state="delayed-open"], [data-state="instant-open"]) * { animation: none !important; }']
      : source.scene === 'after' ? ['html.rrweb-paused :is([data-slot="sheet-content"], [data-slot="sheet-overlay"])[data-state="open"] { animation: none !important; }']
      : source.scene === 'personalize' ? ['html.rrweb-paused [data-slot="popover-content"][data-state="open"] { animation: none !important; }'] : [],
  });
}
type Props = { language: Language; chapter: ChapterId; autoPlay: boolean; theme?: 'dark' | 'light' };
export default function Replay(props: Props) {
  return <SceneReplay {...props} />;
}
function SceneReplay({ language, chapter, autoPlay, theme = 'dark' }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const controller = useRef<ReplayController | null>(null);
  const [view, setView] = useState<ReplayView>({ status: 'loading', poster: '' });
  const [retry, setRetry] = useState(0);
  const [mobile, setMobile] = useState(false);
  const [manifest, setManifest] = useState<Awaited<ReturnType<typeof loadSceneManifest>> | null>(null);
  const t = copy[language];
  useEffect(() => {
    const root = stage.current!;
    const container = host.current!;
    let size = { width: container.clientWidth, height: container.clientHeight };
    setMobile(size.width < MOBILE_REPLAY_WIDTH);
    const motion = matchMedia('(prefers-reduced-motion: reduce)');
    const replay = new ReplayController({
      load: loadSceneEvents,
      create: (data, source) => createSceneEngine(data, source, root),
      requestFrame: callback => window.requestAnimationFrame(callback), cancelFrame: id => window.cancelAnimationFrame(id),
      paint(source, rawMs, reduced) {
        root.style.width = `${source.width}px`; root.style.height = `${source.height}px`;
        const camera = cameraAt(source, rawMs, size.width, size.height, size.width < MOBILE_REPLAY_WIDTH, reduced);
        root.style.transform = `translate(${camera.x}px, ${camera.y}px) scale(${camera.scale})`;
        container.dataset.rawMs = String(rawMs);
        container.dataset.step = playbackPosition(source, rawMs).stepId;
        container.dataset.language = source.language; container.dataset.theme = source.theme;
      },
      changed: setView,
    });
    controller.current = replay;
    replay.setEnvironment({ hidden: document.hidden, reduced: motion.matches, visible: false });
    replay.setPlaying(autoPlay);
    const visibility = () => replay.setEnvironment({ hidden: document.hidden });
    const reduced = () => replay.setEnvironment({ reduced: motion.matches });
    const intersection = new IntersectionObserver(entries => replay.setEnvironment({ visible: entries[0]?.isIntersecting ?? false }));
    const resize = new ResizeObserver(entries => {
      const rect = entries[0].contentRect;
      size = { width: rect.width, height: rect.height }; replay.paint();
      setMobile(current => current === (rect.width < MOBILE_REPLAY_WIDTH) ? current : rect.width < MOBILE_REPLAY_WIDTH);
    });
    intersection.observe(container); resize.observe(container);
    document.addEventListener('visibilitychange', visibility); motion.addEventListener('change', reduced);
    return () => {
      intersection.disconnect(); resize.disconnect();
      document.removeEventListener('visibilitychange', visibility); motion.removeEventListener('change', reduced);
      replay.dispose(); controller.current = null;
    };
  }, []);
  useEffect(() => {
    const request = new AbortController();
    setManifest(null);
    setView(current => ({ ...current, status: 'loading' }));
    controller.current?.suspend();
    void loadSceneManifest(`${import.meta.env.BASE_URL}media/${chapter === 'during' ? 'workflow' : chapter}/manifest.json`, request.signal).then(manifest => {
      if (request.signal.aborted) return;
      setManifest(manifest);
    }).catch(() => { if (!request.signal.aborted) setView(current => ({ ...current, status: 'error' })); });
    return () => request.abort();
  }, [chapter, retry]);
  useEffect(() => {
    if (!manifest) return;
    const selected = selectSceneAsset(manifest, chapter, language, theme, mobile);
    if (!selected) { controller.current?.suspend(); setView(current => ({ ...current, status: 'error' })); return; }
    void controller.current?.select(selected);
  }, [manifest, chapter, language, theme, mobile]);
  const playing = view.status === 'playing';
  const label = playing ? (language === 'zh' ? '暂停演示' : 'Pause demo') : t.play;
  return <div ref={host} className="recording scene-recording" data-player-state={view.status}>
    <div ref={stage} className="replay-stage" aria-hidden="true" />
    {view.poster && (view.status === 'loading' || view.status === 'error') && <img className="replay-poster" src={view.poster} alt="" />}
    {(view.status === 'loading' || view.status === 'error') ? <div className="media-status" role="status">
      {view.status === 'loading' ? <><Loader2 className="animate-spin" />{t.loading}</> : <><span>{t.error}</span><Button variant="secondary" onClick={() => setRetry(value => value + 1)}><RotateCcw />{t.retry}</Button></>}
    </div> : <Tooltip><TooltipTrigger asChild><Button className="replay-toggle" size="icon" variant="secondary" aria-label={label} onClick={() => controller.current?.setPlaying(!playing)}>{playing ? <Pause /> : <Play />}</Button></TooltipTrigger><TooltipContent>{label}</TooltipContent></Tooltip>}
  </div>;
}
