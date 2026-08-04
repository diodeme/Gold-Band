import { Bot, Braces, ChevronDown, FileCode2, FileDiff, FileText, FolderOpen, GitBranch, PencilLine, Plus, X } from 'lucide-react';
import { memo, type ReactNode, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from '@/components/ui/dropdown-menu';
import { cn } from '@/lib/utils';
import { useConversationBranchLiveSnapshot } from '@/lib/conversation-event-router';
import { AgentConversationPanel } from './AgentConversationPanel';
import { fileBrowserWorkspaceResourceKey, useRightWorkspace, type RightWorkspaceResource } from './right-workspace-context';
import { useFileContentEntry } from './files/file-content-store';

export const RightWorkspaceDock = memo(function RightWorkspaceDock() {
  const { t } = useTranslation();
  const { tabs, activeTabKey, activateTab, closeTab, renderResource } = useRightWorkspace();
  const active = tabs.find((tab) => tab.key === activeTabKey) ?? null;
  const tabStripRef = useRef<HTMLDivElement>(null);
  const overflowMenuRef = useRef<HTMLButtonElement>(null);
  const resizeFrameRef = useRef<number | null>(null);
  const [tabsOverflowing, setTabsOverflowing] = useState(false);

  useLayoutEffect(() => {
    if (!tabStripRef.current) return;
    const measure = () => {
      const tabStrip = tabStripRef.current;
      if (!tabStrip) return;
      const availableWidth = tabStrip.clientWidth + (overflowMenuRef.current?.offsetWidth ?? 0);
      const overflowing = tabStrip.scrollWidth > availableWidth + 1;
      setTabsOverflowing((current) => current === overflowing ? current : overflowing);
    };
    const scheduleMeasure = () => {
      if (resizeFrameRef.current !== null) return;
      resizeFrameRef.current = requestAnimationFrame(() => {
        resizeFrameRef.current = null;
        measure();
      });
    };
    measure();
    const observer = new ResizeObserver(scheduleMeasure);
    observer.observe(tabStripRef.current);
    return () => {
      observer.disconnect();
      if (resizeFrameRef.current !== null) cancelAnimationFrame(resizeFrameRef.current);
      resizeFrameRef.current = null;
    };
  }, [tabs]);

  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col bg-background" aria-label={t('workspace.rightWorkspace')} data-right-workspace-dock="true">
      {tabs.length > 0 ? <div className="flex h-10 shrink-0 items-center border-b border-border/60 bg-muted/10">
        <WorkspaceEntryOptions presentation="menu" />
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
      </div> : null}
      <div className="flex min-h-0 flex-1 flex-col">
        {active?.kind === 'agent-transcript' ? <AgentConversationPanel key={active.key} resource={active} /> : null}
        {active && active.kind !== 'agent-transcript' ? renderResource(active) : null}
        {!active ? (
          <div className="flex min-h-0 flex-1 flex-col p-3" data-right-workspace-empty="true">
            <WorkspaceEntryOptions presentation="empty" />
          </div>
        ) : null}
      </div>
    </section>
  );
});

type WorkspaceEntryOption = {
  id: 'file-browser';
  label: string;
  description: string;
  icon: typeof FolderOpen;
  open: () => void;
};

function WorkspaceEntryOptions({ presentation }: { presentation: 'empty' | 'menu' }) {
  const { t } = useTranslation();
  const { openResource, projectId, scopeKey } = useRightWorkspace();
  const options = useMemo<WorkspaceEntryOption[]>(() => {
    if (!projectId || !scopeKey) return [];
    return [{
      id: 'file-browser',
      label: t('workspace.files'),
      description: t('workspace.browseWorkspaceFiles'),
      icon: FolderOpen,
      open: () => {
        void openResource({
          kind: 'file-browser',
          key: fileBrowserWorkspaceResourceKey(projectId),
          scopeKey,
          projectId,
          title: t('workspace.files'),
          description: t('workspace.browseWorkspaceFiles'),
          attention: false,
        });
      },
    }];
  }, [openResource, projectId, scopeKey, t]);

  if (presentation === 'menu') {
    return (
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="ml-1 size-8 shrink-0 rounded-lg"
            disabled={options.length === 0}
            aria-label={t('workspace.openNewTab')}
            data-right-workspace-new-tab-menu="true"
          >
            <Plus className="size-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-56">
          {options.map((option) => {
            const Icon = option.icon;
            return (
              <DropdownMenuItem
                key={option.id}
                className="min-h-9 px-2 py-1.5 text-xs"
                data-right-workspace-entry-option={option.id}
                onSelect={option.open}
              >
                <Icon className="size-3.5" />
                <span className="min-w-0">
                  <span className="block font-medium">{option.label}</span>
                  <span className="block truncate text-[10px] text-muted-foreground">{option.description}</span>
                </span>
              </DropdownMenuItem>
            );
          })}
        </DropdownMenuContent>
      </DropdownMenu>
    );
  }

  return (
    <div data-right-workspace-entry-options="empty">
      {options.map((option) => {
        const Icon = option.icon;
        return (
          <Button
            key={option.id}
            type="button"
            variant="ghost"
            className="h-auto justify-start gap-3 rounded-xl px-3 py-3 text-left"
            onClick={option.open}
          >
            <Icon className="size-4 shrink-0 text-primary" />
            <span className="min-w-0">
              <span className="block text-sm font-medium text-foreground">{option.label}</span>
              <span className="mt-0.5 block text-xs font-normal text-muted-foreground">{option.description}</span>
            </span>
          </Button>
        );
      })}
    </div>
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
  const fileEntry = useFileContentEntry(tab.kind === 'file' ? tab.key : '');
  if (tab.kind === 'agent-transcript') {
    return <AgentWorkspaceTab tab={tab} active={active} onActivate={onActivate} onClose={onClose} />;
  }
  const icon = tab.kind === 'workflow-view'
    ? <GitBranch className="size-3.5 shrink-0" />
    : tab.kind === 'workflow-edit'
      ? <PencilLine className="size-3.5 shrink-0" />
      : tab.kind === 'system-prompt'
        ? <FileCode2 className="size-3.5 shrink-0" />
        : tab.kind === 'raw-frames'
          ? <Braces className="size-3.5 shrink-0" />
          : tab.kind === 'file-browser'
            ? <FolderOpen className="size-3.5 shrink-0" />
            : tab.kind === 'file-diff'
              ? <FileDiff className="size-3.5 shrink-0" />
            : <FileText className="size-3.5 shrink-0" />;
  return (
    <RightWorkspaceTabButton
      tab={tab}
      active={active}
      attention={tab.attention || (tab.kind === 'file' && (
        fileEntry.status === 'error'
        || fileEntry.saveState.kind === 'error'
        || fileEntry.saveState.kind === 'conflict'
      ))}
      icon={icon}
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
