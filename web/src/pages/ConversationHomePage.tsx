import { ConversationComposer } from '@/components/conversation/ConversationComposer';
import { ConversationGreeting } from '@/components/conversation/ConversationGreeting';
import { CONVERSATION_HOME_COMPOSER_LAYOUT } from '@/lib/conversation-composer-layout';
import type { ConversationComposerMulticaBinding } from '@/lib/conversation-composer-draft';
import { cn } from '@/lib/utils';
import { useThemeWallpaperSurface } from '@/components/theme/ThemeAssetsContext';
import type { AgentRegistryVm, ConversationCreateInput, ConversationRunModeVm, ConversationWorkLocation, ConversationWorkspaceVm, ProfileVm, WorkflowRepairTarget, WorkflowTemplateStore, ScheduledScheduleInput } from '../types';

interface ConversationHomePageProps {
  projectId: string;
  workspaceName: string;
  workspaces: ConversationWorkspaceVm[];
  runMode: ConversationRunModeVm;
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  profiles: ProfileVm[];
  busy: boolean;
  inlineContentMaxBytes: number;
  initialScheduledMode?: boolean;
  scheduledTaskCreated?: boolean;
  workLocation: ConversationWorkLocation;
  onRunModeChange: (mode: ConversationRunModeVm, projectId: string) => void;
  onLoadProfiles: () => Promise<ProfileVm[]>;
  onSubmit: (input: ConversationCreateInput, multica?: ConversationComposerMulticaBinding | null) => Promise<string | null | undefined> | string | null | undefined;
  onCreateScheduledTask?: (input: ConversationCreateInput & { schedule: ScheduledScheduleInput; overlapPolicy: 'skip_when_running' | 'retry_when_busy'; sessionPolicy?: 'new' | 'continuous' }) => Promise<void>;
  onScheduledTaskCreated?: () => void;
  onOpenAgentManagement: () => void;
  onOpenScheduledTasks: () => void;
  onOpenRunModeSettings: () => void;
  onWorkflowRepairTargetChange?: (target: WorkflowRepairTarget | null) => void;
  onWorkspaceChange: (projectId: string) => void;
  onWorkLocationChange: (location: ConversationWorkLocation, projectId: string) => Promise<void> | void;
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
  inlineContentMaxBytes,
  initialScheduledMode = false,
  scheduledTaskCreated = false,
  workLocation,
  onRunModeChange,
  onLoadProfiles,
  onSubmit,
  onCreateScheduledTask,
  onScheduledTaskCreated,
  onOpenAgentManagement,
  onOpenScheduledTasks,
  onOpenRunModeSettings,
  onWorkflowRepairTargetChange,
  onWorkspaceChange,
  onWorkLocationChange,
  onScheduledModeExit,
}: ConversationHomePageProps) {
  useThemeWallpaperSurface();
  return (
    <div data-theme-wallpaper-slot="conversation" className={cn(
      'flex h-full flex-col items-center justify-center px-4 sm:px-6 lg:px-8',
      CONVERSATION_HOME_COMPOSER_LAYOUT.opticalBottomPaddingClassName,
    )}>
      <div className={cn('w-full space-y-5', CONVERSATION_HOME_COMPOSER_LAYOUT.contentMaxWidthClassName)}>
        <div className="space-y-1.5 text-center">
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
          inlineContentMaxBytes={inlineContentMaxBytes}
          initialScheduledMode={initialScheduledMode}
          scheduledTaskCreated={scheduledTaskCreated}
          workLocation={workLocation}
          onRunModeChange={onRunModeChange}
          onLoadProfiles={onLoadProfiles}
          onSubmit={onSubmit}
          onCreateScheduledTask={onCreateScheduledTask}
          onScheduledTaskCreated={onScheduledTaskCreated}
          onOpenAgentManagement={onOpenAgentManagement}
          onOpenScheduledTasks={onOpenScheduledTasks}
          onOpenRunModeSettings={onOpenRunModeSettings}
          onWorkflowRepairTargetChange={onWorkflowRepairTargetChange}
          onWorkspaceChange={onWorkspaceChange}
          onWorkLocationChange={onWorkLocationChange}
          onScheduledModeExit={onScheduledModeExit}
        />
      </div>
    </div>
  );
}
