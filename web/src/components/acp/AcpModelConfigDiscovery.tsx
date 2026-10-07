import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { LoaderCircle } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { displayAppError } from '@/i18n';
import { fetchAgentModelConfig } from '@/api';

/** Keyed by Agent/model by the selector, so late results cannot change another model's status. */
export function AcpModelConfigDiscovery({ agentType, modelId, disabled }: {
  agentType: string; modelId: string; disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [status, setStatus] = useState<'idle' | 'loading' | 'done' | 'error'>('idle');
  const [error, setError] = useState('');
  const pending = useRef(false);
  if (status === 'done') return null;
  const fetchConfig = async () => {
    if (pending.current) return;
    pending.current = true;
    setStatus('loading');
    setError('');
    try {
      await fetchAgentModelConfig(agentType, modelId);
      setStatus('done');
    } catch (reason) {
      setError(displayAppError(t, reason));
      setStatus('error');
    } finally {
      pending.current = false;
    }
  };
  return <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs" data-slot="model-config-discovery">
    <span className="min-w-0 break-words text-muted-foreground" role="status">
      {status === 'error' ? error : t('acp.modelConfigNotFetched')}
    </span>
    <Button type="button" variant="ghost" size="sm" className="h-7 px-2 text-xs" disabled={disabled || status === 'loading'} onClick={() => void fetchConfig()}>
      {status === 'loading' ? <LoaderCircle className="size-3 animate-spin" /> : null}
      {t(status === 'loading' ? 'acp.fetchingModelConfig' : status === 'error' ? 'acp.retryModelConfig' : 'acp.fetchModelConfig')}
    </Button>
  </div>;
}
