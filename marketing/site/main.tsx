import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ArrowDown, ArrowRight, ArrowUpRight, Download, Code2, Languages, Loader2, Play, BookOpen, PanelsTopLeft, Sun, Moon } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { CHAPTER_IDS, copy, DESKTOP_QUERY, GITHUB, mediaPath, pageHref, parseRoute, type ChapterId, type Language, type Page } from './content';
import './style.css';
import { demoHref, siteConfig } from './config';
import { MOBILE_REPLAY_WIDTH } from './replay-model';

const config = siteConfig(import.meta.env, import.meta.env.PROD);

const Replay = lazy(() => import('./Replay'));
function useDesktop() {
  const [desktop, setDesktop] = useState(() => matchMedia(DESKTOP_QUERY).matches);
  useEffect(() => { const query = matchMedia(DESKTOP_QUERY); const changed = () => setDesktop(query.matches); query.addEventListener('change', changed); return () => query.removeEventListener('change', changed); }, []);
  return desktop;
}
function Media({ language, chapter, active, desktop, onActivate, theme }: { language: Language; chapter: ChapterId; active: boolean; desktop: boolean; onActivate: (chapter: ChapterId) => void; theme: 'light' | 'dark' }) {
  const [requested, setRequested] = useState(false);
  const surface = useRef<HTMLDivElement>(null);
  const [mobilePoster, setMobilePoster] = useState(false);
  const structuredPoster = chapter !== 'during';
  useEffect(() => {
    if (!structuredPoster || !surface.current) return;
    const observer = new ResizeObserver(entries => setMobilePoster(entries[0].contentRect.width < MOBILE_REPLAY_WIDTH));
    observer.observe(surface.current);
    return () => observer.disconnect();
  }, [chapter, structuredPoster]);
  const t = copy[language];
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const replay = active && (requested || !reducedMotion);
  return <div className="chapter-media" data-chapter-media={chapter}>
    <div className="media-label"><span><img src="/logo.svg" alt="" />Gold Band</span><span>{t.chapters[CHAPTER_IDS.indexOf(chapter)].eyebrow}</span></div>
    <div ref={surface} className="media-surface">
      <img className="poster" src={structuredPoster ? `${import.meta.env.BASE_URL}media/${chapter}/${language}-${theme}${mobilePoster ? '-mobile' : ''}/${chapter === 'before' ? 'establish' : chapter === 'after' ? 'attachment' : 'theme'}.png` : mediaPath(language, chapter, 'png')} width={structuredPoster && mobilePoster ? 320 : 1440} height={structuredPoster ? (mobilePoster ? 780 : 900) : 880} alt={`${t.chapters[CHAPTER_IDS.indexOf(chapter)].eyebrow} · Gold Band`} loading={chapter === 'before' ? 'eager' : 'lazy'} />
      {replay ? <Suspense fallback={<MediaLoading language={language} />}><Replay language={language} chapter={chapter} theme={theme} autoPlay={requested || !reducedMotion} /></Suspense> : <Button className="poster-play" variant="secondary" onClick={() => { setRequested(true); onActivate(chapter); }} aria-label={t.play}><Play />{t.play}</Button>}
    </div>
  </div>;
}
function MediaLoading({ language }: { language: Language }) { return <div className="media-status" role="status"><Loader2 className="animate-spin" />{copy[language].loading}</div>; }
function Story({ language, theme }: { language: Language; theme: 'light' | 'dark' }) {
  const desktop = useDesktop();
  const [active, setActive] = useState<ChapterId | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const t = copy[language];
  useEffect(() => {
    const entries = new Map<string, IntersectionObserverEntry>();
    const observer = new IntersectionObserver(updates => {
      updates.forEach(entry => entries.set(entry.target.id, entry));
      const visible = [...entries.values()].filter(entry => entry.isIntersecting).sort((a, b) => b.intersectionRatio - a.intersectionRatio);
      setActive(visible[0]?.target.id as ChapterId || null);
    }, { rootMargin: desktop ? '-18% 0px -25% 0px' : '-10% 0px -15% 0px', threshold: [0, 0.1, 0.25, 0.5, 0.75, 1] });
    root.current?.querySelectorAll('section[id]').forEach(element => observer.observe(element));
    return () => observer.disconnect();
  }, [desktop]);
  return <div ref={root} className={desktop ? 'story desktop-story' : 'story mobile-story'}>
    {desktop && <aside className="sticky-demonstration"><Media language={language} theme={theme} chapter={active ?? 'before'} active={active !== null} desktop onActivate={setActive} /></aside>}
    <div className="story-chapters">{t.chapters.map((chapter, index) => <section id={chapter.id} className="chapter" key={chapter.id} data-active={chapter.id === active}>
      {!desktop && <Media language={language} theme={theme} chapter={chapter.id} active={chapter.id === active} desktop={false} onActivate={setActive} />}
      <div className="chapter-copy"><div className="eyebrow"><span className="chapter-number">0{index + 1}</span>{chapter.eyebrow}</div><h2>{chapter.title}</h2><p>{chapter.body}</p><ul>{chapter.points.map(point => <li key={point}><span />{point}</li>)}</ul></div>
    </section>)}</div>
  </div>;
}
function App() {
  const [{ language, page }, setRoute] = useState(() => parseRoute(location.pathname.slice(config.base.length - 1)));
  const [theme, setTheme] = useState<'dark' | 'light'>(() => new URLSearchParams(location.search).get('theme') === 'light' ? 'light' : 'dark');
  const demo = demoHref(config.demoUrl, language, location.origin);
  useEffect(() => { document.documentElement.dataset.theme = theme; document.documentElement.classList.toggle('dark', theme === 'dark'); }, [theme]);
  useEffect(() => { if (page === 'demo') location.replace(demo); }, [page, demo]);
  useEffect(() => { const changed = () => setRoute(parseRoute(location.pathname.slice(config.base.length - 1))); window.addEventListener('popstate', changed); return () => window.removeEventListener('popstate', changed); }, []);
  const t = copy[language];
  useEffect(() => { document.documentElement.lang = language === 'zh' ? 'zh-CN' : 'en'; document.title = `${page === 'home' ? 'Gold Band' : page === 'documentation' ? 'Documentation · Gold Band' : page === 'demo' ? 'Demo · Gold Band' : '404 · Gold Band'}`; }, [language, page]);
  return <TooltipProvider><div className="site">
    <a className="skip-link" href="#main">{language === 'zh' ? '跳到正文' : 'Skip to content'}</a>
    <header className="site-header"><a className="brand" href={pageHref(language, 'home')}><img src="/logo.svg" alt="" /><span>Gold Band</span></a>
      <nav aria-label={language === 'zh' ? '主导航' : 'Main navigation'}>{(['home', 'documentation', 'demo'] as Page[]).map((item, index) => <a key={item} href={item === 'demo' ? demo : `${config.base.slice(0, -1)}${pageHref(language, item)}`} aria-current={page === item ? 'page' : undefined}>{t.nav[index]}</a>)}</nav>
      <div className="header-actions"><Tooltip><TooltipTrigger asChild><Button asChild variant="ghost" size="sm"><a href={`${config.base.slice(0, -1)}${pageHref(language === 'zh' ? 'en' : 'zh', page)}${location.hash}`} onClick={event => { event.preventDefault(); const next = language === 'zh' ? 'en' : 'zh'; history.pushState(null, '', `${config.base.slice(0, -1)}${pageHref(next, page)}${location.hash}`); setRoute({ language: next, page }); }} hrefLang={language === 'zh' ? 'en' : 'zh-CN'} aria-label={language === 'zh' ? 'Switch to English' : '切换为中文'}><Languages /><span>{language === 'zh' ? 'EN' : '中文'}</span></a></Button></TooltipTrigger><TooltipContent>{language === 'zh' ? 'English' : '中文'}</TooltipContent></Tooltip>
      <Tooltip><TooltipTrigger asChild><Button size="icon" variant="ghost" aria-label={theme === 'dark' ? t.light : t.dark} onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}>{theme === 'dark' ? <Sun /> : <Moon />}</Button></TooltipTrigger><TooltipContent>{theme === 'dark' ? t.light : t.dark}</TooltipContent></Tooltip>
      <Tooltip><TooltipTrigger asChild><Button asChild size="icon" variant="ghost"><a href={GITHUB} aria-label="GitHub"><Code2 /></a></Button></TooltipTrigger><TooltipContent>GitHub</TooltipContent></Tooltip></div>
    </header>
    <main id="main">{page === 'home' ? <>
      <section className="intro"><div className="eyebrow">{t.kicker}</div><h1>Gold Band</h1><p className="tagline">{t.tagline}</p><p className="intro-copy">{t.intro}</p><div className="intro-actions"><Button asChild size="lg"><a href={config.downloadUrl}><Download />{t.download}<ArrowUpRight /></a></Button><a className="text-link" href="#before">{t.play}<ArrowDown size={16} /></a></div></section>
      <div className="story-heading"><span>{t.story}</span><span>01 / 04</span></div><Story language={language} theme={theme} />
      <section className="closing"><img src="/logo.svg" alt="" /><h2>{t.foot}</h2><p>{t.footText}</p><Button asChild size="lg"><a href={config.downloadUrl}><Download />{t.download}<ArrowUpRight /></a></Button></section>
    </> : <section className="placeholder-page">{page === 'documentation' ? <BookOpen /> : <PanelsTopLeft />}<p className="eyebrow">Gold Band</p><h1>{page === 'documentation' ? t.nav[1] : page === 'demo' ? 'Demo' : t.missing}</h1>{page !== 'not-found' && <><h2>{t.placeholder}</h2><p>{page === 'documentation' ? t.docsText : t.demoText}</p></>}<div className="intro-actions"><Button asChild><a href={pageHref(language, 'home')}>{t.back}<ArrowRight /></a></Button>{page === 'documentation' && <a className="text-link" href={`${GITHUB}#readme`}>GitHub<ArrowUpRight size={16} /></a>}</div></section>}</main>
    <footer><a className="brand" href={pageHref(language, 'home')}><img src="/logo.svg" alt="" /><span>Gold Band</span></a><span>AGPL-3.0 · Open source</span><a href={GITHUB}>{t.source}<ArrowUpRight size={14} /></a></footer>
  </div></TooltipProvider>;
}
createRoot(document.getElementById('root')!).render(<App />);
