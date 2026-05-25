import { FolderOpen } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AppBootstrapVm, AppInfoVm } from '../types';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Page } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';

interface WorkspaceSelectPageProps {
  bootstrap: AppBootstrapVm | null;
  appInfo: AppInfoVm;
  busy: boolean;
  onChooseWorkspace: () => void;
  onSelectRecentWorkspace: (workspace: string) => void;
}

export function WorkspaceSelectPage({ bootstrap, appInfo, busy, onChooseWorkspace, onSelectRecentWorkspace }: WorkspaceSelectPageProps) {
  const { t } = useTranslation();
  const recent = bootstrap?.recentWorkspaces ?? [];

  return (
    <Page className="grid grid-cols-[minmax(0,0.95fr)_minmax(360px,0.55fr)] gap-6 p-8">
      <AppCard className="justify-center overflow-hidden border-primary/20 bg-[radial-gradient(circle_at_top_left,rgba(245,158,11,0.18),transparent_36%),var(--card)]">
        <CardContent className="max-w-2xl space-y-7 px-8 py-10">
          <span className="grid h-16 w-24 place-items-center rounded-2xl bg-sidebar-accent/60 p-2 ring-1 ring-primary/20">
            <img src="/logo.svg" alt="" className="h-full w-full object-contain" />
          </span>
          <div className="space-y-3">
            <p className="text-xs font-semibold uppercase tracking-[0.24em] text-primary">{t('workspaceSelect.product', { appName: appInfo.appName })}</p>
            <h1 className="text-4xl font-semibold tracking-tight">{t('common.selectWorkspace')}</h1>
            <p className="text-sm leading-7 text-muted-foreground">{t('workspaceSelect.description', { configDirName: appInfo.configDirName })}</p>
          </div>
          <Button size="lg" disabled={busy} onClick={onChooseWorkspace}>
            <FolderOpen />
            {t('common.selectWorkspace')}
          </Button>
        </CardContent>
      </AppCard>

      <AppCard className="min-h-0 gap-0 py-0">
        <CardHeader className="border-b px-5 py-3 !pb-3">
          <CardTitle>{t('common.recentWorkspaces')}</CardTitle>
          <CardDescription>{t('workspaceSelect.recentDescription')}</CardDescription>
        </CardHeader>
        <CardContent className="min-h-0 px-0 py-0">
          {recent.length === 0 ? <div className="p-3"><EmptyState>{t('workspaceSelect.emptyRecent')}</EmptyState></div> : null}
          <ScrollArea className="h-[calc(100vh-190px)]">
            <div className="space-y-2 p-3">
              {recent.map((workspace) => (
                <Button className="h-auto w-full justify-between gap-4 p-4" variant="outline" key={workspace} onClick={() => onSelectRecentWorkspace(workspace)} disabled={busy}>
                  <span className="truncate text-xs text-muted-foreground">{workspace}</span>
                  <small className="shrink-0 text-primary">{t('common.open')}</small>
                </Button>
              ))}
            </div>
          </ScrollArea>
        </CardContent>
      </AppCard>
    </Page>
  );
}
