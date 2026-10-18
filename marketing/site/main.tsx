import { useEffect, useState } from 'react';
import { ArrowRight, ArrowUpRight, Code2, Languages, BookOpen, PanelsTopLeft, Sun, Moon, Monitor } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { copy, GITHUB, pageHref, parseRoute, type Language, type Page } from './content';
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { browserStorage, effectiveTheme, readPreferences, writePreferences, type SitePreferences } from './preferences';
import './style.css';
import { demoHref, siteConfig } from './config';
import { isPlainNavigation } from './navigation';
import { DemoPage } from './DemoPage';

const config = siteConfig(import.meta.env, import.meta.env.PROD);
const href = (language: Language, page: Page) => pageHref(language, page, config.base);
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
  const [demoHash, setDemoHash] = useState(location.hash);
  useEffect(() => {
    const changed = () => setDemoHash(location.hash);
    window.addEventListener('hashchange', changed);
    return () => window.removeEventListener('hashchange', changed);
  }, []);
  const demo = demoHref(config.demoUrl, language, location.origin, theme, demoHash);
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
    <main id="main" className={page === 'demo' ? 'site-demo-main' : undefined}>{page === 'demo' ? <DemoPage key={language} src={demo} language={language} /> : <section className="placeholder-page">{page === 'documentation' ? <BookOpen /> : page === 'not-found' ? <PanelsTopLeft /> : null}<p className="eyebrow">Gold Band</p><h1>{page === 'home' ? t.homeTitle : page === 'documentation' ? t.nav[1] : t.missing}</h1>{page === 'home' && <p>{t.homeText}</p>}{page === 'documentation' && <><h2>{t.placeholder}</h2><p>{t.docsText}</p></>}<div className="intro-actions">{page === 'home' ? <Button asChild size="lg"><a href={href(language, 'demo')}>{t.openDemo}<ArrowRight size={16} /></a></Button> : <Button asChild><a href={href(language, 'home')}>{t.back}<ArrowRight /></a></Button>}{page === 'documentation' && <a className="text-link" href={`${GITHUB}#readme`}>GitHub<ArrowUpRight size={16} /></a>}</div></section>}</main>
    {page !== 'demo' && <footer><a className="brand" href={href(language, 'home')}><img src={`${config.base}logo.svg`} alt="" /><span>Gold Band</span></a><span>AGPL-3.0 · Open source</span><a href={GITHUB}>{t.source}<ArrowUpRight size={14} /></a></footer>}
  </div></TooltipProvider>;
}
