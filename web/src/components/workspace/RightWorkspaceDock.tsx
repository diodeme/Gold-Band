import { Bot, ChevronDown, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import { AgentConversationPanel } from './AgentConversationPanel';
import { useRightWorkspace } from './right-workspace-context';

export function RightWorkspaceDock() {
  const { t } = useTranslation();
  const { tabs, activeTabKey, activateTab, closeTab, closeWorkspace } = useRightWorkspace();
  const active = tabs.find((tab) => tab.key === activeTabKey) ?? null;
  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col bg-background" aria-label={t('workspace.rightWorkspace')}>
      <div className="flex h-10 shrink-0 items-center border-b border-border/60 bg-muted/10">
        <div className="themed-scrollbar flex min-w-0 flex-1 items-stretch overflow-x-auto">
          {tabs.map((tab) => (
            <button
              key={tab.key}
              type="button"
              className={cn(
                'group relative flex min-w-36 max-w-56 shrink-0 items-center gap-2 border-r border-border/50 px-3 text-xs text-muted-foreground hover:bg-muted/30 hover:text-foreground',
                tab.key === activeTabKey && 'bg-background text-foreground after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:bg-primary',
              )}
              onClick={() => activateTab(tab.key)}
            >
              <Bot className="size-3.5 shrink-0" />
              <span className="min-w-0 flex-1 truncate text-left">{tab.title}</span>
              {tab.attention ? <span className="size-1.5 shrink-0 rounded-full bg-amber-500" aria-label={t('workspace.attention')} /> : null}
              <span
                role="button"
                tabIndex={0}
                className="rounded p-0.5 opacity-0 hover:bg-muted group-hover:opacity-100"
                onClick={(event) => { event.stopPropagation(); closeTab(tab.key); }}
                onKeyDown={(event) => { if (event.key === 'Enter' || event.key === ' ') closeTab(tab.key); }}
                aria-label={t('workspace.closeTab')}
              >
                <X className="size-3" />
              </span>
            </button>
          ))}
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon" className="size-9 shrink-0 rounded-none" aria-label={t('workspace.allTabs')}>
              <ChevronDown className="size-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-64">
            {tabs.map((tab) => <DropdownMenuItem key={tab.key} onSelect={() => activateTab(tab.key)}>{tab.title}</DropdownMenuItem>)}
          </DropdownMenuContent>
        </DropdownMenu>
        <Button variant="ghost" size="icon" className="size-9 shrink-0 rounded-none" onClick={closeWorkspace} aria-label={t('workspace.closeWorkspace')}>
          <X className="size-4" />
        </Button>
      </div>
      <div className="flex min-h-0 flex-1 flex-col">
        {active?.kind === 'agent-transcript' ? <AgentConversationPanel key={active.key} resource={active} /> : null}
      </div>
    </section>
  );
}
