import { validateWorkflowForSave } from '@/components/WorkflowEditor';
import type {
  AgentRegistryVm,
  ConversationAutoConfigVm,
  ManagedAgentVm,
  ProfileVm,
  WorkflowTemplate,
  WorkflowTemplateStore,
} from '@/types';

export type SelectableAgentOption = {
  agent: ManagedAgentVm;
  selectable: boolean;
  reason?: string;
};

export type SelectableWorkflowOption = {
  template: WorkflowTemplate;
  workflowId: string;
  selectable: boolean;
  reason?: string;
};

export function agentDoctorReason(agent: ManagedAgentVm, t: (key: string, options?: Record<string, unknown>) => string) {
  if (!agent.supported) return t('runMode.agentUnavailable');
  if (agent.diagnostic?.available === true) return null;
  if (agent.diagnostic?.reason?.trim()) return agent.diagnostic.reason;
  return t('runMode.agentDoctorRequired');
}

export function selectableAgentOptions(
  agentRegistry: AgentRegistryVm | null,
  t: (key: string, options?: Record<string, unknown>) => string,
): SelectableAgentOption[] {
  return (agentRegistry?.agents ?? []).map((agent) => {
    const reason = agentDoctorReason(agent, t);
    return { agent, selectable: !reason, reason: reason ?? undefined };
  });
}

export function selectableWorkflowOptions(
  workflowTemplates: WorkflowTemplateStore | null,
  t: (key: string, options?: Record<string, unknown>) => string,
): SelectableWorkflowOption[] {
  const templates = workflowTemplates?.templates ?? [];
  const counts = new Map<string, number>();
  for (const template of templates) {
    const workflowId = template.workflow.id.trim();
    if (!workflowId) continue;
    counts.set(workflowId, (counts.get(workflowId) ?? 0) + 1);
  }
  return templates.map((template) => {
    const workflowId = template.workflow.id.trim();
    const duplicate = workflowId && (counts.get(workflowId) ?? 0) > 1;
    const reason = !workflowId
      ? t('runMode.workflowIdRequired')
      : duplicate
        ? t('runMode.workflowIdDuplicated', { workflowId })
        : null;
    return { template, workflowId, selectable: !reason, reason: reason ?? undefined };
  });
}

export function validateWorkflowTemplateForConversationStart(
  templateId: string | null | undefined,
  agentRegistry: AgentRegistryVm | null,
  profiles: ProfileVm[],
  workflowTemplates: WorkflowTemplateStore | null,
  t: (key: string, options?: Record<string, unknown>) => string,
): string[] {
  const selectedId = templateId?.trim();
  if (!selectedId) return [t('conversation.home.selectWorkflowTemplate')];
  const template = workflowTemplates?.templates.find((item) => item.id === selectedId);
  if (!template) return [t('conversation.validation.workflow.not-found')];
  const agents = agentRegistry?.agents.filter((agent) => agent.supported && agent.diagnostic?.available === true) ?? [];
  const validation = validateWorkflowForSave(
    template.workflow,
    profiles,
    agents,
    t,
    workflowTemplates,
    template.id,
    template.name,
  );
  return validation.valid ? [] : validation.issues.map((issue) => issue.message);
}

export function validateAutoConfig(
  config: ConversationAutoConfigVm | null | undefined,
  agentRegistry: AgentRegistryVm | null,
  workflowTemplates: WorkflowTemplateStore | null,
  t: (key: string, options?: Record<string, unknown>) => string,
): string[] {
  const issues: string[] = [];
  const agents = agentRegistry?.agents ?? [];
  const agentById = new Map(agents.map((agent) => [agent.agentType, agent]));
  const workflows = selectableWorkflowOptions(workflowTemplates, t);
  const validWorkflowIds = new Set(workflows.filter((item) => item.selectable).map((item) => item.workflowId));
  const workflowReasonById = new Map(workflows.map((item) => [item.workflowId, item.reason]));
  const strategy = config?.agentStrategy ?? 'fixed';

  const requireReadyAgent = (agentType: string | null | undefined, label: string) => {
    const id = agentType?.trim();
    if (!id) {
      issues.push(t('runMode.validationAgentRequired', { label }));
      return;
    }
    const found = agentById.get(id);
    if (!found) {
      issues.push(t('runMode.validationAgentMissing', { label, agent: id }));
      return;
    }
    const reason = agentDoctorReason(found, t);
    if (reason) issues.push(t('runMode.validationAgentUnavailable', { label, agent: found.displayName, reason }));
  };

  if (strategy === 'dynamic') {
    requireReadyAgent(config?.bootstrapAgentType || config?.agentType, t('workflowEditor.dynamicBootstrapAgent'));
    const availableAgents = config?.availableAgents ?? [];
    if (availableAgents.length === 0) {
      issues.push(t('runMode.validationDynamicAvailableAgentsRequired'));
    }
    const hasRoutingPrompt = Boolean(config?.routingPrompt?.trim());
    const seen = new Set<string>();
    for (const item of availableAgents) {
      const provider = item.provider.trim();
      if (seen.has(provider)) issues.push(t('runMode.validationDynamicAgentDuplicated', { agent: provider }));
      seen.add(provider);
      requireReadyAgent(provider, t('workflowEditor.dynamicAvailableAgents'));
      if (!hasRoutingPrompt && !item.model?.trim()) {
        issues.push(t('runMode.validationDynamicAgentModelRequiredWithoutRouting', { agent: provider }));
      }
    }
  } else {
    requireReadyAgent(config?.agentType, t('runMode.agent'));
  }

  const selectedWorkflowIds = config?.allowedWorkflows?.map((item) => item.workflowId.trim()).filter(Boolean) ?? [];
  const seenWorkflowIds = new Set<string>();
  for (const workflowId of selectedWorkflowIds) {
    if (seenWorkflowIds.has(workflowId)) {
      issues.push(t('workflowEditor.validationAllowedWorkflowDuplicated', { node: 'AI-DYNAMIC', workflow: workflowId }));
      continue;
    }
    seenWorkflowIds.add(workflowId);
    if (!validWorkflowIds.has(workflowId)) {
      issues.push(workflowReasonById.get(workflowId) ?? t('workflowEditor.validationAllowedWorkflowMissing', { node: 'AI-DYNAMIC', workflow: workflowId }));
    }
  }

  const control = config?.control;
  if (control) {
    const numericFields = [
      ['maxDynamicNodes', t('workflowEditor.maxDynamicNodes')],
      ['maxFanout', t('workflowEditor.maxFanout')],
      ['maxDepth', t('workflowEditor.maxDepth')],
      ['maxParallel', t('workflowEditor.maxParallel')],
      ['maxGroupDepth', t('workflowEditor.maxGroupDepth')],
      ['maxWorkflowInvocations', t('workflowEditor.maxWorkflowInvocations')],
    ] as const;
    for (const [key, label] of numericFields) {
      if (!Number.isFinite(control[key]) || control[key] <= 0) {
        issues.push(t('runMode.validationPositiveNumber', { field: label }));
      }
    }
  }

  return Array.from(new Set(issues));
}
