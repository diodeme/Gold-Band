import { lazy, Suspense, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ArrowRight, ArrowUpRight, Download, Code2, Languages, Loader2, Play, BookOpen, PanelsTopLeft, Sun, Moon, Monitor } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { CHAPTER_IDS, copy, DESKTOP_QUERY, GITHUB, posterPath, pageHref, parseRoute, type ChapterId, type Language, type Page } from './content';
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { browserStorage, effectiveTheme, readPreferences, writePreferences, type SitePreferences } from './preferences';
import './style.css';
import { demoHref, siteConfig } from './config';
import { isPlainNavigation } from './navigation';
import { DemoPage } from './DemoPage';
import { MOBILE_REPLAY_WIDTH } from './replay-model';

const config = siteConfig(import.meta.env, import.meta.env.PROD);
const href = (language: Language, page: Page) => pageHref(language, page, config.base);

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
  useEffect(() => {
    if (!surface.current) return;
    const observer = new ResizeObserver(entries => setMobilePoster(entries[0].contentRect.width < MOBILE_REPLAY_WIDTH));
    observer.observe(surface.current);
    return () => observer.disconnect();
  }, []);
  const t = copy[language];
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const replay = active && (requested || !reducedMotion);
  return <div className="chapter-media" data-chapter-media={chapter}>
    <div ref={surface} className="media-surface">
      <img className="poster" src={posterPath(config.base, language, chapter, theme, mobilePoster)} width={mobilePoster ? 320 : 1440} height={mobilePoster ? 780 : 900} alt={`${t.chapters[CHAPTER_IDS.indexOf(chapter)].eyebrow} · Gold Band`} loading={chapter === 'before' ? 'eager' : 'lazy'} />
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
  useLayoutEffect(() => {
    const restore = () => {
      const chapter = location.hash.slice(1);
      if (CHAPTER_IDS.includes(chapter as ChapterId)) document.getElementById(chapter)?.scrollIntoView({ behavior: 'instant', block: 'start' });
    };
    restore();
    window.addEventListener('hashchange', restore);
    return () => window.removeEventListener('hashchange', restore);
  }, [language]);
  useEffect(() => {
    const entries = new Map<string, IntersectionObserverEntry>();
    const observer = new IntersectionObserver(updates => {
      updates.forEach(entry => entries.set(entry.target.id, entry));
      const visible = [...entries.values()].filter(entry => entry.isIntersecting).sort((a, b) => b.intersectionRatio - a.intersectionRatio);
      const next = visible[0]?.target.id as ChapterId || null;
      setActive(next);
      if (next && location.hash !== `#${next}`) history.replaceState(null, '', `${location.pathname}${location.search}#${next}`);
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
export function App() {
  const [preferences, setPreferences] = useState(() => readPreferences(browserStorage(), navigator.language));
  const [{ language, page }, setRoute] = useState(() => parseRoute(location.pathname.slice(config.base.length - 1), preferences.language));
  const [systemDark, setSystemDark] = useState(() => matchMedia('(prefers-color-scheme: dark)').matches);
  const theme = effectiveTheme(preferences.theme, systemDark);
  const chooseTheme = (value: string) => {
    const next = { ...preferences, language, theme: value as SitePreferences['theme'] };
    setPreferences(next); writePreferences(browserStorage(), next);
  };
  useEffect(() => {
    const query = matchMedia('(prefers-color-scheme: dark)');
    const changed = () => setSystemDark(query.matches);
    query.addEventListener('change', changed);
    return () => query.removeEventListener('change', changed);
  }, []);
  const demo = demoHref(config.demoUrl, language, location.origin, theme);
  useEffect(() => { document.documentElement.dataset.theme = theme; document.documentElement.classList.toggle('dark', theme === 'dark'); }, [theme]);

  useEffect(() => { const changed = () => setRoute(parseRoute(location.pathname.slice(config.base.length - 1), preferences.language)); window.addEventListener('popstate', changed); return () => window.removeEventListener('popstate', changed); }, [preferences.language]);
  const t = copy[language];
  useEffect(() => { document.documentElement.lang = language === 'zh' ? 'zh-CN' : 'en'; document.title = `${page === 'home' ? 'Gold Band' : page === 'documentation' ? 'Documentation · Gold Band' : page === 'demo' ? 'Demo · Gold Band' : '404 · Gold Band'}`; }, [language, page]);
  return <TooltipProvider><div className="site" onClick={event => {
    if (!isPlainNavigation(event)) return;
    const anchor = (event.target as Element).closest('a');
    if (!anchor || anchor.target || anchor.hasAttribute('download')) return;
    const url = new URL(anchor.href);
    if (url.origin !== location.origin || !url.pathname.startsWith(config.base)) return;
    const destination = parseRoute(url.pathname.slice(config.base.length - 1), preferences.language);
    if (destination.page === 'not-found') return;
    if (url.pathname === location.pathname && url.hash && url.hash !== location.hash) return;
    event.preventDefault();
    history.pushState(null, '', url.pathname + url.search + url.hash);
    setRoute(destination);
    window.scrollTo({ top: 0, behavior: 'instant' });
  }}>
    <a className="skip-link" href="#main">{language === 'zh' ? '跳到正文' : 'Skip to content'}</a>
    <header className="site-header"><a className="brand" href={href(language, 'home')}><img src={`${config.base}logo.svg`} alt="" /><span>Gold Band</span></a>
      <nav aria-label={language === 'zh' ? '主导航' : 'Main navigation'}>{(['home', 'documentation', 'demo'] as Page[]).map((item, index) => <a key={item} href={href(language, item)} aria-current={page === item ? 'page' : undefined}>{t.nav[index]}</a>)}</nav>
      <div className="header-actions"><Tooltip><TooltipTrigger asChild><Button asChild variant="ghost" size="sm"><a href={`${href(language === 'zh' ? 'en' : 'zh', page)}${location.hash}`} onClick={event => { event.preventDefault(); const next = language === 'zh' ? 'en' : 'zh'; history.pushState(null, '', `${href(next, page)}${location.hash}`); const updated = { ...preferences, language: next } as SitePreferences; setPreferences(updated); writePreferences(browserStorage(), updated); setRoute({ language: next, page }); }} hrefLang={language === 'zh' ? 'en' : 'zh-CN'} aria-label={language === 'zh' ? 'Switch to English' : '切换为中文'}><Languages /><span>{language === 'zh' ? 'EN' : '中文'}</span></a></Button></TooltipTrigger><TooltipContent>{language === 'zh' ? 'English' : '中文'}</TooltipContent></Tooltip>
      <DropdownMenu><Tooltip><TooltipTrigger asChild><DropdownMenuTrigger asChild><Button size="icon" variant="ghost" aria-label={t.appearance}>{preferences.theme === 'system' ? <Monitor /> : theme === 'dark' ? <Moon /> : <Sun />}</Button></DropdownMenuTrigger></TooltipTrigger><TooltipContent>{t.appearance}</TooltipContent></Tooltip><DropdownMenuContent align="end"><DropdownMenuRadioGroup value={preferences.theme} onValueChange={chooseTheme}>{(['system', 'light', 'dark'] as const).map(value => <DropdownMenuRadioItem key={value} value={value}>{t[value]}</DropdownMenuRadioItem>)}</DropdownMenuRadioGroup></DropdownMenuContent></DropdownMenu>
      <Tooltip><TooltipTrigger asChild><Button asChild size="icon" variant="ghost"><a href={GITHUB} aria-label="GitHub"><Code2 /></a></Button></TooltipTrigger><TooltipContent>GitHub</TooltipContent></Tooltip></div>
    </header>
    <main id="main" className={page === 'demo' ? 'site-demo-main' : undefined}>{page === 'home' ? <>
      <section className="intro"><div className="eyebrow">{t.kicker}</div><h1>Gold Band</h1><p className="tagline">{t.tagline}</p><p className="intro-copy">{t.intro}</p><div className="intro-actions"><Button asChild size="lg"><a href={config.downloadUrl}><Download />{t.download}<ArrowUpRight /></a></Button><a className="text-link" href={href(language, 'demo')}>{t.openDemo}<ArrowRight size={16} /></a></div></section>
      <div className="story-heading"><span>{t.story}</span><span>01 / 04</span></div><Story language={language} theme={theme} />
      <section className="closing"><img src={`${config.base}logo.svg`} alt="" /><h2>{t.foot}</h2><p>{t.footText}</p><Button asChild size="lg"><a href={config.downloadUrl}><Download />{t.download}<ArrowUpRight /></a></Button></section>
    </> : page === 'demo' ? <DemoPage key={language} src={demo} language={language} /> : <section className="placeholder-page">{page === 'documentation' ? <BookOpen /> : <PanelsTopLeft />}<p className="eyebrow">Gold Band</p><h1>{page === 'documentation' ? t.nav[1] : t.missing}</h1>{page === 'documentation' && <><h2>{t.placeholder}</h2><p>{t.docsText}</p></>}<div className="intro-actions"><Button asChild><a href={href(language, 'home')}>{t.back}<ArrowRight /></a></Button>{page === 'documentation' && <a className="text-link" href={`${GITHUB}#readme`}>GitHub<ArrowUpRight size={16} /></a>}</div></section>}</main>
    {page !== 'demo' && <footer><a className="brand" href={href(language, 'home')}><img src={`${config.base}logo.svg`} alt="" /><span>Gold Band</span></a><span>AGPL-3.0 · Open source</span><a href={GITHUB}>{t.source}<ArrowUpRight size={14} /></a></footer>}
  </div></TooltipProvider>;
}
