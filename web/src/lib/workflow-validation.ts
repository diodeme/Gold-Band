import type { DynamicControlDsl, ManagedAgentVm, ProfileVm, WorkflowAiDynamicNodeDsl, WorkflowControlDsl, WorkflowDsl, WorkflowEdgeDsl, WorkflowJsonConditionDsl, WorkflowModelBindings, WorkflowNodeDsl, WorkflowTemplate, WorkflowTemplateStore, WorkflowWorkerNodeDsl } from "@/types";

import type { WorkflowProfileCatalogState } from "@/lib/workflow-profile-catalog";

export const END_NODE = '$end';
export const ENTRY_NODE = '$entry';
export const NEW_ROUND_NODE = '$new-round';

export type WorkflowValidationIssue = { message: string; fieldKey?: string; nodeId?: string; nodeIds?: string[]; edgeIndex?: number };

export type WorkflowValidationResult = {
  valid: boolean;
  issues: WorkflowValidationIssue[];
  fieldErrors: Record<string, string[]>;
  sanitizedWorkflow: WorkflowDsl;
};

function emptyWorkflowModelBindings(): WorkflowModelBindings {
  return { definitionRevision: '', bindingRevision: 0, bindings: [] };
}

export function nodeSupportsFailureOutcome(node: WorkflowNodeDsl | undefined): boolean {
  return node?.type === 'worker' && Boolean(node.manual_check || node.output || node.success_condition);
}

export function dynamicControlFields(t: (key: string) => string): Array<{ key: Exclude<keyof DynamicControlDsl, 'allowNestedDynamic'>; label: string; help: string }> {
  return [
    { key: 'maxDynamicNodes', label: t('workflowEditor.maxDynamicNodes'), help: t('workflowEditor.maxDynamicNodesHelp') },
    { key: 'maxFanout', label: t('workflowEditor.maxFanout'), help: t('workflowEditor.maxFanoutHelp') },
    { key: 'maxDepth', label: t('workflowEditor.maxDepth'), help: t('workflowEditor.maxDepthHelp') },
    { key: 'maxParallel', label: t('workflowEditor.maxParallel'), help: t('workflowEditor.maxParallelHelp') },
    { key: 'maxGroupDepth', label: t('workflowEditor.maxGroupDepth'), help: t('workflowEditor.maxGroupDepthHelp') },
    { key: 'maxWorkflowInvocations', label: t('workflowEditor.maxWorkflowInvocations'), help: t('workflowEditor.maxWorkflowInvocationsHelp') },
  ];
}

export function workflowContainsAiDynamic(workflow: WorkflowDsl) {
  return workflow.nodes.some((item) => item.type === 'ai-dynamic');
}

export function workflowIdCountMap(templates: WorkflowTemplate[]) {
  const counts = new Map<string, number>();
  templates.forEach((template) => {
    const workflowId = template.workflow.id.trim();
    if (!workflowId) return;
    counts.set(workflowId, (counts.get(workflowId) ?? 0) + 1);
  });
  return counts;
}

export function deriveWorkflowEntryCandidateIds(workflow: Pick<WorkflowDsl, 'nodes' | 'edges'>): string[] {
  const nodeIds = new Set(workflow.nodes.map((node) => node.id).filter(Boolean));
  const nodeOrder = workflowSuccessTopologyOrder({ ...workflow, entry: '' });
  const incomingNodeIds = new Set<string>();
  workflow.edges.forEach((edge) => {
    if (!nodeIds.has(edge.from) || !nodeIds.has(edge.to)) return;
    if (edge.on !== 'success' && isBackwardEdge(edge.from, edge.to, nodeOrder)) return;
    incomingNodeIds.add(edge.to);
  });
  return workflow.nodes
    .map((node) => node.id)
    .filter((id) => Boolean(id) && !incomingNodeIds.has(id));
}

export function parseWorkflowJson(json?: string | null): WorkflowDsl | null {
  if (!json) return null;
  try {
    const value = JSON.parse(json) as WorkflowDsl;
    return value?.version && Array.isArray(value.nodes) ? value : null;
  } catch {
    return null;
  }
}

export function defaultDynamicControl(): DynamicControlDsl {
  return {
    maxDynamicNodes: 20,
    maxFanout: 5,
    maxDepth: 6,
    maxParallel: 3,
    maxGroupDepth: 1,
    maxWorkflowInvocations: 10,
    allowNestedDynamic: false,
  };
}

export function normalizeWorkflowSchemas(workflow: WorkflowDsl): WorkflowDsl {
  const rawControl = workflow.control as WorkflowControlDsl & Record<string, unknown>;
  const control: WorkflowControlDsl = {};
  if (rawControl?.max_attempts != null) control.max_attempts = normalizeControlLimit(rawControl.max_attempts);
  if (rawControl?.max_rounds != null) control.max_rounds = normalizeControlLimit(rawControl.max_rounds);
  return {
    ...workflow,
    control,
    edges: normalizeWorkflowEdges(workflow.edges ?? []),
    nodes: workflow.nodes.map((node) => {
      if (node.type === 'ai-dynamic') {
        const rawNode = node as WorkflowAiDynamicNodeDsl & {
          provider?: string | null;
          profile?: string | null;
          goal?: string | null;
          agentStrategy?: WorkflowAiDynamicNodeDsl['agentStrategy'];
          allowedProfiles?: string[];
          globalGoal?: string | null;
        };
        const normalizedStrategy = rawNode.agentStrategy ?? {
          mode: 'fixed',
          provider: rawNode.provider ?? '',
        };
        const agentStrategy = normalizedStrategy.mode === 'fixed'
          ? {
            ...normalizedStrategy,
            model: normalizedStrategy.model?.trim() ? normalizedStrategy.model : undefined,
            permissionMode: normalizedStrategy.permissionMode?.trim() ? normalizedStrategy.permissionMode : undefined,
          }
          : {
            ...normalizedStrategy,
            bootstrapModel: normalizedStrategy.bootstrapModel?.trim() ? normalizedStrategy.bootstrapModel : undefined,
            permissionMode: normalizedStrategy.permissionMode?.trim() ? normalizedStrategy.permissionMode : undefined,
            acceptanceModel: normalizedStrategy.acceptanceModel?.trim() ? normalizedStrategy.acceptanceModel : undefined,
            routingPrompt: normalizedStrategy.routingPrompt ?? '',
            availableAgents: (normalizedStrategy.availableAgents ?? []).map((agent) => ({
              ...agent,
              model: agent.model?.trim() ? agent.model : undefined,
              permissionMode: agent.permissionMode?.trim() ? agent.permissionMode : undefined,
            })),
          };
        return {
          ...node,
          agentStrategy,
          allowedProfiles: node.allowedProfiles ?? rawNode.allowedProfiles ?? [],
          globalGoal: node.globalGoal ?? rawNode.globalGoal ?? null,
          control: { ...defaultDynamicControl(), ...((node.control ?? {}) as Partial<DynamicControlDsl>), allowNestedDynamic: false },
          allowedWorkflows: node.allowedWorkflows ?? [],
        };
      }
      const normalizedNode = { ...(node as WorkflowWorkerNodeDsl & { primary_artifact?: unknown }) };
      delete normalizedNode.primary_artifact;
      if (!normalizedNode.output?.schema) return normalizedNode;
      return {
        ...normalizedNode,
        output: {
          ...normalizedNode.output,
          schema: normalizeOutputSchema(normalizedNode.output.schema),
        },
      };
    }),
  };
}

function normalizeWorkflowEdges(edges: WorkflowEdgeDsl[]): WorkflowEdgeDsl[] {
  return edges.map((edge) => {
    const normalized = { ...edge };
    const newRoundEntry = normalized.new_round_entry?.trim();
    if (normalized.to === NEW_ROUND_NODE) {
      if (newRoundEntry) normalized.new_round_entry = newRoundEntry;
      else delete normalized.new_round_entry;
    } else {
      delete normalized.new_round_entry;
    }
    return normalized;
  });
}

function normalizeControlLimit(value: unknown): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? Math.trunc(parsed) : 0;
}

export function normalizeOutputSchema(schema: unknown): unknown {
  const simple = jsonSchemaToSimpleShape(schema);
  return simple ?? schema;
}

function jsonSchemaToSimpleShape(schema: unknown): unknown | null {
  if (!isRecord(schema)) return null;
  if (schema.type === 'object' && isRecord(schema.properties)) {
    const shape: Record<string, unknown> = {};
    Object.entries(schema.properties).forEach(([key, value]) => {
      shape[key] = jsonSchemaToSimpleShape(value) ?? simpleTypeFromJsonSchema(value);
    });
    return shape;
  }
  if (schema.type === 'array') {
    const itemShape = jsonSchemaToSimpleShape(schema.items) ?? simpleTypeFromJsonSchema(schema.items);
    return itemShape ? [itemShape] : ['String'];
  }
  return simpleTypeFromJsonSchema(schema);
}

function simpleTypeFromJsonSchema(schema: unknown): string | null {
  if (!isRecord(schema) || typeof schema.type !== 'string') return null;
  if (schema.type === 'string') return 'String';
  if (schema.type === 'boolean') return 'boolean';
  if (schema.type === 'number') return 'number';
  if (schema.type === 'integer') return 'integer';
  if (schema.type === 'object') return 'object';
  if (schema.type === 'array') return 'array';
  if (schema.type === 'null') return 'null';
  return null;
}

export function cloneWorkflow(workflow: WorkflowDsl): WorkflowDsl {
  return JSON.parse(JSON.stringify(workflow)) as WorkflowDsl;
}

type PathSegment = { type: 'key'; key: string } | { type: 'index'; index: number };

export function validateWorkflowForSave(
  workflow: WorkflowDsl,
  profileCatalog: WorkflowProfileCatalogState,
  agents: ManagedAgentVm[],
  t: (key: string, options?: Record<string, unknown>) => string,
  workflowTemplates: WorkflowTemplateStore | null = null,
  currentTemplateId: string | null = null,
  currentTemplateName: string | null = null,
  validateTemplateDuplicateId = true,
  modelBindings: WorkflowModelBindings = emptyWorkflowModelBindings(),
  validateModelBindings = true,
): WorkflowValidationResult {
  const sanitizedWorkflow = normalizeWorkflowSchemas(cloneWorkflow(workflow));
  const issues: WorkflowValidationIssue[] = [];
  const fieldErrors: Record<string, string[]> = {};
  const profiles = profileCatalog.profiles;
  const profileCatalogReady = profileCatalog.status === 'ready';
  const profileIds = new Set(profiles.map((profile) => profile.id));
  const agentById = new Map(agents.map((agent) => [agent.agentType, agent]));
  const agentIds = new Set(agentById.keys());
  const templates = workflowTemplates?.templates ?? [];
  const workflowIdCounts = workflowIdCountMap(templates);
  const duplicateWorkflowTemplates = workflow.id.trim()
    ? templates.filter((template) => template.workflow.id.trim() === workflow.id.trim())
    : [];
  const duplicateConflictTemplates = duplicateWorkflowTemplates.filter((template) => template.id !== currentTemplateId);
  const nodeIds = new Set(workflow.nodes.map((node) => node.id).filter(Boolean));
  const nodeById = new Map(workflow.nodes.map((node) => [node.id, node]));
  const entryCandidateIds = deriveWorkflowEntryCandidateIds(sanitizedWorkflow);
  sanitizedWorkflow.entry = entryCandidateIds.length === 1 ? entryCandidateIds[0] : '';
  const outgoingEdgeCounts = workflow.edges.reduce<Record<string, number>>((counts, edge) => {
    if (edge.from.trim()) {
      counts[edge.from] = (counts[edge.from] ?? 0) + 1;
    }
    return counts;
  }, {});
  const edgeOutcomeCounts = workflow.edges.reduce<Record<string, number>>((counts, edge) => {
    if (edge.from.trim() && ['success', 'failure'].includes(edge.on)) {
      const key = `${edge.from}\0${edge.on}`;
      counts[key] = (counts[key] ?? 0) + 1;
    }
    return counts;
  }, {});
  const reportedDuplicateEdgeOutcomes = new Set<string>();
  const nodeIdCounts = workflow.nodes.reduce<Record<string, number>>((counts, node) => {
    counts[node.id] = (counts[node.id] ?? 0) + 1;
    return counts;
  }, {});
  const bindingBySlot = new Map(modelBindings.bindings.map((binding) => [binding.executionSlotId, binding]));
  const slotNodeIds = new Map<string, string[]>();
  workflow.nodes.forEach((node) => {
    if (node.type !== 'worker' || !node.executionSlotId?.trim()) return;
    slotNodeIds.set(node.executionSlotId, [...(slotNodeIds.get(node.executionSlotId) ?? []), node.id]);
  });

  const addIssue = (message: string, fieldKey?: string, nodeId?: string, edgeIndex?: number, nodeIds?: string[]) => {
    issues.push({ message, fieldKey, nodeId, edgeIndex, nodeIds });
    if (fieldKey) fieldErrors[fieldKey] = [...(fieldErrors[fieldKey] ?? []), message];
  };
  const nodeField = (node: WorkflowNodeDsl, field: string) => `node:${node.id}:${field}`;
  const edgeField = (index: number, field: string) => `edge:${index}:${field}`;
  const controlField = (field: string) => `control:${field}`;
  if (!workflow.id.trim()) addIssue(t('workflowEditor.validationWorkflowIdRequired'));
  else if (validateTemplateDuplicateId && duplicateConflictTemplates.length > 0) {
    addIssue(
      t('errors.workflow.duplicate-id', {
        workflowName: currentTemplateName ?? duplicateWorkflowTemplates.find((template) => template.id === currentTemplateId)?.name ?? workflow.id.trim(),
        workflowId: workflow.id.trim(),
        conflicts: duplicateConflictTemplates.map((template) => template.name).join('、'),
      }),
    );
  }
  if (!workflow.nodes.length) addIssue(t('workflowEditor.validationNodesRequired'));
  else if (entryCandidateIds.length === 0) {
    addIssue(t('workflowEditor.validationEntryCandidateMissing'));
  } else if (entryCandidateIds.length > 1) {
    addIssue(t('workflowEditor.validationEntryCandidateMultiple', { entries: entryCandidateIds.join(', ') }), undefined, undefined, undefined, entryCandidateIds);
  }
  if (!workflow.edges.some((edge) => edge.to === END_NODE)) addIssue(t('workflowEditor.validationEndNodeRequired'));
  if (sanitizedWorkflow.control.max_attempts != null && sanitizedWorkflow.control.max_attempts <= 0) {
    addIssue(t('workflowEditor.validationMaxAttemptsPositive'), controlField('max_attempts'));
  }
  if (sanitizedWorkflow.control.max_rounds != null && sanitizedWorkflow.control.max_rounds <= 0) {
    addIssue(t('workflowEditor.validationMaxRoundsPositive'), controlField('max_rounds'));
  }

  workflow.nodes.forEach((node, nodeIndex) => {
    const nodeLabel = node.id || t('workflowEditor.unnamedNode');
    if (!node.id.trim()) addIssue(t('workflowEditor.validationNodeIdRequired', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    if ([END_NODE, ENTRY_NODE, NEW_ROUND_NODE].includes(node.id)) addIssue(t('workflowEditor.validationReservedNodeId', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    if ((nodeIdCounts[node.id] ?? 0) > 1) addIssue(t('workflowEditor.validationDuplicateNodeId', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    if ((outgoingEdgeCounts[node.id] ?? 0) === 0) {
      addIssue(t('workflowEditor.validationDanglingNode', { node: nodeLabel }), nodeField(node, 'id'), node.id);
    }

    if (node.type === 'ai-dynamic') {
      validateAiDynamicNodeForSave(node, nodeLabel, workflowTemplates, profiles, profileCatalogReady, agentIds, agentById, nodeField, addIssue, t);
      return;
    }
    const slotId = node.executionSlotId?.trim();
    const binding = slotId ? bindingBySlot.get(slotId) : null;
    if (!slotId) addIssue(t('workflowEditor.validationSlotRequired', { node: nodeLabel }), nodeField(node, 'provider'), node.id);
    else if ((slotNodeIds.get(slotId)?.length ?? 0) > 1) addIssue(t('workflowEditor.validationSlotDuplicate', { node: nodeLabel }), nodeField(node, 'provider'), node.id);
    else if (validateModelBindings && !binding?.agentId.trim()) addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'provider'), node.id);
    else if (validateModelBindings && binding && !agentIds.has(binding.agentId)) addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'provider'), node.id);
    else if (validateModelBindings && binding) {
      const agent = agentById.get(binding.agentId);
      if (binding.modelId?.trim() && !(agent?.supportedModels ?? []).some((model) => model.id === binding.modelId)) {
        addIssue(t('workflowEditor.validationModelUnavailable', { node: nodeLabel }), nodeField(node, 'model'), node.id);
      }
      const supportedModeIds = new Set((agent?.supportedModes ?? []).map((mode) => mode.id));
      if (binding.permissionModeId?.trim() && !supportedModeIds.has(binding.permissionModeId)) {
        addIssue(t('workflowEditor.validationPermissionModeUnavailable', { node: nodeLabel }), nodeField(node, 'permission_mode'), node.id);
      }
      Object.entries(binding.configOptions ?? {}).forEach(([optionId, value]) => {
        const option = agent?.configOptions?.find((item) => item.id === optionId);
        if (!option?.options.some((item) => item.value === value)) {
          addIssue(t('workflowEditor.validationConfigOptionUnavailable', { node: nodeLabel, option: optionId }), nodeField(node, 'model'), node.id);
        }
      });
    }

    const workerNode = node as WorkflowWorkerNodeDsl;
    if (!workerNode.profile?.trim()) {
      addIssue(t('workflowEditor.validationNodeProfileRequired', { node: nodeLabel }), nodeField(workerNode, 'profile'), workerNode.id);
    } else if (profileCatalogReady && !profileIds.has(workerNode.profile)) {
      addIssue(t('workflowEditor.validationNodeProfileVisibilityChanged', { node: nodeLabel }), nodeField(workerNode, 'profile'), workerNode.id);
      const sanitized = sanitizedWorkflow.nodes[nodeIndex];
      if (sanitized && sanitized.type === 'worker') sanitized.profile = null;
    }
    const validationEnabled = Boolean(workerNode.output || workerNode.success_condition);
    if (validationEnabled && workerNode.manual_check) {
      addIssue(t('workflowEditor.validationResultModeExclusive', { node: nodeLabel }), nodeField(workerNode, 'success_condition'), workerNode.id);
    }
    if (validationEnabled) {
      if (!workerNode.output?.artifact?.trim()) addIssue(t('workflowEditor.validationOutputArtifactRequired', { node: nodeLabel }), nodeField(workerNode, 'output.artifact'), workerNode.id);
      if (!workerNode.success_condition) addIssue(t('workflowEditor.validationSuccessExpressionRequired', { node: nodeLabel }), nodeField(workerNode, 'success_condition'), workerNode.id);
      let path: PathSegment[] | null = null;
      if (workerNode.success_condition) {
        try {
          path = successConditionPath(workerNode.success_condition);
        } catch {
          addIssue(t('workflowEditor.saveErrorInvalidExpression', { node: nodeLabel }), nodeField(workerNode, 'success_condition'), workerNode.id);
        }
      }
      const schema = workerNode.output?.schema;
      if (schema && looksLikeJsonSchema(schema)) {
        addIssue(t('workflowEditor.saveErrorLegacySchema', { node: nodeLabel }), nodeField(node, 'output.schema'), node.id);
      }
      if (schema && path && !looksLikeJsonSchema(schema) && !schemaContainsPath(schema, path)) {
        addIssue(t('workflowEditor.saveErrorMissingPath', { node: nodeLabel }), nodeField(node, 'output.schema'), node.id);
      }
    }
  });

  workflow.edges.forEach((edge, index) => {
    if (!edge.from.trim()) addIssue(t('workflowEditor.validationEdgeSourceRequired', { index: index + 1 }), edgeField(index, 'from'), undefined, index);
    else if (!nodeIds.has(edge.from)) addIssue(t('workflowEditor.validationEdgeSourceMissing', { node: edge.from }), edgeField(index, 'from'), edge.from, index);
    if (!edge.to.trim()) addIssue(t('workflowEditor.validationEdgeTargetRequired', { index: index + 1 }), edgeField(index, 'to'), undefined, index);
    else if (![END_NODE, NEW_ROUND_NODE].includes(edge.to) && !nodeIds.has(edge.to)) addIssue(t('workflowEditor.validationEdgeTargetMissing', { node: edge.to }), edgeField(index, 'to'), edge.to, index);
    if (!['success', 'failure'].includes(edge.on)) addIssue(t('workflowEditor.validationEdgeOutcomeRequired', { index: index + 1 }), edgeField(index, 'on'), undefined, index);
    else if (edge.on === 'failure' && !nodeSupportsFailureOutcome(nodeById.get(edge.from))) {
      addIssue(t('workflowEditor.validationFailureOutcomeRequiresResultDecision', { node: edge.from }), edgeField(index, 'on'), edge.from, index);
    }
    else if (edge.on === 'success' && edge.to === NEW_ROUND_NODE) {
      addIssue(t('workflowEditor.validationSuccessNewRoundTarget', { node: edge.from }), edgeField(index, 'to'), edge.from, index);
    } else if (edge.from.trim()) {
      const edgeOutcomeKey = `${edge.from}\0${edge.on}`;
      const edgeOutcomeCount = edgeOutcomeCounts[edgeOutcomeKey] ?? 0;
      if (edgeOutcomeCount > 1 && !reportedDuplicateEdgeOutcomes.has(edgeOutcomeKey)) {
        addIssue(t('workflowEditor.validationDuplicateEdgeOutcome', { node: edge.from, outcome: edge.on, num: edgeOutcomeCount }), edgeField(index, 'on'), edge.from, index);
        reportedDuplicateEdgeOutcomes.add(edgeOutcomeKey);
      }
    }
    if (edge.to === NEW_ROUND_NODE) {
      const newRoundEntry = edge.new_round_entry?.trim();
      if (!newRoundEntry) {
        addIssue(t('workflowEditor.validationNewRoundEntryRequired', { node: edge.from }), edgeField(index, 'new_round_entry'), edge.from, index);
      } else if (newRoundEntry !== ENTRY_NODE && !nodeIds.has(newRoundEntry)) {
        addIssue(t('workflowEditor.validationNewRoundEntryMissing', { node: edge.from, entry: newRoundEntry }), edgeField(index, 'new_round_entry'), edge.from, index);
      }
    }
    if ([END_NODE, NEW_ROUND_NODE].includes(edge.from)) addIssue(t('workflowEditor.validationTerminalEdgeSource', { node: edge.from }), edgeField(index, 'from'), undefined, index);
    if (edge.session === 'continue' && [END_NODE, NEW_ROUND_NODE].includes(edge.to)) addIssue(t('workflowEditor.validationContinueTerminalTarget', { index: index + 1 }), edgeField(index, 'session'), undefined, index);
  });

  return { valid: issues.length === 0, issues, fieldErrors, sanitizedWorkflow };
}

function validateAiDynamicNodeForSave(
  node: WorkflowAiDynamicNodeDsl,
  nodeLabel: string,
  workflowTemplates: WorkflowTemplateStore | null | undefined,
  profiles: ProfileVm[],
  profileCatalogReady: boolean,
  agentIds: Set<string>,
  agentById: Map<string, ManagedAgentVm>,
  nodeField: (node: WorkflowNodeDsl, field: string) => string,
  addIssue: (message: string, fieldKey?: string, nodeId?: string, edgeIndex?: number) => void,
  t: (key: string, options?: Record<string, unknown>) => string,
) {
  const control = { ...defaultDynamicControl(), ...(node.control ?? {}) };
  const validatePermissionMode = (provider: string | undefined, permissionMode: string | null | undefined, field: string) => {
    const mode = permissionMode?.trim();
    if (!provider || !mode) return;
    const supportedModeIds = new Set((agentById.get(provider)?.supportedModes ?? []).map((option) => option.id));
    if (supportedModeIds.size > 0 && !supportedModeIds.has(mode)) {
      addIssue(t('workflowEditor.validationPermissionModeUnavailable', { node: nodeLabel }), nodeField(node, field), node.id);
    }
  };
  if (node.agentStrategy.mode === 'fixed') {
    const provider = node.agentStrategy.provider?.trim();
    if (!provider) {
      addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.provider'), node.id);
    } else if (!agentIds.has(provider)) {
      addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'agentStrategy.provider'), node.id);
    }
    validatePermissionMode(provider, node.agentStrategy.permissionMode, 'agentStrategy.permissionMode');
  } else {
    const bootstrapProvider = node.agentStrategy.bootstrapProvider?.trim();
    if (!bootstrapProvider) {
      addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.bootstrapProvider'), node.id);
    } else if (!agentIds.has(bootstrapProvider)) {
      addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, 'agentStrategy.bootstrapProvider'), node.id);
    }
    validatePermissionMode(bootstrapProvider, node.agentStrategy.permissionMode, 'agentStrategy.permissionMode');
    if ((node.agentStrategy.availableAgents ?? []).length === 0) {
      addIssue(t('workflowEditor.validationDynamicAvailableAgentsRequired', { node: nodeLabel }), nodeField(node, 'agentStrategy.availableAgents'), node.id);
    }
    const seenDynamicAgents = new Set<string>();
    (node.agentStrategy.availableAgents ?? []).forEach((agentRef, index) => {
      const provider = agentRef.provider?.trim();
      if (!provider) {
        addIssue(t('workflowEditor.validationNodeProviderRequired', { node: nodeLabel }), nodeField(node, `agentStrategy.availableAgents.${index}.provider`), node.id);
        return;
      }
      if (seenDynamicAgents.has(provider)) {
        addIssue(t('workflowEditor.validationDynamicAgentDuplicated', { node: nodeLabel, agent: provider }), nodeField(node, 'agentStrategy.availableAgents'), node.id);
        return;
      }
      seenDynamicAgents.add(provider);
      if (!agentIds.has(provider)) {
        addIssue(t('workflowEditor.validationNodeProviderUnavailable', { node: nodeLabel }), nodeField(node, `agentStrategy.availableAgents.${index}.provider`), node.id);
      }
      validatePermissionMode(provider, agentRef.permissionMode, `agentStrategy.availableAgents.${index}.permissionMode`);
    });
  }
  const knownProfileIds = new Set(profiles.map((profile) => profile.id));
  const seenProfiles = new Set<string>();
  (node.allowedProfiles ?? []).forEach((profileId) => {
    const value = profileId?.trim();
    if (!value) {
      addIssue(t('workflowEditor.validationAllowedProfileRequired', { node: nodeLabel }), nodeField(node, 'allowedProfiles'), node.id);
      return;
    }
    if (seenProfiles.has(value)) {
      addIssue(t('workflowEditor.validationAllowedProfileDuplicated', { node: nodeLabel, profile: value }), nodeField(node, 'allowedProfiles'), node.id);
      return;
    }
    seenProfiles.add(value);
    if (profileCatalogReady && !knownProfileIds.has(value)) {
      addIssue(t('workflowEditor.validationAllowedProfileMissing', { node: nodeLabel, profile: value }), nodeField(node, 'allowedProfiles'), node.id);
    }
  });
  if (node.globalGoal !== undefined && node.globalGoal !== null && !node.globalGoal.trim()) {
    addIssue(t('workflowEditor.validationGlobalGoalBlank', { node: nodeLabel }), nodeField(node, 'globalGoal'), node.id);
  }
  dynamicControlFields(t).forEach((field) => {
    if ((control[field.key] ?? 0) <= 0) {
      addIssue(t('workflowEditor.validationDynamicLimitPositive', { node: nodeLabel, field: field.label }), nodeField(node, `control.${field.key}`), node.id);
    }
  });
  const templates = workflowTemplates?.templates ?? [];
  const workflowIdCounts = workflowIdCountMap(templates);
  const templateById = new Map(
    templates
      .filter((template) => workflowIdCounts.get(template.workflow.id.trim()) === 1)
      .map((template) => [template.workflow.id.trim(), template] as const),
  );
  const seen = new Set<string>();
  (node.allowedWorkflows ?? []).forEach((allowed) => {
    const workflowId = allowed.workflowId?.trim();
    if (!workflowId) {
      addIssue(t('workflowEditor.validationAllowedWorkflowRequired', { node: nodeLabel }), nodeField(node, 'allowedWorkflows'), node.id);
      return;
    }
    if (seen.has(workflowId)) {
      addIssue(t('workflowEditor.validationAllowedWorkflowDuplicated', { node: nodeLabel, workflow: workflowId }), nodeField(node, 'allowedWorkflows'), node.id);
      return;
    }
    seen.add(workflowId);
    const template = templateById.get(workflowId);
    if (!template) {
      const duplicated = (workflowIdCounts.get(workflowId) ?? 0) > 1;
      addIssue(t(duplicated ? 'workflowEditor.validationAllowedWorkflowIdNotUnique' : 'workflowEditor.validationAllowedWorkflowMissing', { node: nodeLabel, workflow: workflowId }), nodeField(node, 'allowedWorkflows'), node.id);
      return;
    }
    if (!control.allowNestedDynamic && workflowContainsAiDynamic(template.workflow)) {
      addIssue(t('workflowEditor.validationAllowedWorkflowNestedDynamic', { node: nodeLabel, workflow: workflowId }), nodeField(node, 'allowedWorkflows'), node.id);
    }
  });
}

function successConditionPath(condition: WorkflowJsonConditionDsl) {
  if ('expression' in condition) return parseExpressionPath(condition.expression ?? '');
  return parseJsonPath(condition.path ?? '');
}

function parseExpressionPath(expression: string) {
  const operators = ['>=', '<=', '!=', '==', '>', '<'];
  const operator = operators.find((item) => expression.includes(item));
  if (!operator) throw new Error('unsupported expression');
  const [left] = expression.split(operator);
  if (!left.trim().startsWith('$')) throw new Error('left side must start with $');
  return parseJsonPath(left.trim());
}

function parseJsonPath(path: string): PathSegment[] {
  let value = path.trim();
  if (value.startsWith('$.')) value = value.slice(2);
  else if (value === '$') throw new Error('root path is not supported');
  else if (value.startsWith('$')) value = value.slice(1);
  if (!value) throw new Error('empty path');

  const segments: PathSegment[] = [];
  let key = '';
  for (let index = 0; index < value.length;) {
    const char = value[index];
    if (char === '.') {
      if (!key) {
        if (segments.at(-1)?.type !== 'index') throw new Error('empty segment');
      } else {
        segments.push({ type: 'key', key });
        key = '';
      }
      index += 1;
      continue;
    }
    if (char === '[') {
      if (key) {
        segments.push({ type: 'key', key });
        key = '';
      }
      const closeIndex = value.indexOf(']', index + 1);
      if (closeIndex < 0) throw new Error('unclosed index');
      const rawIndex = value.slice(index + 1, closeIndex);
      if (!/^\d+$/.test(rawIndex)) throw new Error('invalid index');
      segments.push({ type: 'index', index: Number(rawIndex) });
      index = closeIndex + 1;
      if (index < value.length && value[index] !== '.' && value[index] !== '[') throw new Error('invalid separator');
      continue;
    }
    key += char;
    index += 1;
  }
  if (key) segments.push({ type: 'key', key });
  if (!segments.length) throw new Error('empty path');
  return segments;
}

function looksLikeJsonSchema(schema: unknown) {
  if (!isRecord(schema)) return false;
  return ['type', 'properties', 'required', 'additionalProperties', 'items'].some((key) => key in schema);
}

function schemaContainsPath(schema: unknown, path: PathSegment[]) {
  let cursor = schema;
  for (const segment of path) {
    if (segment.type === 'key') {
      if (!isRecord(cursor) || !(segment.key in cursor)) return false;
      cursor = cursor[segment.key];
      continue;
    }
    if (!Array.isArray(cursor)) return false;
    cursor = cursor[segment.index] ?? cursor[0];
    if (cursor === undefined) return false;
  }
  return true;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

export function isBackwardEdge(
  from: string,
  to: string,
  nodeOrder: Map<string, number>,
): boolean {
  const s = nodeOrder.get(from);
  const t = nodeOrder.get(to);
  return s !== undefined && t !== undefined && t < s;
}

export function workflowSuccessTopologyOrder(workflow: Pick<WorkflowDsl, 'entry' | 'nodes' | 'edges'>): Map<string, number> {
  const nodeIds = workflow.nodes.map((node) => node.id).filter(Boolean);
  const nodeIdSet = new Set(nodeIds);
  const adjacency = new Map<string, string[]>();
  const indegree = new Map<string, number>();

  nodeIds.forEach((id) => {
    adjacency.set(id, []);
    indegree.set(id, 0);
  });

  workflow.edges.forEach((edge) => {
    if (edge.on !== 'success') return;
    if (!nodeIdSet.has(edge.from) || !nodeIdSet.has(edge.to)) return;
    adjacency.get(edge.from)?.push(edge.to);
    indegree.set(edge.to, (indegree.get(edge.to) ?? 0) + 1);
  });

  const queued = new Set<string>();
  const queue: string[] = [];
  const pushRoot = (id: string) => {
    if (!nodeIdSet.has(id) || queued.has(id)) return;
    queued.add(id);
    queue.push(id);
  };

  pushRoot(workflow.entry);
  nodeIds.forEach((id) => {
    if ((indegree.get(id) ?? 0) === 0) pushRoot(id);
  });

  const ordered: string[] = [];
  while (queue.length > 0) {
    const id = queue.shift()!;
    ordered.push(id);
    adjacency.get(id)?.forEach((nextId) => {
      indegree.set(nextId, (indegree.get(nextId) ?? 0) - 1);
      if ((indegree.get(nextId) ?? 0) === 0) pushRoot(nextId);
    });
  }

  nodeIds.forEach((id) => {
    if (!queued.has(id)) ordered.push(id);
  });

  return new Map(ordered.map((id, index) => [id, index]));
}
