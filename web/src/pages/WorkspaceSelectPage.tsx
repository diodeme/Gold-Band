import { FolderOpen, Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { AppBootstrapVm, AppInfoVm } from '../types';
import { AppCard } from '@/components/AppCard';
import { EmptyState, Page } from '@/components/PageScaffold';
import { Button } from '@/components/ui/button';
import { CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { canRemoveRecentWorkspace } from '@/lib/workspace-picker-scope';

interface WorkspaceSelectPageProps {
  bootstrap: AppBootstrapVm | null;
  appInfo: AppInfoVm;
  busy: boolean;
  onChooseWorkspace: () => void;
  onSelectRecentWorkspace: (workspace: string) => void;
  onRemoveRecentWorkspace: (workspace: string) => void;
}

export function WorkspaceSelectPage({ bootstrap, appInfo, busy, onChooseWorkspace, onSelectRecentWorkspace, onRemoveRecentWorkspace }: WorkspaceSelectPageProps) {
  const { t } = useTranslation();
  const recent = bootstrap?.recentWorkspaces ?? [];
  const currentWorkspace = bootstrap?.repoRoot ?? null;

  return (
    <TooltipProvider>
    <Page className="grid grid-cols-1 gap-4 overflow-y-auto p-4 sm:p-6 lg:grid-cols-[minmax(0,0.95fr)_minmax(360px,0.55fr)] lg:gap-6 xl:p-8">
      <AppCard className="justify-center overflow-hidden border-primary/20 bg-[radial-gradient(circle_at_top_left,rgba(245,158,11,0.18),transparent_36%),var(--card)]">
        <CardContent className="max-w-2xl space-y-7 px-8 py-10">
          <span className="grid h-16 w-24 place-items-center rounded-2xl bg-sidebar-accent/60 p-2 ring-1 ring-primary/20">
            <img src="/logo.svg" alt="" className="h-full w-full object-contain" />
          </span>
          <div className="space-y-3">
            <p className="text-xs font-semibold uppercase tracking-[0.24em] text-primary">{t('workspaceSelect.product', { appName: appInfo.appName })}</p>
            <h1 className="text-4xl font-semibold tracking-tight">{t('common.selectWorkspace')}</h1>
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
        </CardHeader>
        <CardContent className="min-h-0 px-0 py-0">
          {recent.length === 0 ? <div className="p-3"><EmptyState>{t('workspaceSelect.emptyRecent')}</EmptyState></div> : null}
          <ScrollArea className="h-72 lg:h-[calc(100vh-190px)]">
            <div className="space-y-2 p-3">
              {recent.map((workspace) => {
                const allowRecentRemoval = canRemoveRecentWorkspace(recent.length, workspace, currentWorkspace);
                const removeTooltip = workspace === currentWorkspace
                  ? t('workspaceSelect.currentWorkspaceLocked')
                  : allowRecentRemoval
                    ? t('workspaceSelect.removeRecent')
                    : t('workspaceSelect.keepOneRecent');
                return (
                  <div className="flex items-center gap-2 rounded-md border bg-background p-2 shadow-xs transition-colors hover:bg-accent/40" key={workspace}>
                    <Button className="h-auto min-w-0 flex-1 justify-between gap-4 border-0 bg-transparent p-2 text-left shadow-none hover:bg-transparent" variant="outline" onClick={() => onSelectRecentWorkspace(workspace)} disabled={busy}>
                      <span className="truncate text-xs text-muted-foreground">{workspace}</span>
                      <small className="shrink-0 text-primary">{t('common.open')}</small>
                    </Button>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="inline-flex shrink-0">
                          <Button
                            type="button"
                            size="icon-sm"
                            variant="ghost"
                            className="text-muted-foreground hover:text-destructive"
                            aria-label={t('workspaceSelect.removeRecent')}
                            disabled={busy || !allowRecentRemoval}
                            onClick={() => onRemoveRecentWorkspace(workspace)}
                          >
                            <Trash2 className="size-4" />
                          </Button>
                        </span>
                      </TooltipTrigger>
                      <TooltipContent>{removeTooltip}</TooltipContent>
                    </Tooltip>
                  </div>
                );
              })}
            </div>
          </ScrollArea>
        </CardContent>
      </AppCard>
    </Page>
    </TooltipProvider>
  );
}
