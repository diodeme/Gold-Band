import { useTranslation } from 'react-i18next';
import { ConversationComposer } from '@/components/conversation/ConversationComposer';
import type { AgentRegistryVm, ConversationCreateInput, ConversationRunModeVm } from '../types';

interface ConversationHomePageProps {
  projectId: string;
  workspaceName: string;
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  busy: boolean;
  onRunModeChange: (mode: ConversationRunModeVm) => void;
  onSubmit: (input: ConversationCreateInput) => void;
  onOpenRunModeSettings: () => void;
}

export function ConversationHomePage({
  projectId,
  workspaceName,
  runMode,
  agentRegistry,
  busy,
  onRunModeChange,
  onSubmit,
  onOpenRunModeSettings,
}: ConversationHomePageProps) {
  const { t } = useTranslation();

  return (
    <div className="flex h-full flex-col items-center justify-center px-8">
      <div className="w-full max-w-2xl space-y-6">
        <div className="text-center space-y-2">
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">
            {t('conversation.home.title')}
          </h1>
        </div>
        <ConversationComposer
          projectId={projectId}
          workspaceName={workspaceName}
          runMode={runMode}
          agentRegistry={agentRegistry}
          busy={busy}
          onRunModeChange={onRunModeChange}
          onSubmit={onSubmit}
          onOpenRunModeSettings={onOpenRunModeSettings}
        />
      </div>
    </div>
  );
}
