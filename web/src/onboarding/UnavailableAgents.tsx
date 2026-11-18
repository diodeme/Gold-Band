import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowRight, ChevronRight, CircleHelp } from 'lucide-react';
import { AgentIdentityLabel } from '@/components/AgentIdentityLabel';
import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { agentDiagnosticHelpReason, agentDiagnosticShortReason } from '@/lib/agent-diagnostic';
import type { ManagedAgentDiagnosticVm } from '@/types';
import type { EnvironmentReport, SetupUnavailableAgent } from './types';

const VERSION_WARNING_STATUSES = new Set(['unverified', 'unsupported']);

function diagnosticOf(reason: SetupUnavailableAgent['reason']): ManagedAgentDiagnosticVm | null {
  return reason.kind === 'doctor-failed' ? { status: 'unhealthy', available: false, error: reason.error, checkedAt: '' } : null;
}

export function npxRuntimeHint(environment: EnvironmentReport | undefined, recommendedVersion: string): { key: string; params: Record<string, string> } | null {
  const runtime = environment?.checks.filter((check) => check.tool !== 'git') ?? [];
  const node = runtime.find((check) => check.tool === 'node');
  if (node?.version && VERSION_WARNING_STATUSES.has(node.status)) return { key: 'onboarding.unavailable.nodeVersionHint', params: { version: node.version, recommended: recommendedVersion } };
  return runtime.some((check) => check.status !== 'ready') ? { key: 'onboarding.unavailable.runtimeHint', params: {} } : null;
}

export function UnavailableAgents({ agents, readyCount, environment, recommendedVersion, onViewRuntime }: {
  agents: SetupUnavailableAgent[];
  readyCount: number;
  environment?: EnvironmentReport;
  recommendedVersion: string;
  onViewRuntime: () => void;
}) {
  const { t } = useTranslation();
  const [chosenOpen, setChosenOpen] = useState<boolean>();
  if (!agents.length) return null;
  const open = chosenOpen ?? readyCount === 0;
  const hint = npxRuntimeHint(environment, recommendedVersion);
  const viewRuntime = <Button variant="ghost" size="sm" className="shrink-0" onClick={onViewRuntime}>{t('onboarding.viewRuntime')}<ArrowRight className="size-4" /></Button>;
  return <Collapsible open={open} onOpenChange={setChosenOpen} className="mt-7 border-t pt-4">
    <CollapsibleTrigger asChild>
      <Button variant="ghost" size="sm" className="-ml-2 text-muted-foreground" aria-expanded={open}>
        <ChevronRight className={`size-4 transition-transform ${open ? 'rotate-90' : ''}`} />{t('onboarding.unavailable.title', { count: agents.length })}
      </Button>
    </CollapsibleTrigger>
    <CollapsibleContent>
      <ul className="mt-1 divide-y" aria-label={t('onboarding.unavailable.title', { count: agents.length })}>
        {agents.map((agent) => {
          const diagnostic = diagnosticOf(agent.reason);
          const agentHint = agent.reason.kind === 'doctor-failed' && agent.reason.launch === 'npx' ? hint : null;
          return <li key={agent.agentType} className="grid grid-cols-[minmax(0,1fr)_auto] items-start gap-x-3 gap-y-1 py-3 text-sm">
            <div className="min-w-0 space-y-1">
              <AgentIdentityLabel iconKey={agent.iconKey} name={agent.displayName} />
              <p className="break-words text-muted-foreground [overflow-wrap:anywhere]">
                {diagnostic ? t('onboarding.unavailable.doctorFailed', { reason: agentDiagnosticShortReason(t, diagnostic) }) : t('onboarding.unavailable.runtimeRequired')}
              </p>
              {agentHint && <p className="break-words text-gold-warning [overflow-wrap:anywhere]">{t(agentHint.key, agentHint.params)}</p>}
            </div>
            <div className="flex items-center gap-1">
              {diagnostic?.error && <Popover>
                <PopoverTrigger asChild>
                  <Button type="button" variant="ghost" size="icon" className="size-7 rounded-full text-muted-foreground hover:text-foreground" aria-label={t('agentManagement.diagnosticHelpLabel')}><CircleHelp className="size-4" /></Button>
                </PopoverTrigger>
                <PopoverContent side="left" align="start" sideOffset={8} className="w-72 max-w-[20rem] px-2.5 py-2 text-xs leading-[1.45]">
                  <div className="max-h-40 overflow-y-auto whitespace-pre-wrap break-words [overflow-wrap:anywhere]">{agentDiagnosticHelpReason(t, diagnostic)}</div>
                </PopoverContent>
              </Popover>}
              {(!diagnostic || agentHint) && viewRuntime}
            </div>
          </li>;
        })}
      </ul>
    </CollapsibleContent>
  </Collapsible>;
}
