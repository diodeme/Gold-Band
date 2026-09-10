import { useEffect, useRef, useState, type MutableRefObject } from 'react';
import { Replayer } from 'rrweb';
import { Loader2, Pause, Play, RotateCcw } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { CAPTURE, copy, mediaPath, type ChapterId, type Language, type SiteTheme } from './content';
import { cameraTransform, checkpointAt, restoreCheckpoint, playbackGate, readRecording, sampleTrack } from './timeline';
import { RECORDING_LIMITS } from '../../web/rrweb-demo/recording';
import { CheckpointPoster } from './CheckpointPoster';
import { REPLAY_OPTIONS, settleReplayAnimations } from './replay-options';
import 'rrweb/dist/style.css';

export type ReplayPositions = Partial<Record<ChapterId, { checkpoint: { id: string; progress: number }; paused: boolean; autoPlay: boolean }>>;
export default function Replay({ language, chapter, autoPlay, mobile = false, theme = 'dark', positions }: { language: Language; chapter: ChapterId; autoPlay: boolean; mobile?: boolean; theme?: SiteTheme; positions?: MutableRefObject<ReplayPositions> }) {
  const host = useRef<HTMLDivElement>(null);
  const localPositions = useRef<ReplayPositions>({});
  const memory = positions ?? localPositions;
  const command = useRef<(paused: boolean) => void>(() => {});
  const [paused, setPaused] = useState(!autoPlay);
  const [state, setState] = useState<'loading' | 'ready' | 'error'>('loading');
  const [retry, setRetry] = useState(0);
  const [posterCheckpoint, setPosterCheckpoint] = useState('establish');
  const t = copy[language];
  useEffect(() => {
    const controller = new AbortController();
    let player: Replayer | undefined;
    let observer: ResizeObserver | undefined;
    let intersection: IntersectionObserver | undefined;
    let frame = 0;
    let visible = false;
    const saved = memory.current[chapter];
    setPosterCheckpoint(saved?.checkpoint.id ?? 'establish');
    let userPaused = saved?.autoPlay === autoPlay ? saved.paused : !autoPlay;
    let remember = () => {};
    let gate: ReturnType<typeof playbackGate> | undefined;
    let paint: () => void = () => {};
    const reduced = matchMedia('(prefers-reduced-motion: reduce)');
    let reducedMotion = reduced.matches;
    const allowed = () => visible && !document.hidden && !userPaused;
    const sync = () => {
      gate?.(allowed());
      cancelAnimationFrame(frame);
      frame = 0;
      if (allowed()) frame = requestAnimationFrame(paint);
    };
    const motionChanged = (event: MediaQueryListEvent) => {
      reducedMotion = event.matches;
      if (reducedMotion) { userPaused = true; setPaused(true); sync(); }
    };
    command.current = value => { userPaused = value; sync(); };
    document.addEventListener('visibilitychange', sync);
    reduced.addEventListener('change', motionChanged);
    setPaused(userPaused);
    setState('loading');
    void (async () => {
      try {
        const response = await fetch(mediaPath(language, chapter, 'json', theme), { signal: controller.signal });
        if (!response.ok) throw { code: 'site.recording-unavailable' };
        // Enforce the budget before JSON parsing and DOM allocation.
        const reader = response.body!.getReader();
        const chunks: Uint8Array[] = [];
        let bytes = 0;
        while (true) {
          const item = await reader.read();
          if (item.done) break;
          bytes += item.value.byteLength;
          if (bytes > RECORDING_LIMITS.bytes) { await reader.cancel(); throw { code: 'site.recording-budget' }; }
          chunks.push(item.value);
        }
        const data = new Uint8Array(bytes);
        let offset = 0;
        for (const chunk of chunks) { data.set(chunk, offset); offset += chunk.byteLength; }
        const { recording, shots, duration } = readRecording(JSON.parse(new TextDecoder().decode(data)));
        if (controller.signal.aborted || !host.current) return;
        const stage = host.current;
        player = new Replayer(recording.events, { ...REPLAY_OPTIONS, root: stage });
        const replay = player;
        remember = () => { memory.current[chapter] = { checkpoint: checkpointAt(shots, duration, replay.getCurrentTime()), paused: userPaused, autoPlay }; };
        const plane = stage.querySelector<HTMLElement>('.replayer-wrapper')!;
        plane.style.transformOrigin = 'top left';
        let size = { width: stage.clientWidth, height: stage.clientHeight };
        let source = { width: CAPTURE.width, height: CAPTURE.height };
        let speed = 1;
        gate = playbackGate(replay);
        replay.on('resize', dimensions => { source = dimensions as typeof source; });
        replay.on('finish', () => { gate?.(false); replay.pause(0); sync(); });
        paint = () => {
          frame = 0;
          if (controller.signal.aborted) return;
          const sample = sampleTrack(shots, replay.getCurrentTime(), mobile);
          if (plane.dataset.checkpoint !== sample.checkpoint) plane.dataset.checkpoint = sample.checkpoint;
          const cameraState = reducedMotion || sample.settled ? 'stable' : 'moving';
          if (plane.dataset.cameraState !== cameraState) plane.dataset.cameraState = cameraState;
          plane.style.transform = cameraTransform(reducedMotion ? shots[0].desktop : sample.frame, source, size);
          if (Math.abs(speed - sample.speed) > 0.01) { speed = sample.speed; replay.setConfig({ speed }); }
          if (allowed()) frame = requestAnimationFrame(paint);
        };
        observer = new ResizeObserver(() => { size = { width: stage.clientWidth, height: stage.clientHeight }; cancelAnimationFrame(frame); paint(); });
        observer.observe(stage);
        intersection = new IntersectionObserver(entries => { visible = entries[0].isIntersecting; sync(); }, { threshold: 0.05 });
        intersection.observe(stage);
        replay.pause(saved ? restoreCheckpoint(shots, duration, saved.checkpoint) : 0);
        settleReplayAnimations(stage.querySelector('iframe')?.contentDocument);
        paint();
        setState('ready');
      } catch { if (!controller.signal.aborted) setState('error'); }
    })();
    return () => {
      remember();
      controller.abort(); observer?.disconnect(); intersection?.disconnect(); cancelAnimationFrame(frame);
      document.removeEventListener('visibilitychange', sync);
      reduced.removeEventListener('change', motionChanged);
      command.current = () => {};
      player?.destroy();
    };
  }, [language, chapter, retry, autoPlay, mobile, theme, memory]);
  return <div className="recording" data-player-state={state} data-player-theme={theme}>
    {state !== 'ready' && <CheckpointPoster language={language} chapter={chapter} theme={theme} checkpoint={posterCheckpoint} mobile={mobile} />}
    {state !== 'ready' && <div className="media-status" role="status">{state === 'loading' ? <><Loader2 className="animate-spin" />{t.loading}</> : <><span>{t.error}</span><Button variant="secondary" onClick={() => setRetry(value => value + 1)}><RotateCcw />{t.retry}</Button></>}</div>}
    <div ref={host} className="replay-host" />
    {state === 'ready' && <Tooltip><TooltipTrigger asChild><Button className="replay-toggle" size="icon" variant="secondary" aria-label={paused ? t.resume : t.pause} onClick={() => { command.current(!paused); setPaused(!paused); }}>{paused ? <Play /> : <Pause />}</Button></TooltipTrigger><TooltipContent>{paused ? t.resume : t.pause}</TooltipContent></Tooltip>}
  </div>;
}
