import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { createRoot } from 'react-dom/client';
import Player from 'rrweb-player';
import { Circle, Download, Loader2, Moon, Play, RotateCcw, Square, Sun } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { CAPTURE_SIZE, canReplay, type Recording } from './recording';
import 'rrweb-player/dist/style.css';
import './style.css';

type Phase = 'loading' | 'ready' | 'preparing' | 'recording' | 'replay' | 'error';
// rrweb-player exposes Svelte lifecycle methods, but does not ship Svelte's types.
type PlayerInstance = Player & {
  $set: (props: { width: number; height: number; maxScale: number }) => void;
  $destroy: () => void;
};
const CHAPTERS = [
  { width: CAPTURE_SIZE.width, label: '三栏', wait: 2200 },
  { width: CAPTURE_SIZE.medium, label: '双栏', wait: 2800 },
  { width: CAPTURE_SIZE.narrow, label: '单栏', wait: 2800 },
  { width: CAPTURE_SIZE.medium, label: '恢复双栏', wait: 2800 },
  { width: CAPTURE_SIZE.width, label: '恢复三栏', wait: 2800 },
];

function Replay({ recording }: { recording: Recording }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!host.current) return;
    const width = host.current.clientWidth;
    const player = new Player({ target: host.current, props: {
      events: recording.events,
      width,
      height: Math.round(width * CAPTURE_SIZE.height / CAPTURE_SIZE.width),
      maxScale: width / CAPTURE_SIZE.width,
      autoPlay: !matchMedia('(prefers-reduced-motion: reduce)').matches,
      skipInactive: false,
      showController: true,
    } }) as PlayerInstance;
    let resizeFrame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(() => {
        if (!host.current) return;
        const nextWidth = host.current.clientWidth;
        player.$set({ width: nextWidth, height: Math.round(nextWidth * CAPTURE_SIZE.height / CAPTURE_SIZE.width), maxScale: nextWidth / CAPTURE_SIZE.width });
        player.triggerResize();
      });
    });
    observer.observe(host.current);
    const pauseHidden = () => { if (document.hidden) player.pause(); };
    document.addEventListener('visibilitychange', pauseHidden);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(resizeFrame);
      document.removeEventListener('visibilitychange', pauseHidden);
      player.pause();
      player.getReplayer().destroy();
      player.$destroy();
    };
  }, [recording]);
  return <div ref={host} className="replay-host" data-demo-replay />;
}

function App() {
  const [phase, setPhase] = useState<Phase>('loading');
  const [theme, setTheme] = useState<'light' | 'dark'>('light');
  const [recording, setRecording] = useState<Recording | null>(null);
  const [width, setWidth] = useState<number>(CAPTURE_SIZE.width);
  const [chapter, setChapter] = useState('准备中');
  const [error, setError] = useState('');
  const [generation, setGeneration] = useState(0);
  const [elapsed, setElapsed] = useState(0);
  const [automatic, setAutomatic] = useState(false);
  const iframe = useRef<HTMLIFrameElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const controller = useRef<AbortController | null>(null);
  const startedAt = useRef(0);
  const replaying = phase === 'replay';

  useEffect(() => {
    document.documentElement.classList.toggle('dark', theme === 'dark');
    document.documentElement.style.colorScheme = theme;
  }, [theme]);

  useEffect(() => {
    if (!stage.current || replaying) return;
    const element = stage.current;
    const resize = () => element.style.setProperty('--capture-scale', String(Math.min(1, element.clientWidth / CAPTURE_SIZE.width)));
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    resize();
    return () => observer.disconnect();
  }, [generation, replaying]);

  useEffect(() => {
    if (replaying) return;
    let cancelled = false;
    const deadline = Date.now() + 30_000;
    const timer = setInterval(() => {
      const child = iframe.current?.contentWindow;
      if (child?.goldBandCapture && child.document.querySelector('[id="workspace-center"]')) {
        clearInterval(timer);
        if (!cancelled) { setPhase('ready'); setChapter('真实页面'); }
      } else if (Date.now() > deadline) {
        clearInterval(timer);
        if (!cancelled) { setPhase('error'); setError('产品预览未就绪，请重新加载。'); }
      }
    }, 200);
    return () => { cancelled = true; clearInterval(timer); };
  }, [generation, replaying]);

  useEffect(() => {
    if (phase !== 'recording') return;
    const timer = setInterval(() => {
      setElapsed(Math.floor((Date.now() - startedAt.current) / 1000));
      if (!iframe.current?.contentWindow?.goldBandCapture?.status().recording) finish();
    }, 250);
    return () => clearInterval(timer);
  }, [phase]);

  useEffect(() => () => {
    controller.current?.abort();
    iframe.current?.contentWindow?.goldBandCapture?.stop();
  }, []);

  function reset(nextTheme = theme) {
    controller.current?.abort();
    iframe.current?.contentWindow?.goldBandCapture?.stop();
    setPhase('loading');
    setTheme(nextTheme);
    setWidth(CAPTURE_SIZE.width);
    setChapter('准备中');
    setError('');
    setGeneration((value) => value + 1);
  }

  function finish() {
    controller.current?.abort();
    const result = iframe.current?.contentWindow?.goldBandCapture?.stop();
    setAutomatic(false);
    if (result && canReplay(result)) {
      setRecording(result);
      setPhase('replay');
      setChapter('录制回放');
    } else {
      setPhase('error');
      setError('未获得完整画面，请重新录制。');
    }
  }

  function wait(ms: number, signal: AbortSignal) {
    return new Promise<void>((resolve, reject) => {
      if (signal.aborted) { reject(signal.reason); return; }
      const abort = () => { clearTimeout(timer); reject(signal.reason); };
      const timer = setTimeout(() => { signal.removeEventListener('abort', abort); resolve(); }, ms);
      signal.addEventListener('abort', abort, { once: true });
    });
  }

  async function start(auto: boolean) {
    const child = iframe.current?.contentWindow;
    if (!child?.goldBandCapture || phase !== 'ready') return;
    const work = new AbortController();
    controller.current?.abort();
    controller.current = work;
    setAutomatic(auto);
    setPhase('preparing');
    startedAt.current = Date.now();
    setElapsed(0);
    try {
      await child.document.fonts.ready;
      if (work.signal.aborted) return;
      if (auto) {
        setWidth(CAPTURE_SIZE.width);
        const file = [...child.document.querySelectorAll<HTMLButtonElement>('button[aria-label]')]
          .find((button) => button.getAttribute('aria-label')?.includes('docs/workspace-notes.md'));
        if (!file) throw new Error('样例文件尚未就绪，请重新加载。');
        file.click();
        const deadline = Date.now() + 5000;
        while (!child.document.querySelector('[data-right-workspace-dock]') || child.innerWidth !== CAPTURE_SIZE.width) {
          if (Date.now() > deadline) throw new Error('三栏布局未就绪，请重新加载。');
          await wait(100, work.signal);
        }
        await wait(500, work.signal);
      }
      child.goldBandCapture.start();
      setPhase('recording');
      if (!auto) return;
      for (const item of CHAPTERS) {
        setWidth(item.width);
        setChapter(item.label);
        child.goldBandCapture.mark(item.label);
        await wait(item.wait, work.signal);
      }
      finish();
    } catch (cause) {
      if (work.signal.aborted) return;
      child.goldBandCapture.stop();
      setPhase('error');
      setError(cause instanceof Error ? cause.message : '录制失败，请重试。');
    }
  }

  function download() {
    if (!recording) return;
    const url = URL.createObjectURL(new Blob([JSON.stringify(recording)], { type: 'application/json' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = 'gold-band-rrweb-recording.json';
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  return <TooltipProvider><main className="demo-shell" data-demo-phase={phase}>
    <header className="demo-header">
      <div className="brand"><img src="/logo.svg" alt="" /><div><h1>Gold Band</h1><span>录制回放 · 浏览器预览数据</span></div></div>
      <div className="actions">
        <Tooltip><TooltipTrigger asChild><Button variant="ghost" size="icon" aria-label={theme === 'light' ? '深色' : '浅色'} disabled={phase === 'recording' || phase === 'preparing'} onClick={() => reset(theme === 'light' ? 'dark' : 'light')}>{theme === 'light' ? <Moon /> : <Sun />}</Button></TooltipTrigger><TooltipContent>切换主题并重新录制</TooltipContent></Tooltip>
        <Button variant="outline" onClick={() => reset()} disabled={phase === 'recording' || phase === 'preparing'}><RotateCcw />重新加载</Button>
      </div>
    </header>
    <div className="toolbar">
      <div className="actions">
        {phase !== 'recording' ? <>
          <Button onClick={() => start(true)} disabled={phase !== 'ready'}><Play />录制布局演示</Button>
          <Button variant="outline" onClick={() => start(false)} disabled={phase !== 'ready'}><Circle />手动录制</Button>
        </> : <Button variant="destructive" onClick={finish}><Square />停止并回放</Button>}
        {recording && <Button variant="ghost" onClick={download}><Download />导出 JSON</Button>}
      </div>
      <div className="status" aria-live="polite">{(phase === 'loading' || phase === 'preparing') && <Loader2 className="animate-spin" />}{phase === 'recording' && <span className="record-dot" />}{phase === 'preparing' ? '准备录制' : chapter}{phase === 'recording' && <span>{elapsed}s / 60s</span>}{replaying && recording && <span>{(recording.bytes / 1024 / 1024).toFixed(2)} MB · {recording.events.length} 个事件</span>}</div>
    </div>
    {!replaying && <div className="viewport-toolbar">
      <Tabs value={String(width)} onValueChange={(value) => setWidth(Number(value))}>
        <TabsList><TabsTrigger value={String(CAPTURE_SIZE.width)} disabled={automatic}>1440 px</TabsTrigger><TabsTrigger value={String(CAPTURE_SIZE.medium)} disabled={automatic}>900 px</TabsTrigger><TabsTrigger value={String(CAPTURE_SIZE.narrow)} disabled={automatic}>600 px</TabsTrigger></TabsList>
      </Tabs>
      <span>录制视口</span>
    </div>}
    {error && <p role="alert" className="error">{error}</p>}
    {replaying && recording ? <Replay recording={recording} /> : <div ref={stage} className="capture-stage">
      <div className="capture-plane"><iframe key={generation} ref={iframe} title="Gold Band 产品录制" src={`./capture.html?theme=${theme}`} style={{ width, height: CAPTURE_SIZE.height } as CSSProperties} /></div>
    </div>}
    {recording && recording.reason !== 'manual' && replaying && <p role="status" className="error">已达到录制上限，保留上限以内的画面。</p>}
  </main></TooltipProvider>;
}

createRoot(document.getElementById('root')!).render(<App />);
