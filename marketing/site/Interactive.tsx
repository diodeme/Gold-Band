import { useEffect, useRef, useState } from 'react';
import { Moon, Sun, RotateCcw, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable';
import { CAPTURE, copy, type Language } from './content';
import type {} from './preview';

export default function Interactive({ language }: { language: Language }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const [generation, setGeneration] = useState(0);
  const [state, setState] = useState<'loading' | 'ready' | 'error'>('loading');
  const [scheme, setScheme] = useState<'dark' | 'light'>('dark');
  const [font, setFont] = useState<'default' | 'mono'>('default');
  const t = copy[language];
  useEffect(() => {
    const element = stage.current!;
    let raf = 0;
    const resize = () => {
      const scale = element.clientWidth / CAPTURE.width;
      element.style.setProperty('--preview-scale', String(scale));
      element.style.height = `${CAPTURE.height * scale}px`;
    };
    const observer = new ResizeObserver(() => { cancelAnimationFrame(raf); raf = requestAnimationFrame(resize); });
    observer.observe(element); resize();
    return () => { observer.disconnect(); cancelAnimationFrame(raf); };
  }, []);
  useEffect(() => {
    setState('loading');
    const deadline = Date.now() + 45_000;
    const interval = setInterval(() => {
      if (Date.now() > deadline) { setState('error'); clearInterval(interval); return; }
      const doc = frame.current?.contentDocument;
      if (doc?.getElementById('workspace-center')) {
        const button = Array.from(doc.querySelectorAll<HTMLButtonElement>('button[aria-label]')).find(item => item.getAttribute('aria-label')?.includes('docs/workspace-notes.md'));
        if (!button) return;
        button.click(); setState('ready'); clearInterval(interval);
      }
    }, 250);
    return () => clearInterval(interval);
  }, [generation, language]);
  function appearance(nextScheme: typeof scheme, nextFont: typeof font) {
    setScheme(nextScheme); setFont(nextFont);
    frame.current?.contentWindow?.goldBandPreview?.appearance(nextScheme, nextFont);
  }
  function reset() { setState('loading'); setScheme('dark'); setFont('default'); setGeneration(value => value + 1); }
  return <div className="interactive-tool">
    <div className="preview-toolbar">
      <Tabs value={scheme} onValueChange={value => appearance(value as typeof scheme, font)}><TabsList><TabsTrigger value="dark" disabled={state !== 'ready'}><Moon size={14} />{t.dark}</TabsTrigger><TabsTrigger value="light" disabled={state !== 'ready'}><Sun size={14} />{t.light}</TabsTrigger></TabsList></Tabs>
      <Tabs value={font} onValueChange={value => appearance(scheme, value as typeof font)} aria-label={t.font}><TabsList><TabsTrigger value="default" disabled={state !== 'ready'}>{t.defaultFont}</TabsTrigger><TabsTrigger value="mono" disabled={state !== 'ready'}>{t.monoFont}</TabsTrigger></TabsList></Tabs>
      <Tooltip><TooltipTrigger asChild><Button variant="ghost" size="icon" aria-label={t.reset} onClick={reset}><RotateCcw /></Button></TooltipTrigger><TooltipContent>{t.reset}</TooltipContent></Tooltip>
    </div>
    <div ref={stage} className="interactive-stage">
      <div className="interactive-plane">
        <ResizablePanelGroup key={generation} orientation="horizontal">
          <ResizablePanel id="product-preview" defaultSize="99%" minSize={`${100 * CAPTURE.minimum / CAPTURE.width}%`}>
            <iframe ref={frame} src={`/preview.html?language=${language}&scene=personalize`} title={t.preview} sandbox="allow-scripts allow-same-origin" />
          </ResizablePanel>
          <ResizableHandle withHandle aria-label={t.resize} className="preview-handle" />
          <ResizablePanel defaultSize="1%" minSize="1%" />
        </ResizablePanelGroup>
      </div>
      {state !== 'ready' && <div className="media-status" role="status">{state === 'loading' ? <><Loader2 className="animate-spin" />{t.loading}</> : <><span>{t.error}</span><Button onClick={reset}>{t.retry}</Button></>}</div>}
    </div>
  </div>;
}
