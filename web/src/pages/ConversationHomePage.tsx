import { ConversationComposer } from '@/components/conversation/ConversationComposer';
import { ConversationGreeting } from '@/components/conversation/ConversationGreeting';
import { CONVERSATION_HOME_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';
import { cn } from '@/lib/utils';
import { useThemeWallpaperSurface } from '@/components/theme/ThemeAssetsContext';
import type { AgentRegistryVm, ConversationCreateInput, ConversationRunModeVm, ConversationWorkspaceVm, ProfileVm, WorkflowRepairTarget, WorkflowTemplateStore, ScheduledScheduleInput } from '../types';

interface ConversationHomePageProps {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  profiles: ProfileVm[];
  busy: boolean;
  initialScheduledMode?: boolean;
  onRunModeChange: (mode: ConversationRunModeVm, projectId: string) => void;
  onLoadProfiles: () => Promise<ProfileVm[]>;
  onSubmit: (input: ConversationCreateInput) => Promise<string | null | undefined> | string | null | undefined;
  onCreateScheduledTask?: (input: ConversationCreateInput & { schedule: ScheduledScheduleInput; overlapPolicy: 'skip_when_running' | 'retry_when_busy'; sessionPolicy?: 'new' | 'continuous' }) => Promise<void>;
  onOpenAgentManagement: () => void;
  onOpenRunModeSettings: () => void;
  onWorkflowRepairTargetChange?: (target: WorkflowRepairTarget | null) => void;
  onWorkspaceChange: (projectId: string) => void;
  onScheduledModeExit?: () => void;
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
  initialScheduledMode = false,
  onRunModeChange,
  onLoadProfiles,
  onSubmit,
  onCreateScheduledTask,
  onOpenAgentManagement,
  onOpenRunModeSettings,
  onWorkflowRepairTargetChange,
  onWorkspaceChange,
  onScheduledModeExit,
}: ConversationHomePageProps) {
  useThemeWallpaperSurface();
  return (
    <div data-theme-wallpaper-slot="conversation" className={cn(
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
          initialScheduledMode={initialScheduledMode}
          onRunModeChange={onRunModeChange}
          onLoadProfiles={onLoadProfiles}
          onSubmit={onSubmit}
          onCreateScheduledTask={onCreateScheduledTask}
          onOpenAgentManagement={onOpenAgentManagement}
          onOpenRunModeSettings={onOpenRunModeSettings}
          onWorkflowRepairTargetChange={onWorkflowRepairTargetChange}
          onWorkspaceChange={onWorkspaceChange}
          onScheduledModeExit={onScheduledModeExit}
        />
      </div>
    </div>
  );
}
