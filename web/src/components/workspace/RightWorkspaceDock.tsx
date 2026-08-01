import { Bot, ChevronDown, FileText, X } from 'lucide-react';
import { memo, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import { useConversationBranchLiveSnapshot } from '@/lib/conversation-event-router';
import { AgentConversationPanel } from './AgentConversationPanel';
import { useRightWorkspace, type RightWorkspaceResource } from './right-workspace-context';

export function RightWorkspaceDock() {
  const { t } = useTranslation();
  const { tabs, activeTabKey, activateTab, closeTab, closeWorkspace } = useRightWorkspace();
  const active = tabs.find((tab) => tab.key === activeTabKey) ?? null;
  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col bg-background" aria-label={t('workspace.rightWorkspace')} data-right-workspace-dock="true">
      <div className="flex h-10 shrink-0 items-center border-b border-border/60 bg-muted/10">
        <div className="themed-scrollbar flex min-w-0 flex-1 items-stretch overflow-x-auto">
          {tabs.map((tab) => (
            <RightWorkspaceTab
              key={tab.key}
              tab={tab}
              active={tab.key === activeTabKey}
              onActivate={activateTab}
              onClose={closeTab}
            />
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

const RightWorkspaceTab = memo(function RightWorkspaceTab({
  tab,
  active,
  onActivate,
  onClose,
}: {
  tab: RightWorkspaceResource;
  active: boolean;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
}) {
  if (tab.kind === 'agent-transcript') {
    return <AgentWorkspaceTab tab={tab} active={active} onActivate={onActivate} onClose={onClose} />;
  }
  return (
    <RightWorkspaceTabButton
      tab={tab}
      active={active}
      attention={tab.attention}
      icon={<FileText className="size-3.5 shrink-0" />}
      onActivate={onActivate}
      onClose={onClose}
    />
  );
});

const AgentWorkspaceTab = memo(function AgentWorkspaceTab({
  tab,
  active,
  onActivate,
  onClose,
}: {
  tab: Extract<RightWorkspaceResource, { kind: 'agent-transcript' }>;
  active: boolean;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
}) {
  const live = useConversationBranchLiveSnapshot(tab.locator, tab.locator.branchId);
  const attention = live.revision > 0 ? live.attention : tab.attention;
  return (
    <RightWorkspaceTabButton
      tab={tab}
      active={active}
      attention={attention}
      icon={<Bot className="size-3.5 shrink-0" />}
      onActivate={onActivate}
      onClose={onClose}
    />
  );
});

function RightWorkspaceTabButton({
  tab,
  active,
  attention,
  icon,
  onActivate,
  onClose,
}: {
  tab: RightWorkspaceResource;
  active: boolean;
  attention: boolean;
  icon: ReactNode;
  onActivate: (key: string) => void;
  onClose: (key: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      className={cn(
        'group relative flex min-w-36 max-w-56 shrink-0 items-center gap-2 border-r border-border/50 px-3 text-xs text-muted-foreground hover:bg-muted/30 hover:text-foreground',
        active && 'bg-background text-foreground after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:bg-primary',
      )}
    >
      <button
        type="button"
        className="flex min-w-0 flex-1 items-center gap-2 self-stretch text-left outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        onClick={() => onActivate(tab.key)}
      >
        {icon}
        <span className="min-w-0 flex-1 truncate">{tab.title}</span>
        {attention ? <span className="size-1.5 shrink-0 rounded-full bg-amber-500" aria-label={t('workspace.attention')} /> : null}
      </button>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="size-6 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
        onClick={() => onClose(tab.key)}
        aria-label={t('workspace.closeTab')}
      >
        <X className="size-3" />
      </Button>
    </div>
  );
}
