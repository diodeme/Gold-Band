import { useEffect, useRef, useState } from 'react';
import { demoHref, type Language, type SiteTheme } from './content';

export default function DemoPage({ language, theme }: { language: Language; theme: SiteTheme }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [src] = useState(() => `${demoHref(language, theme)}${location.hash}`);
  useEffect(() => {
    const origin = new URL(src, location.href).origin;
    const receive = (event: MessageEvent) => {
      if (event.source !== frame.current?.contentWindow || event.origin !== origin) return;
      if (event.data?.type !== 'gold-band.demo.location' || typeof event.data.hash !== 'string' || (event.data.hash && !event.data.hash.startsWith('#'))) return;
      history.replaceState(null, '', `${location.pathname}${location.search}${event.data.hash}`);
    };
    const navigate = () => frame.current?.contentWindow?.postMessage({ type: 'gold-band.demo.navigate', hash: location.hash }, origin);
    window.addEventListener('message', receive);
    window.addEventListener('popstate', navigate);
    window.addEventListener('hashchange', navigate);
    return () => {
      window.removeEventListener('message', receive);
      window.removeEventListener('popstate', navigate);
      window.removeEventListener('hashchange', navigate);
    };
  }, [src]);
  // The initial appearance travels in the iframe URL; later changes reach the loaded document directly.
  useEffect(() => {
    const origin = new URL(src, location.href).origin;
    frame.current?.contentWindow?.postMessage({ type: 'gold-band.demo.appearance', appearance: theme }, origin);
  }, [src, theme]);
  return <iframe ref={frame} className="site-demo-frame" src={src} title="Gold Band Demo" />;
}
