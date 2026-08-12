import type {
  AgentRegistryVm,
  ConversationCreateInput,
  ProfileVm,
  WorkflowTemplateStore,
} from '@/types';
import {
  validateAutoConfig,
  validateDirectConfig,
  validateWorkflowTemplateForConversationStartWithFreshProfiles,
} from '@/lib/run-mode-validation';

type ScheduledValidationDependencies = {
  agentRegistry: AgentRegistryVm | null;
  workflowTemplates: WorkflowTemplateStore | null;
  profiles: ProfileVm[];
  loadProfiles: () => Promise<ProfileVm[]>;
  t: (key: string, options?: Record<string, unknown>) => string;
};

export async function validateScheduledConversationInput(
  input: ConversationCreateInput,
  dependencies: ScheduledValidationDependencies,
): Promise<string[]> {
  if (input.runMode === 'direct') {
    return validateDirectConfig(input.directConfig, dependencies.agentRegistry, dependencies.t);
  }
  if (input.runMode === 'auto') {
    return validateAutoConfig(input.autoConfig, dependencies.agentRegistry, dependencies.workflowTemplates, dependencies.t);
  }
  return validateWorkflowTemplateForConversationStartWithFreshProfiles(
    input.workflowTemplateId,
    dependencies.agentRegistry,
    dependencies.profiles,
    dependencies.loadProfiles,
    dependencies.workflowTemplates,
    dependencies.t,
  );
}
