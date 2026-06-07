import { Bot, Boxes, ChevronsUpDown, Command, MessageSquare, PanelLeft, Settings } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { ConversationPage, ConversationSidebarVm, DesktopUiMode, PrimaryModule } from '../types';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { ConversationShell } from '@/components/conversation/ConversationShell';
import { cn } from '@/lib/utils';

interface ShellProps {
  uiMode: DesktopUiMode;
  active: PrimaryModule;
  conversationPage: ConversationPage;
  conversationSidebar: ConversationSidebarVm;
  appName: string;
  repoRoot?: string;
  needsWorkspace?: boolean;
  showSettingsUpdateDot?: boolean;
  onSelect: (module: PrimaryModule) => void;
  onSelectConversation: (page: ConversationPage) => void;
  onToggleUiMode: () => void;
  onChooseWorkspace: () => void;
  onConversationNew: () => void;
  onConversationSearch: () => void;
  onConversationSelectTask: (projectId: string, taskId: string) => void;
  onConversationSelectRun: (projectId: string, taskId: string, runId: string) => void;
  onConversationRenameTask: (projectId: string, taskId: string, title: string) => void;
  onConversationPinTask: (projectId: string, taskId: string) => void;
  onConversationUnpinTask: (projectId: string, taskId: string) => void;
  onConversationNewInWorkspace?: (projectId: string) => void;
  onConversationAddWorkspace?: () => void;
  onConversationRemoveWorkspace?: (projectId: string) => void;
  children: React.ReactNode;
}

export function Shell({ uiMode, active, conversationPage, conversationSidebar, appName, repoRoot, needsWorkspace, showSettingsUpdateDot = false, onSelect, onSelectConversation, onToggleUiMode, onChooseWorkspace, onConversationNew, onConversationSearch, onConversationSelectTask, onConversationSelectRun, onConversationRenameTask, onConversationPinTask, onConversationUnpinTask, onConversationNewInWorkspace, onConversationAddWorkspace, onConversationRemoveWorkspace, children }: ShellProps) {
  if (uiMode === 'conversation') {
    return (
      <ConversationShell
        vm={conversationSidebar}
        active={conversationPage}
        onSelect={onSelectConversation}
        onToggleUiMode={onToggleUiMode}
        onNewConversation={onConversationNew}
        onSearch={onConversationSearch}
        onSelectTask={onConversationSelectTask}
        onSelectRun={onConversationSelectRun}
        onPinTask={onConversationPinTask}
        onUnpinTask={onConversationUnpinTask}
        onRenameTask={onConversationRenameTask}
        onNewConversationInWorkspace={onConversationNewInWorkspace}
        onAddWorkspace={onConversationAddWorkspace}
        onRemoveWorkspace={onConversationRemoveWorkspace}
      >
        {children}
      </ConversationShell>
    );
  }
  return (
    <WorkbenchShell
      active={active}
      appName={appName}
      repoRoot={repoRoot}
      needsWorkspace={needsWorkspace}
      showSettingsUpdateDot={showSettingsUpdateDot}
      onSelect={onSelect}
      onToggleUiMode={onToggleUiMode}
      onChooseWorkspace={onChooseWorkspace}
    >
      {children}
    </WorkbenchShell>
  );
}

// ── WorkbenchShell ──

interface WorkbenchShellProps {
  active: PrimaryModule;
  appName: string;
  repoRoot?: string;
  needsWorkspace?: boolean;
  showSettingsUpdateDot?: boolean;
  onSelect: (module: PrimaryModule) => void;
  onToggleUiMode: () => void;
  onChooseWorkspace: () => void;
  children: React.ReactNode;
}

function WorkbenchShell({ active, appName, repoRoot, needsWorkspace, showSettingsUpdateDot = false, onSelect, onToggleUiMode, onChooseWorkspace, children }: WorkbenchShellProps) {
  const { t } = useTranslation();
  return (
    <TooltipProvider>
      <div className="grid h-screen grid-cols-[256px_minmax(0,1fr)] bg-gold-workspace text-foreground" onContextMenu={(event) => event.preventDefault()}>
        <aside className="flex min-h-0 flex-col gap-5 border-r bg-sidebar px-5 py-7 text-sidebar-foreground">
          <Button variant="ghost" className="h-auto justify-start gap-3 px-0 py-0 hover:bg-transparent" asChild>
            <a href="/tasks" onClick={(event) => handleNavLinkClick(event, () => onSelect('task-orchestration'))}>
              <span className="grid h-9 w-14 place-items-center rounded-lg border border-sidebar-border bg-sidebar-accent/60 p-1">
                <img src="/logo.svg" alt="" className="h-full w-full object-contain" />
              </span>
              <span className="text-left">
                <strong className="block text-xl leading-none text-primary">{appName}</strong>
                <small className="mt-1 block text-[11px] uppercase tracking-[0.18em] text-muted-foreground">AI Orchestrator</small>
              </span>
            </a>
          </Button>

          <Tooltip>
            <TooltipTrigger asChild>
              <Button variant="outline" className="h-auto justify-between gap-3 border-sidebar-border bg-transparent p-3 text-left hover:bg-sidebar-accent" onClick={onChooseWorkspace}>
                <span className="min-w-0">
                  <span className="block truncate text-xs text-muted-foreground">{needsWorkspace ? t('common.workspace') : (repoRoot ?? t('common.workspace'))}</span>
                  <small className="mt-1 block text-xs font-semibold text-primary">{t('common.selectWorkspace')}</small>
                </span>
                <ChevronsUpDown className="size-4 shrink-0 text-muted-foreground" />
              </Button>
            </TooltipTrigger>
            <TooltipContent className="max-w-[360px] whitespace-pre-wrap break-words" sideOffset={6}>{needsWorkspace ? t('common.selectWorkspace') : (repoRoot ?? t('common.switchWorkspace'))}</TooltipContent>
          </Tooltip>

          <nav className="mt-6 flex flex-1 flex-col gap-2">
            <ShellNavButton active={active === 'task-orchestration'} href="/tasks" icon={<Command />} label={t('common.taskOrchestration')} onClick={() => onSelect('task-orchestration')} />
            <ShellNavButton active={active === 'agent-management'} href="/agents" icon={<Bot />} label={t('common.agentManagement')} onClick={() => onSelect('agent-management')} />
            <ShellNavButton active={active === 'knowledge-base'} href="/contexts" icon={<Boxes />} label={t('common.contextManagement')} onClick={() => onSelect('knowledge-base')} />
          </nav>

          <div className="flex items-center gap-2 rounded-lg border border-sidebar-border px-3 py-2">
            <MessageSquare className="size-4 text-muted-foreground" />
            <span className="text-xs text-muted-foreground">{t('conversation.toggleToConversation')}</span>
            <Button variant="ghost" size="icon" className="ml-auto size-7" onClick={onToggleUiMode} aria-label={t('conversation.toggleToConversation')}>
              <PanelLeft className="size-4" />
            </Button>
          </div>

          <Separator />
          <ShellNavButton active={active === 'settings'} href="/settings" icon={<Settings />} label={t('common.settings')} trailing={showSettingsUpdateDot ? <UpdateDot /> : null} onClick={() => onSelect('settings')} />
        </aside>
        <main className="relative flex min-w-0 flex-col overflow-hidden bg-gold-workspace">{children}</main>
      </div>
    </TooltipProvider>
  );
}

// ── Shared helpers ──

function handleNavLinkClick(event: React.MouseEvent<HTMLAnchorElement>, onClick?: () => void) {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  onClick?.();
}

function ShellNavButton({ active, disabled, href, icon, label, trailing, onClick }: { active?: boolean; disabled?: boolean; href?: string; icon: React.ReactNode; label: string; trailing?: React.ReactNode; onClick?: () => void }) {
  const className = cn(
    'h-10 justify-between rounded-lg px-3 text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
    active && 'bg-sidebar-accent text-sidebar-primary',
  );
  const content = (
    <>
      <span className="flex items-center gap-3">
        <span className="[&_svg]:size-4">{icon}</span>
        <span className="text-sm">{label}</span>
      </span>
      {trailing ? <span className="flex items-center text-xs">{trailing}</span> : null}
    </>
  );
  const button = href && !disabled ? (
    <Button variant="ghost" className={className} asChild>
      <a href={href} onClick={(event) => handleNavLinkClick(event, onClick)}>{content}</a>
    </Button>
  ) : (
    <Button variant="ghost" disabled={disabled} className={className} onClick={onClick}>{content}</Button>
  );

  if (!disabled) return button;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{button}</TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function UpdateDot() {
  return <span className="size-2 rounded-full bg-destructive" aria-hidden="true" />;
}
