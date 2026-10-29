import { memo } from 'react';

import { Button } from '@/components/ui/button';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import {
  ACP_SESSION_COMPOSER_LAYOUT,
  acpComposerStackShapeClassName,
  type AcpComposerStackSlot,
} from '@/lib/conversation-composer-layout';
import { cn } from '@/lib/utils';

export interface AcpRunningAgentCard {
  agentExecutionId: string;
  label: string;
  statusLabel: string;
  metricsSummary: string;
  attention: boolean;
  canOpen: boolean;
}

/**
 * Summary cards of the Agents running right now, the top layer of the
 * composer stack. Opening a card shows the same Agent transcript as the
 * Agent card in the conversation.
 */
export const AcpRunningAgentsStrip = memo(function AcpRunningAgentsStrip({
  agents,
  ariaLabel,
  stackSlot,
  onOpen,
}: {
  agents: readonly AcpRunningAgentCard[];
  ariaLabel: string;
  stackSlot?: AcpComposerStackSlot;
  onOpen: (agentExecutionId: string) => void;
}) {
  if (agents.length === 0) return null;
  return (
    <div
      role="list"
      aria-label={ariaLabel}
      data-acp-running-agents="true"
      className={cn(
        'gold-themed-scrollbar flex min-w-0 flex-wrap gap-1.5 overflow-y-auto overscroll-contain bg-card px-2 py-1.5',
        ACP_SESSION_COMPOSER_LAYOUT.stackSurfaceClassName,
        ACP_SESSION_COMPOSER_LAYOUT.runningAgentsMaxHeightClassName,
        acpComposerStackShapeClassName(stackSlot),
      )}
    >
      {agents.map((agent) => (
        <Tooltip key={agent.agentExecutionId}>
          <TooltipTrigger asChild>
            <Button
              type="button"
              role="listitem"
              variant="ghost"
              size="sm"
              disabled={!agent.canOpen}
              data-acp-running-agent={agent.agentExecutionId}
              aria-label={`${agent.label} · ${agent.statusLabel}`}
              className="h-7 min-w-0 max-w-[16rem] gap-1.5 rounded-full bg-muted/40 px-2.5 text-xs font-normal hover:bg-muted/70 disabled:opacity-100"
              onClick={() => onOpen(agent.agentExecutionId)}
            >
              <span
                aria-hidden="true"
                className={cn(
                  'size-1.5 shrink-0 rounded-full',
                  agent.attention ? 'bg-amber-500' : 'animate-pulse bg-primary',
                )}
              />
              <span className="min-w-0 truncate text-foreground/90">{agent.label}</span>
            </Button>
          </TooltipTrigger>
          <TooltipContent side="top" className="max-w-80 break-words">
            <span className="block">{agent.label}</span>
            <span className="block">
              {agent.metricsSummary ? `${agent.statusLabel} · ${agent.metricsSummary}` : agent.statusLabel}
            </span>
          </TooltipContent>
        </Tooltip>
      ))}
    </div>
  );
});
