import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { copy, type Language } from './content';

export function DemoPage({ src, language }: { src: string; language: Language }) {
  const [loaded, setLoaded] = useState(false);
  return <div className="site-demo-page">
    {!loaded && <div className="media-status" role="status"><Loader2 className="animate-spin" />{copy[language].loading}</div>}
    <iframe className="site-demo-frame" src={src} title={copy[language].openDemo} onLoad={() => setLoaded(true)} />
  </div>;
}
