import { lazy, Suspense, useEffect, useRef, useState, type MutableRefObject } from 'react';
import { createRoot } from 'react-dom/client';
import { ArrowDown, ArrowRight, ArrowUpRight, Download, Code2, Languages, Loader2, Play, BookOpen, PanelsTopLeft, Sun, Moon, Monitor } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { useSiteTheme } from './theme';
import type { ReplayPositions } from './Replay';
import type { SiteTheme } from './content';
import { CHAPTER_IDS, chapterFromHash, copy, DESKTOP_QUERY, GITHUB, SITE_LINKS, mediaPath, pageHref, parseRoute, type ChapterId, type Language, type Page } from './content';
import './style.css';

const Replay = lazy(() => import('./Replay'));
const DemoPage = lazy(() => import('./DemoPage'));
function useDesktop() {
  const [desktop, setDesktop] = useState(() => matchMedia(DESKTOP_QUERY).matches);
  useEffect(() => { const query = matchMedia(DESKTOP_QUERY); const changed = () => setDesktop(query.matches); query.addEventListener('change', changed); return () => query.removeEventListener('change', changed); }, []);
  return desktop;
}
function Media({ language, chapter, active, desktop, onActivate, theme, positions }: { language: Language; chapter: ChapterId; active: boolean; desktop: boolean; onActivate: (chapter: ChapterId) => void; theme: SiteTheme; positions: MutableRefObject<ReplayPositions> }) {
  const [requested, setRequested] = useState(false);
  const t = copy[language];
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const replay = active && (requested || !reducedMotion);
  return <div className="chapter-media" data-chapter-media={chapter}>
    <div className="media-surface">
      <img className="poster" src={mediaPath(language, chapter, 'png', theme)} width="1440" height="880" alt={`${t.chapters[CHAPTER_IDS.indexOf(chapter)].eyebrow} · Gold Band`} loading={chapter === 'before' ? 'eager' : 'lazy'} />
      {replay ? <Suspense fallback={<MediaLoading language={language} />}><Replay language={language} chapter={chapter} autoPlay={requested || !reducedMotion} mobile={!desktop} theme={theme} positions={positions} /></Suspense> : <Button className="poster-play" variant="secondary" onClick={() => { setRequested(true); onActivate(chapter); }} aria-label={t.play}><Play />{t.play}</Button>}
    </div>
  </div>;
}
function MediaLoading({ language }: { language: Language }) { return <div className="media-status" role="status"><Loader2 className="animate-spin" />{copy[language].loading}</div>; }
function Story({ language, theme }: { language: Language; theme: SiteTheme }) {
  const positions = useRef<ReplayPositions>({});
  const desktop = useDesktop();
  const [active, setActive] = useState<ChapterId | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const chapterPosition = useRef<ChapterId | null>(chapterFromHash(location.hash));
  const t = copy[language];
  useEffect(() => {
    let frame = 0;
    const restore = () => {
      const chapter = chapterPosition.current;
      cancelAnimationFrame(frame);
      if (chapter) frame = requestAnimationFrame(() => root.current?.querySelector(`#${chapter}`)?.scrollIntoView({ block: 'start' }));
    };
    const navigate = () => { chapterPosition.current = chapterFromHash(location.hash); restore(); };
    restore();
    window.addEventListener('hashchange', navigate);
    return () => { cancelAnimationFrame(frame); window.removeEventListener('hashchange', navigate); };
  }, [desktop]);
  useEffect(() => {
    const entries = new Map<string, IntersectionObserverEntry>();
    const observer = new IntersectionObserver(updates => {
      updates.forEach(entry => entries.set(entry.target.id, entry));
      const visible = [...entries.values()].filter(entry => entry.isIntersecting).sort((a, b) => b.intersectionRatio - a.intersectionRatio);
      const next = visible[0]?.target.id as ChapterId || null;
      if (next) chapterPosition.current = next;
      setActive(next);
    }, { rootMargin: desktop ? '-18% 0px -25% 0px' : '-10% 0px -15% 0px', threshold: [0, 0.1, 0.25, 0.5, 0.75, 1] });
    root.current?.querySelectorAll('section[id]').forEach(element => observer.observe(element));
    return () => observer.disconnect();
  }, [desktop]);
  return <div ref={root} className={desktop ? 'story desktop-story' : 'story mobile-story'}>
    {desktop && <aside className="sticky-demonstration"><Media key={`${language}-${active ?? 'before'}`} language={language} chapter={active ?? 'before'} active={active !== null} desktop onActivate={setActive} theme={theme} positions={positions} /></aside>}
    <div className="story-chapters">{t.chapters.map((chapter, index) => <section id={chapter.id} className="chapter" key={chapter.id} data-active={chapter.id === active}>
      {!desktop && <Media language={language} chapter={chapter.id} active={chapter.id === active} desktop={false} onActivate={setActive} theme={theme} positions={positions} />}
      <div className="chapter-copy"><div className="eyebrow"><span className="chapter-number">0{index + 1}</span>{chapter.eyebrow}</div><h2>{chapter.title}</h2><p>{chapter.body}</p><ul>{chapter.points.map(point => <li key={point}><span />{point}</li>)}</ul></div>
    </section>)}</div>
  </div>;
}
function App() {
  const themeTrigger = useRef<HTMLButtonElement>(null);
  const [pathname, setPathname] = useState(location.pathname);
  const { language, page } = parseRoute(pathname);
  const { theme, selected, select } = useSiteTheme();
  const t = copy[language];
  useEffect(() => {
    const update = () => setPathname(location.pathname);
    window.addEventListener('popstate', update);
    return () => window.removeEventListener('popstate', update);
  }, []);
  useEffect(() => { document.documentElement.lang = language === 'zh' ? 'zh-CN' : 'en'; document.title = `${page === 'home' ? 'Gold Band' : page === 'documentation' ? 'Documentation · Gold Band' : page === 'demo' ? 'Demo · Gold Band' : '404 · Gold Band'}`; }, [language, page]);
  return <TooltipProvider><div className={`site${page === 'demo' ? ' site-with-demo' : ''}`} onClick={event => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const anchor = (event.target as Element).closest('a');
    if (!anchor || anchor.target || anchor.hasAttribute('download')) return;
    const url = new URL(anchor.href);
    if (url.origin !== location.origin || url.pathname === location.pathname || parseRoute(url.pathname).page === 'not-found') return;
    event.preventDefault();
    history.pushState(null, '', url);
    setPathname(url.pathname);
    window.scrollTo({ top: 0, behavior: 'instant' });
  }}>
    <a className="skip-link" href="#main">{language === 'zh' ? '跳到正文' : 'Skip to content'}</a>
    <header className="site-header"><a className="brand" href={pageHref(language, 'home')}><img src="/logo.svg" alt="" /><span>Gold Band</span></a>
      <nav aria-label={language === 'zh' ? '主导航' : 'Main navigation'}>{(['home', 'documentation', 'demo'] as Page[]).map((item, index) => <a key={item} href={pageHref(language, item)} aria-current={page === item ? 'page' : undefined}>{t.nav[index]}</a>)}</nav>
<div className="header-actions"><DropdownMenu><Tooltip><TooltipTrigger asChild><DropdownMenuTrigger asChild><Button ref={themeTrigger} size="icon" variant="ghost" aria-label={language === 'zh' ? '主题' : 'Theme'}>{selected === 'system' ? <Monitor /> : theme === 'dark' ? <Moon /> : <Sun />}</Button></DropdownMenuTrigger></TooltipTrigger><TooltipContent>{language === 'zh' ? '主题' : 'Theme'}</TooltipContent></Tooltip><DropdownMenuContent align="end" onCloseAutoFocus={event => { event.preventDefault(); themeTrigger.current?.focus({ preventScroll: true }); }}><DropdownMenuRadioGroup value={selected} onValueChange={select}>{(['system', 'light', 'dark'] as const).map((value, index) => <DropdownMenuRadioItem key={value} value={value}>{language === 'zh' ? ['跟随系统', '浅色', '深色'][index] : ['System', 'Light', 'Dark'][index]}</DropdownMenuRadioItem>)}</DropdownMenuRadioGroup></DropdownMenuContent></DropdownMenu><Tooltip><TooltipTrigger asChild><Button asChild variant="ghost" size="sm"><a href={`${pageHref(language === 'zh' ? 'en' : 'zh', page)}${location.hash}`} onClick={event => { const active = document.querySelector('section[data-active="true"]')?.id; if (active && page === 'home') event.currentTarget.hash = active; }} hrefLang={language === 'zh' ? 'en' : 'zh-CN'} aria-label={language === 'zh' ? 'Switch to English' : '切换为中文'}><Languages /><span>{language === 'zh' ? 'EN' : '中文'}</span></a></Button></TooltipTrigger><TooltipContent>{language === 'zh' ? 'English' : '中文'}</TooltipContent></Tooltip>
      <Tooltip><TooltipTrigger asChild><Button asChild size="icon" variant="ghost"><a href={GITHUB} aria-label="GitHub"><Code2 /></a></Button></TooltipTrigger><TooltipContent>GitHub</TooltipContent></Tooltip></div>
    </header>
    <main id="main">{page === 'home' ? <>
      <section className="intro"><div className="eyebrow">{t.kicker}</div><h1>Gold Band</h1><p className="tagline">{t.tagline}</p><p className="intro-copy">{t.intro}</p><div className="intro-actions"><Button asChild size="lg"><a href={SITE_LINKS.download}><Download />{t.download}<ArrowUpRight /></a></Button><a className="text-link" href={pageHref(language, 'demo')}>{t.interactive}<ArrowUpRight size={16} /></a><a className="text-link" href="#before">{t.play}<ArrowDown size={16} /></a></div></section>
      <div className="story-heading"><span>{t.story}</span><span>01 / 04</span></div><Story language={language} theme={theme} />
      <section className="closing"><img src="/logo.svg" alt="" /><h2>{t.foot}</h2><p>{t.footText}</p><Button asChild size="lg"><a href={SITE_LINKS.download}><Download />{t.download}<ArrowUpRight /></a></Button></section>
    </> : page === 'demo' ? <Suspense fallback={<MediaLoading language={language} />}><DemoPage key={language} language={language} theme={theme} /></Suspense> : <section className="placeholder-page">{page === 'documentation' ? <BookOpen /> : <PanelsTopLeft />}<p className="eyebrow">Gold Band</p><h1>{page === 'documentation' ? t.nav[1] : t.missing}</h1>{page !== 'not-found' && <><h2>{t.placeholder}</h2><p>{page === 'documentation' ? t.docsText : t.demoText}</p></>}<div className="intro-actions"><Button asChild><a href={pageHref(language, 'home')}>{t.back}<ArrowRight /></a></Button>{page === 'documentation' && <a className="text-link" href={`${GITHUB}#readme`}>GitHub<ArrowUpRight size={16} /></a>}</div></section>}</main>
    {page !== 'demo' && <footer><a className="brand" href={pageHref(language, 'home')}><img src="/logo.svg" alt="" /><span>Gold Band</span></a><span>AGPL-3.0 · Open source</span><a href={GITHUB}>{t.source}<ArrowUpRight size={14} /></a></footer>}
  </div></TooltipProvider>;
}
createRoot(document.getElementById('root')!).render(<App />);
