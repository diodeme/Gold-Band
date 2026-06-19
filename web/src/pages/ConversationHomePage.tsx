import { useTranslation } from 'react-i18next';
import { ConversationComposer } from '@/components/conversation/ConversationComposer';
import type { AgentRegistryVm, ConversationCreateInput, ConversationRunModeVm, ConversationWorkspaceVm, ProfileVm, WorkflowTemplateStore } from '../types';

interface ConversationHomePageProps {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  profiles: ProfileVm[];
  busy: boolean;
  onRunModeChange: (mode: ConversationRunModeVm) => void;
  onSubmit: (input: ConversationCreateInput) => Promise<string | null | undefined> | string | null | undefined;
  onOpenRunModeSettings: () => void;
  onWorkspaceChange: (projectId: string) => void;
}

export function ConversationHomePage({
  projectId,
  workspaceName,
  workspaces,
  runMode,
  agentRegistry,
  workflowTemplates,
  profiles,
  busy,
  onRunModeChange,
  onSubmit,
  onOpenRunModeSettings,
  onWorkspaceChange,
}: ConversationHomePageProps) {
  const { t } = useTranslation();

  return (
    <div className="flex h-full flex-col items-center justify-center px-8">
      <div className="w-full max-w-2xl space-y-5">
        <div className="text-center space-y-1.5">
          <h1 className="text-2xl font-semibold tracking-tight text-foreground">
            {t('conversation.home.title')}
          </h1>
        </div>
        <ConversationComposer
          projectId={projectId}
          workspaceName={workspaceName}
          workspaces={workspaces}
          runMode={runMode}
          agentRegistry={agentRegistry}
          workflowTemplates={workflowTemplates}
          profiles={profiles}
          busy={busy}
          onRunModeChange={onRunModeChange}
          onSubmit={onSubmit}
          onOpenRunModeSettings={onOpenRunModeSettings}
          onWorkspaceChange={onWorkspaceChange}
        />
      </div>
    </div>
  );
}
