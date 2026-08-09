import { ConversationComposer } from '@/components/conversation/ConversationComposer';
import { ConversationGreeting } from '@/components/conversation/ConversationGreeting';
import { CONVERSATION_HOME_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';
import { cn } from '@/lib/utils';
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
  onRunModeChange: (mode: ConversationRunModeVm, projectId: string) => void;
  onLoadProfiles: () => Promise<ProfileVm[]>;
  onSubmit: (input: ConversationCreateInput) => Promise<string | null | undefined> | string | null | undefined;
  onOpenAgentManagement: () => void;
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
  onLoadProfiles,
  onSubmit,
  onOpenAgentManagement,
  onOpenRunModeSettings,
  onWorkspaceChange,
}: ConversationHomePageProps) {
  return (
    <div className={cn(
      'flex h-full flex-col items-center justify-center px-4 sm:px-6 lg:px-8',
      CONVERSATION_HOME_COMPOSER_LAYOUT.opticalBottomPaddingClassName,
    )}>
      <div className={cn('w-full space-y-5', CONVERSATION_HOME_COMPOSER_LAYOUT.contentMaxWidthClassName)}>
        <div className="text-center space-y-1.5">
          <ConversationGreeting />
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
          onLoadProfiles={onLoadProfiles}
          onSubmit={onSubmit}
          onOpenAgentManagement={onOpenAgentManagement}
          onOpenRunModeSettings={onOpenRunModeSettings}
          onWorkspaceChange={onWorkspaceChange}
        />
      </div>
    </div>
  );
}
