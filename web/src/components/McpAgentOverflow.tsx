import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { useTwoRowAgentOverflow } from '@/hooks/use-two-row-agent-overflow';
import { agentIconClass, agentIconSrc } from '@/lib/agent-icons';
import {
  mcpAgentSupportStatus,
  shouldShowCompatibilityLoading,
  type McpAgentCompatibility,
} from '@/lib/mcp-agent-compatibility';
import { cn } from '@/lib/utils';
import type { McpServerVm } from '@/types';

interface McpAgentOverflowProps {
  agents: McpAgentCompatibility[];
  transport: McpServerVm['transport'];
  transportLabel: string;
  diagnosingAgentType?: string | null;
  onDiagnoseAgent?: (agentType: string) => void;
}

export function McpAgentOverflow({
  agents,
  transport,
  transportLabel,
  diagnosingAgentType,
  onDiagnoseAgent,
}: McpAgentOverflowProps) {
  const { t } = useTranslation();
  const { containerRef, layout } = useTwoRowAgentOverflow(0, agents.length);
  const visibleAgents = agents.slice(0, layout.visibleSyncCount);
  const hiddenAgents = agents.slice(layout.visibleSyncCount);

  return (
    <div
      ref={containerRef}
      className="flex min-w-0 flex-1 flex-wrap content-center items-center gap-0.5 overflow-hidden"
      data-testid="mcp-agent-overflow"
    >
      {visibleAgents.map((agent) => (
        <McpAgentCompatibilityControl
          key={agent.agentType}
          agent={agent}
          transport={transport}
          transportLabel={transportLabel}
          diagnosingAgentType={diagnosingAgentType}
          onDiagnoseAgent={onDiagnoseAgent}
        />
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
                    className="h-6 w-7 min-w-7 shrink-0 rounded-full p-0 text-ui-micro font-medium tabular-nums"
                    aria-label={t('contextManagement.mcp.moreAgents', { count: layout.hiddenCount, defaultValue: `还有 ${layout.hiddenCount} 个 Agent` })}
                  >
                    +{layout.hiddenCount}
                  </Button>
                </PopoverTrigger>
              </TooltipTrigger>
              <TooltipContent side="top">
                {t('contextManagement.mcp.moreAgents', { count: layout.hiddenCount, defaultValue: `还有 ${layout.hiddenCount} 个 Agent` })}
              </TooltipContent>
            </Tooltip>
          </TooltipProvider>
          <PopoverContent side="top" align="start" sideOffset={8} className="w-64 p-2">
            <div className="px-2 pb-1 text-ui-micro font-medium uppercase tracking-wide text-muted-foreground">
              {t('contextManagement.mcp.agentCompatibility', 'Agent 兼容性')}
            </div>
            <ScrollArea className="max-h-72">
              <div className="space-y-0.5 pr-1">
                {hiddenAgents.map((agent) => (
                  <McpAgentCompatibilityControl
                    key={agent.agentType}
                    agent={agent}
                    transport={transport}
                    transportLabel={transportLabel}
                    diagnosingAgentType={diagnosingAgentType}
                    onDiagnoseAgent={onDiagnoseAgent}
                    expanded
                  />
                ))}
              </div>
            </ScrollArea>
          </PopoverContent>
        </Popover>
      ) : null}
    </div>
  );
}

function McpAgentCompatibilityControl({
  agent,
  transport,
  transportLabel,
  diagnosingAgentType,
  onDiagnoseAgent,
  expanded = false,
}: {
  agent: McpAgentCompatibility;
  transport: McpServerVm['transport'];
  transportLabel: string;
  diagnosingAgentType?: string | null;
  onDiagnoseAgent?: (agentType: string) => void;
  expanded?: boolean;
}) {
  const { t } = useTranslation();
  const status = mcpAgentSupportStatus(transport, agent);
  const isDiagnosing = diagnosingAgentType === agent.agentType;
  const clickable = status === 'unknown' && !!onDiagnoseAgent && !isDiagnosing;
  const showLoading = shouldShowCompatibilityLoading(status, isDiagnosing);
  const tip = status === 'supported'
    ? t('contextManagement.mcp.agentSupports', { agent: agent.label, transport: transportLabel, defaultValue: '{{agent}} 支持 {{transport}} MCP 传输' })
    : status === 'unsupported'
      ? t('contextManagement.mcp.agentNotSupports', { agent: agent.label, transport: transportLabel, defaultValue: '{{agent}} 不支持 {{transport}} MCP 传输' })
      : status === 'unavailable'
        ? t('contextManagement.mcp.agentUnavailable', {
          agent: agent.label,
          reason: agent.diagnosticReason ?? t('agentManagement.diagnosticFailedFallback'),
          defaultValue: '{{agent}} 当前不可用：{{reason}}',
        })
        : isDiagnosing
          ? t('contextManagement.mcp.agentChecking', { agent: agent.label, defaultValue: '正在更新 {{agent}} 的 MCP 兼容性' })
          : t('contextManagement.mcp.agentUnknown', { agent: agent.label, defaultValue: '{{agent}} 尚未声明 MCP 兼容性，点击重新检测' });

  return (
    <TooltipProvider delayDuration={300}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className={expanded ? 'block w-full' : 'inline-grid size-6 shrink-0 place-items-center rounded-full'}>
            <Button
              type="button"
              size={expanded ? 'sm' : 'icon'}
              variant="ghost"
              className={cn(
                'shrink-0 disabled:cursor-default disabled:opacity-100',
                expanded ? 'h-8 w-full justify-start gap-2 px-2 text-xs font-normal' : 'size-6 rounded-full',
              )}
              disabled={!clickable}
              aria-busy={isDiagnosing}
              aria-label={tip}
              onClick={clickable ? () => onDiagnoseAgent?.(agent.agentType) : undefined}
            >
              {showLoading ? (
                <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground/70" />
              ) : (
                <span className="relative grid size-5 shrink-0 place-items-center">
                  {status === 'supported' ? <span className="pointer-events-none absolute left-0 top-0 z-10 size-1.5 rounded-full bg-emerald-500 ring-1 ring-background" /> : null}
                  {status === 'unavailable' ? <span className="pointer-events-none absolute left-0 top-0 z-10 size-1.5 rounded-full bg-red-500 ring-1 ring-background" /> : null}
                  <img
                    src={agentIconSrc(agent.iconKey)}
                    alt={agent.label}
                    className={agentIconClass(agent.iconKey, cn(
                      'relative z-0 size-3.5 transition-opacity',
                      (status === 'unsupported' || status === 'unavailable') && 'grayscale opacity-35',
                      status === 'unknown' && 'grayscale opacity-55',
                      clickable && 'cursor-pointer',
                    ))}
                  />
                </span>
              )}
              {expanded ? <span className="min-w-0 flex-1 truncate text-left">{agent.label}</span> : null}
            </Button>
          </span>
        </TooltipTrigger>
        <TooltipContent side={expanded ? 'right' : 'top'}>{tip}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
