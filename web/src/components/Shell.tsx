import { Bot, Boxes, BrainCircuit, ChevronsUpDown, Command, Settings } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { PrimaryModule } from '../types';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface ShellProps {
  active: PrimaryModule;
  repoRoot?: string;
  onSelect: (module: PrimaryModule) => void;
  onChooseWorkspace: () => void;
  children: React.ReactNode;
}

export function Shell({ active, repoRoot, onSelect, onChooseWorkspace, children }: ShellProps) {
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
                <strong className="block text-xl leading-none text-primary">Gold Band</strong>
                <small className="mt-1 block text-[11px] uppercase tracking-[0.18em] text-muted-foreground">AI Orchestrator</small>
              </span>
            </a>
          </Button>

          <Button variant="outline" className="h-auto justify-between gap-3 border-sidebar-border bg-transparent p-3 text-left hover:bg-sidebar-accent" onClick={onChooseWorkspace} title={repoRoot ?? t('common.switchWorkspace')}>
            <span className="min-w-0">
              <span className="block truncate text-xs text-muted-foreground">{repoRoot ?? t('common.workspace')}</span>
              <small className="mt-1 block text-xs font-semibold text-primary">{t('common.switchWorkspace')}</small>
            </span>
            <ChevronsUpDown className="size-4 shrink-0 text-muted-foreground" />
          </Button>

          <nav className="mt-6 flex flex-1 flex-col gap-2">
            <ShellNavButton active={active === 'task-orchestration'} href="/tasks" icon={<Command />} label={t('common.taskOrchestration')} onClick={() => onSelect('task-orchestration')} />
            <ShellNavButton active={active === 'agent-management'} href="/agents" icon={<Bot />} label={t('common.agentManagement')} onClick={() => onSelect('agent-management')} />
            <ShellNavButton active={active === 'knowledge-base'} href="/contexts" icon={<Boxes />} label={t('common.contextManagement')} onClick={() => onSelect('knowledge-base')} />
            <ShellNavButton disabled icon={<BrainCircuit />} label={t('common.modelManagement')} suffix={t('common.comingSoon')} />
          </nav>

          <Separator />
          <ShellNavButton active={active === 'settings'} href="/settings" icon={<Settings />} label={t('common.settings')} onClick={() => onSelect('settings')} />
        </aside>
        <main className="flex min-w-0 flex-col overflow-hidden bg-gold-workspace">{children}</main>
      </div>
    </TooltipProvider>
  );
}

function handleNavLinkClick(event: React.MouseEvent<HTMLAnchorElement>, onClick?: () => void) {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  onClick?.();
}

function ShellNavButton({ active, disabled, href, icon, label, suffix, onClick }: { active?: boolean; disabled?: boolean; href?: string; icon: React.ReactNode; label: string; suffix?: string; onClick?: () => void }) {
  const className = cn(
    'h-12 justify-between rounded-lg px-3 text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground',
    active && 'bg-sidebar-accent text-sidebar-primary',
  );
  const content = (
    <>
      <span className="flex items-center gap-3">
        <span className="[&_svg]:size-5">{icon}</span>
        <span>{label}</span>
      </span>
      {suffix ? <span className="text-xs">{suffix}</span> : null}
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
      <TooltipContent>{suffix}</TooltipContent>
    </Tooltip>
  );
}
