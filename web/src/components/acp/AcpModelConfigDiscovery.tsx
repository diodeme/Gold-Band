import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { LoaderCircle, RefreshCw } from 'lucide-react';
import { DropdownMenuItem, DropdownMenuSeparator } from '@/components/ui/dropdown-menu';
import { displayAppError } from '@/i18n';
import { fetchAgentModelConfig } from '@/api';

type DiscoveryStatus = 'idle' | 'loading' | 'done' | 'error';

export type AcpModelConfigDiscovery = {
  status: DiscoveryStatus;
  error: string;
  fetchConfig: () => void;
};

/**
 * Lives outside the dropdown content so status survives menu close. State is owned by the
 * Agent/model it was requested for; late results never change another model's status.
 */
export function useAcpModelConfigDiscovery(agentType: string | undefined, modelId: string | null | undefined): AcpModelConfigDiscovery {
  const { t } = useTranslation();
  const key = agentType && modelId ? JSON.stringify([agentType, modelId]) : null;
  const [state, setState] = useState<{ key: string; status: DiscoveryStatus; error: string } | null>(null);
  const current = key && state?.key === key ? state : null;
  const fetchConfig = () => {
    if (!key || !agentType || !modelId || current?.status === 'loading') return;
    setState({ key, status: 'loading', error: '' });
    const settle = (status: DiscoveryStatus, error = '') => setState((prev) => (prev?.key === key ? { key, status, error } : prev));
    fetchAgentModelConfig(agentType, modelId).then(
      () => settle('done'),
      (reason) => settle('error', displayAppError(t, reason)),
    );
  };
  return { status: current?.status ?? 'idle', error: current?.error ?? '', fetchConfig };
}

/**
 * Footer section for the model dropdown; selecting it keeps the menu open. Pinned to the bottom of
 * the scrolling menu content (offsetting its p-1) so long model lists never hide it.
 */
export function AcpModelConfigDiscoveryMenuSection({ discovery }: { discovery: AcpModelConfigDiscovery }) {
  const { t } = useTranslation();
  const { status, error, fetchConfig } = discovery;
  if (status === 'done') return null;
  const loading = status === 'loading';
  return <div data-acp-model-config-discovery-footer="true" className="sticky -bottom-1 z-10 -mx-1 -mb-1 flex flex-col bg-popover px-1 pb-1">
    <DropdownMenuSeparator />
    <p role="status" className="px-2 py-1 whitespace-normal break-words text-ui-caption leading-4 text-muted-foreground">
      {status === 'error' ? error : t('acp.modelConfigNotFetched')}
    </p>
    <DropdownMenuItem
      data-acp-model-config-discovery="true"
      disabled={loading}
      onSelect={(event) => {
        event.preventDefault();
        fetchConfig();
      }}
    >
      {loading ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
      {t(loading ? 'acp.fetchingModelConfig' : status === 'error' ? 'acp.retryModelConfig' : 'acp.fetchModelConfig')}
    </DropdownMenuItem>
  </div>;
}
