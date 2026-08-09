import type { ReactNode } from 'react';
import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import { useTwoRowAgentOverflow } from '@/hooks/use-two-row-agent-overflow';
import type { ConfiguredSkillAgentMeta, SkillAgentDisplayMeta } from '@/lib/skill-agent-display';
import { cn } from '@/lib/utils';

interface SkillAgentOverflowProps {
  sourceAgents: SkillAgentDisplayMeta[];
  syncAgents: ConfiguredSkillAgentMeta[];
  syncedAgentTypes: Set<string>;
  isPending: (agentType: string) => boolean;
  onToggleAgent: (agentType: string) => void;
}

export function SkillAgentOverflow({
  sourceAgents,
  syncAgents,
  syncedAgentTypes,
  isPending,
  onToggleAgent,
}: SkillAgentOverflowProps) {
  const { t } = useTranslation();
  const { containerRef, layout } = useTwoRowAgentOverflow(sourceAgents.length, syncAgents.length);

  const visibleSourceAgents = sourceAgents.slice(0, layout.visibleSourceCount);
  const visibleSyncAgents = syncAgents.slice(0, layout.visibleSyncCount);
  const hiddenSourceAgents = sourceAgents.slice(layout.visibleSourceCount);
  const hiddenSyncAgents = syncAgents.slice(layout.visibleSyncCount);

  return (
    <div
      ref={containerRef}
      className="flex min-w-0 flex-1 flex-wrap content-center items-center gap-0.5 overflow-hidden"
      data-testid="skill-agent-overflow"
    >
      {visibleSourceAgents.map((agent) => (
        <SourceAgentIcon key={agent.agentType} agent={agent} />
      ))}
      {visibleSyncAgents.map((agent, index) => (
        <div key={agent.agentType} className={cn('flex shrink-0 items-center', index === 0 && visibleSourceAgents.length > 0 && 'gap-1.5')}>
          {index === 0 && visibleSourceAgents.length > 0 ? <span className="h-4 w-px shrink-0 bg-border/70" aria-hidden="true" /> : null}
          <SyncAgentIcon
            agent={agent}
            isSynced={syncedAgentTypes.has(agent.agentType)}
            isPending={isPending(agent.agentType)}
            onToggleAgent={onToggleAgent}
          />
        </div>
      ))}
      {layout.hiddenCount > 0 ? (
        <Popover>
          <TooltipProvider delayDuration={300}>
            <Tooltip>
              <TooltipTrigger asChild>
                <PopoverTrigger asChild>
                  <Button
                    type="button"
                    size="icon"
                    variant="secondary"
                    className="h-6 w-7 min-w-7 shrink-0 rounded-full p-0 text-[10px] font-medium tabular-nums"
                    aria-label={t('contextManagement.skills.moreAgents', { count: layout.hiddenCount, defaultValue: `还有 ${layout.hiddenCount} 个 Agent` })}
                  >
                    +{layout.hiddenCount}
                  </Button>
                </PopoverTrigger>
              </TooltipTrigger>
              <TooltipContent side="top">
                {t('contextManagement.skills.moreAgents', { count: layout.hiddenCount, defaultValue: `还有 ${layout.hiddenCount} 个 Agent` })}
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
          <PopoverContent side="top" align="start" sideOffset={8} className="w-64 p-2">
            <ScrollArea className="max-h-72">
              <div className="space-y-3 pr-1">
                {hiddenSourceAgents.length > 0 ? (
                  <AgentGroup title={t('contextManagement.skills.sourceAgents', '直接读取')}>
                    {hiddenSourceAgents.map((agent) => (
                      <div key={agent.agentType} className="flex h-8 items-center gap-2 rounded-md px-2 text-xs text-muted-foreground">
                        <AgentImage agent={agent} />
                        <span className="truncate">{agent.label}</span>
                      </div>
                    ))}
                  </AgentGroup>
                ) : null}
                {hiddenSyncAgents.length > 0 ? (
                  <AgentGroup title={t('contextManagement.skills.syncableAgents', '同步设置')}>
                    {hiddenSyncAgents.map((agent) => (
                      <SyncAgentRow
                        key={agent.agentType}
                        agent={agent}
                        isSynced={syncedAgentTypes.has(agent.agentType)}
                        isPending={isPending(agent.agentType)}
                        onToggleAgent={onToggleAgent}
                      />
                    ))}
                  </AgentGroup>
                ) : null}
              </div>
            </ScrollArea>
          </PopoverContent>
        </Popover>
      ) : null}
    </div>
  );
}

function AgentGroup({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <div className="px-2 pb-1 text-[10px] font-medium uppercase tracking-wide text-muted-foreground">{title}</div>
      <div className="space-y-0.5">{children}</div>
    </section>
  );
}

function SourceAgentIcon({ agent }: { agent: SkillAgentDisplayMeta }) {
  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="flex size-6 shrink-0 items-center justify-center rounded-full">
            <AgentImage agent={agent} />
          </span>
        </TooltipTrigger>
        <TooltipContent side="top">{agent.label}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function SyncAgentIcon({
  agent,
  isSynced,
  isPending,
  onToggleAgent,
}: {
  agent: ConfiguredSkillAgentMeta;
  isSynced: boolean;
  isPending: boolean;
  onToggleAgent: (agentType: string) => void;
}) {
  const { t } = useTranslation();
  const actionLabel = isSynced
    ? t('contextManagement.skills.unsyncAgent', { agent: agent.label, defaultValue: `取消同步 ${agent.label}` })
    : t('contextManagement.skills.syncAgent', { agent: agent.label, defaultValue: `同步到 ${agent.label}` });
  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            size="icon"
            variant="ghost"
            className="relative size-6 rounded-full hover:bg-muted"
            disabled={isPending}
            aria-label={actionLabel}
            onClick={() => onToggleAgent(agent.agentType)}
          >
            {isPending ? <Loader2 className="size-3.5 animate-spin text-muted-foreground" /> : <AgentSyncImage agent={agent} isSynced={isSynced} />}
          </Button>
        </TooltipTrigger>
        <TooltipContent side="top">{actionLabel}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}

function SyncAgentRow({
  agent,
  isSynced,
  isPending,
  onToggleAgent,
}: {
  agent: ConfiguredSkillAgentMeta;
  isSynced: boolean;
  isPending: boolean;
  onToggleAgent: (agentType: string) => void;
}) {
  const { t } = useTranslation();
  const actionLabel = isSynced
    ? t('contextManagement.skills.unsyncAgent', { agent: agent.label, defaultValue: `取消同步 ${agent.label}` })
    : t('contextManagement.skills.syncAgent', { agent: agent.label, defaultValue: `同步到 ${agent.label}` });
  return (
    <Button
      type="button"
      variant="ghost"
      className="h-8 w-full justify-start gap-2 px-2 text-xs font-normal"
      disabled={isPending}
      aria-label={actionLabel}
      onClick={() => onToggleAgent(agent.agentType)}
    >
      {isPending ? <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" /> : <AgentSyncImage agent={agent} isSynced={isSynced} />}
      <span className="min-w-0 flex-1 truncate text-left">{agent.label}</span>
      <span className={cn('size-1.5 shrink-0 rounded-full', isSynced ? 'bg-emerald-500' : 'bg-muted-foreground/25')} aria-hidden="true" />
    </Button>
  );
}

function AgentImage({ agent }: { agent: SkillAgentDisplayMeta }) {
  return <img src={agentIconSrc(agent.iconKey)} alt={agent.label} className={agentIconClass(agent.iconKey, 'size-3.5')} />;
}

function AgentSyncImage({ agent, isSynced }: { agent: SkillAgentDisplayMeta; isSynced: boolean }) {
  return (
    <span className="relative grid size-5 shrink-0 place-items-center">
      {isSynced ? <span className="pointer-events-none absolute left-0 top-0 z-10 size-1.5 rounded-full bg-emerald-500 ring-1 ring-background" /> : null}
      <img
        src={agentIconSrc(agent.iconKey)}
        alt={agent.label}
        className={agentIconClass(agent.iconKey, cn('relative z-0 size-3.5 transition-opacity', !isSynced && 'grayscale opacity-35'))}
      />
    </span>
  );
}
