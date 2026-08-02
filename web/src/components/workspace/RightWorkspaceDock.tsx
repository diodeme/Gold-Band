import { Bot, ChevronDown, FileText, X } from 'lucide-react';
import { memo, type ReactNode, useLayoutEffect, useRef, useState } from 'react';
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
  const tabStripRef = useRef<HTMLDivElement>(null);
  const overflowMenuRef = useRef<HTMLButtonElement>(null);
  const [tabsOverflowing, setTabsOverflowing] = useState(false);

  useLayoutEffect(() => {
    const tabStrip = tabStripRef.current;
    if (!tabStrip) return;
    const measure = () => {
      const availableWidth = tabStrip.clientWidth + (overflowMenuRef.current?.offsetWidth ?? 0);
      const overflowing = tabStrip.scrollWidth > availableWidth + 1;
      setTabsOverflowing((current) => current === overflowing ? current : overflowing);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(tabStrip);
    for (const tab of tabStrip.children) observer.observe(tab);
    return () => observer.disconnect();
  }, [tabs]);

  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col bg-background" aria-label={t('workspace.rightWorkspace')} data-right-workspace-dock="true">
      <div className="flex h-10 shrink-0 items-center border-b border-border/60 bg-muted/10">
        <div
          ref={tabStripRef}
          className="gold-themed-scrollbar right-workspace-tab-scrollbar flex min-w-0 flex-1 items-center gap-1 overflow-x-auto px-1"
          data-right-workspace-tab-strip="true"
        >
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
        {tabsOverflowing ? (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                ref={overflowMenuRef}
                variant="ghost"
                size="icon"
                className="size-8 shrink-0 rounded-none"
                aria-label={t('workspace.allTabs')}
                data-right-workspace-overflow-menu="true"
              >
                <ChevronDown className="size-3" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-64">
              {tabs.map((tab) => <DropdownMenuItem key={tab.key} onSelect={() => activateTab(tab.key)}>{tab.title}</DropdownMenuItem>)}
            </DropdownMenuContent>
          </DropdownMenu>
        ) : null}
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
      data-right-workspace-tab="true"
      data-state={active ? 'active' : 'inactive'}
      className={cn(
        'group relative flex h-8 min-w-36 max-w-56 shrink-0 items-center gap-2 rounded-xl px-2.5 text-xs transition-colors',
        active
          ? 'bg-muted/70 text-foreground'
          : 'text-muted-foreground hover:bg-muted/35 hover:text-foreground',
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
        className={cn(
          'size-6 shrink-0 rounded-lg transition-opacity hover:bg-background/70',
          active
            ? 'opacity-60 hover:opacity-100'
            : 'opacity-0 group-hover:opacity-60 focus-visible:opacity-100',
        )}
        onClick={() => onClose(tab.key)}
        aria-label={t('workspace.closeTab')}
      >
        <X className="size-3" />
      </Button>
    </div>
  );
}
