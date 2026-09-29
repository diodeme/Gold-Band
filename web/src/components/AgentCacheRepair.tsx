import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { LoaderCircle, Wrench } from 'lucide-react';
import { previewAgentCacheRepair, repairAgentCache, doctorAgent } from '@/api';
import { displayAppError } from '@/i18n';
import type { AgentRegistryVm } from '@/types';
import { Button } from '@/components/ui/button';
import { AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription, AlertDialogFooter, AlertDialogHeader, AlertDialogTitle } from '@/components/ui/alert-dialog';

type Flow = { phase: 'loading' | 'confirm' | 'repairing' | 'diagnosing' | 'result'; token?: string; paths?: string[]; message?: string; error?: string };

export function AgentCacheRepair({ agentType, disabled, onRegistryChange, onBusyChange }: {
  agentType: string; disabled: boolean; onRegistryChange: (vm: AgentRegistryVm) => void; onBusyChange: (busy: boolean) => void;
}) {
  const { t } = useTranslation();
  const [flow, setFlow] = useState<Flow | null>(null);
  const inFlight = useRef(false);
  const busy = flow?.phase === 'loading' || flow?.phase === 'repairing' || flow?.phase === 'diagnosing';
  const open = async () => {
    if (inFlight.current || disabled) return;
    inFlight.current = true;
    onBusyChange(true);
    setFlow({ phase: 'loading' });
    try { setFlow({ phase: 'confirm', ...await previewAgentCacheRepair(agentType) }); }
    catch (error) { setFlow({ phase: 'result', error: displayAppError(t, error) }); }
    finally { inFlight.current = false; }
  };
  const repair = async () => {
    if (inFlight.current || flow?.phase !== 'confirm' || !flow.token) return;
    inFlight.current = true;
    setFlow({ ...flow, phase: 'repairing' });
    try {
      const results = await repairAgentCache(agentType, flow.token);
      if (results.some((item) => item.errorCode)) {
        setFlow({ ...flow, phase: 'result', error: results.map((item) => `${item.path}: ${item.errorCode ? t(`errors.${item.errorCode}`) : t('agentManagement.cacheRepair.cleared')}`).join('\n') });
        return;
      }
      setFlow({ ...flow, phase: 'diagnosing' });
      try {
        const next = await doctorAgent(agentType);
        const healthy = next.agents.find((a) => a.agentType === agentType)?.diagnostic?.available;
        setFlow({ ...flow, phase: 'result', message: t(healthy ? 'agentManagement.cacheRepair.complete' : 'agentManagement.cacheRepair.stillFailed') });
        onRegistryChange(next);
      } catch (error) {
        setFlow({ ...flow, phase: 'result', error: `${t('agentManagement.cacheRepair.stillFailed')}\n${displayAppError(t, error)}` });
      }
    } catch (error) { setFlow({ ...flow, phase: 'result', error: displayAppError(t, error) }); }
    finally { inFlight.current = false; }
  };
  const close = () => { if (!inFlight.current) { setFlow(null); onBusyChange(false); } };
  return <>
    <Button variant="ghost" size="sm" disabled={disabled || busy} onClick={() => void open()}><Wrench />{t('agentManagement.cacheRepair.action')}</Button>
    <AlertDialog open={flow !== null} onOpenChange={(open) => { if (!open) close(); }}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{t('agentManagement.cacheRepair.title')}</AlertDialogTitle>
          <AlertDialogDescription>{t('agentManagement.cacheRepair.description')}</AlertDialogDescription>
        </AlertDialogHeader>
        {busy && <p role="status" className="flex items-center gap-2 text-sm"><LoaderCircle className="size-4 animate-spin" />{t(`agentManagement.cacheRepair.${flow?.phase}`)}</p>}
        {!!flow?.paths?.length && <details><summary className="cursor-pointer text-sm">{t('agentManagement.cacheRepair.targets', { count: flow.paths.length })}</summary><ul className="mt-2 max-h-40 overflow-y-auto text-xs font-mono">{flow.paths.map((path) => <li className="break-all" key={path}>{path}</li>)}</ul></details>}
        {flow?.error && <p role="alert" className="max-h-48 overflow-y-auto whitespace-pre-wrap break-all text-sm text-destructive">{flow.error}</p>}
        {flow?.message && <p role="status" className="text-sm">{flow.message}</p>}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={busy}>{t('common.close')}</AlertDialogCancel>
          {flow?.phase === 'confirm' && <AlertDialogAction onClick={(event) => { event.preventDefault(); void repair(); }}>{t('agentManagement.cacheRepair.confirm')}</AlertDialogAction>}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  </>;
}
