import { lazy, Suspense, useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { WindowCloseCoordinator } from '@/components/WindowCloseCoordinator';
import { loadI18nLanguage } from '@/i18n';
import { applyAppearance } from '@/theme';
import { isTauriRuntime } from '@/api/shared';
import { setupApi } from './api';
import type { SetupApi, SetupStatus } from './types';
import { OnboardingPage } from './OnboardingPage';

const BusinessApp = lazy(() => import('@/App').then(({ App }) => ({ default: App })));
export function OnboardingGate() {
  const { t } = useTranslation();
  const [loaded, setLoaded] = useState<{ api: SetupApi | null; status: SetupStatus | null; onboardingRoute: boolean }>();
  const [failure, setFailure] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [route, setRoute] = useState(location.hash);
  const onboardingRoute = route.startsWith('#/onboarding');
  useEffect(() => {
    if (isTauriRuntime()) void import('@tauri-apps/api/window').then(({ getCurrentWindow }) => getCurrentWindow().show()).catch(() => {});
  }, []);
  useEffect(() => {
    const onHash = () => setRoute(location.hash);
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, []);
  useEffect(() => {
    let live = true;
    setFailure(false);
    void (async () => {
      const api = await setupApi();
      const status = api ? await api.status() : null;
      await loadI18nLanguage(status?.language ?? 'zh-cn');
      if (!live) return;
      if (status) applyAppearance(status.appearance);
      setLoaded({ api, status, onboardingRoute });
    })().catch(() => { if (live) setFailure(true); });
    return () => { live = false; };
  }, [attempt, onboardingRoute]);
  const loading = <div className="flex h-dvh items-center justify-center" role="status"><Loader2 className="size-5 animate-spin" aria-label={t('onboarding.loading')} /></div>;
  let content: ReactNode;
  if (!loaded || loaded.onboardingRoute !== onboardingRoute) {
    content = failure ? <div className="flex h-dvh flex-col items-center justify-center gap-4"><p role="alert">{t('onboarding.loadFailed')}</p><Button onClick={() => setAttempt((v) => v + 1)}>{t('onboarding.retry')}</Button></div> : loading;
  } else if (loaded.api && loaded.status && (loaded.status.state.status === 'pending' || onboardingRoute)) {
    content = <OnboardingPage key={route} api={loaded.api} status={loaded.status} initialStep={loaded.status.state.status !== 'pending' && route === '#/onboarding/defaults' ? 2 : 0} onDone={async () => {
      const status = await loaded.api!.status();
      setLoaded({ ...loaded, status, onboardingRoute: false });
      location.hash = '';
      setRoute('');
    }} onExit={() => { location.hash = '#/settings'; setRoute('#/settings'); }} />;
  } else {
    content = <Suspense fallback={loading}><BusinessApp /></Suspense>;
  }
  return <><WindowCloseCoordinator platform={loaded?.status?.platform} />{content}</>;
}
