import { describe, expect, it } from 'vitest';

import {
  isWorkflowAgentDoctorReady,
  validateWorkflowForSave,
  workflowEditorSupportedAgents,
} from '@/components/WorkflowEditor';
import type { AgentRegistryVm, ManagedAgentVm, WorkflowDsl } from '@/types';

const failedAgent = {
  agentType: 'cursor',
  displayName: 'Cursor',
  supported: true,
  diagnostic: {
    available: false,
    status: 'unhealthy',
    reason: 'adapter exited before initialize',
  },
} as ManagedAgentVm;

describe('workflow agent health', () => {
  it('keeps a configured failed agent in editor options but marks it unavailable', () => {
    const registry = {
      agents: [failedAgent],
      supportedTypes: [],
    } as AgentRegistryVm;

    expect(workflowEditorSupportedAgents(registry)).toEqual([failedAgent]);
    expect(isWorkflowAgentDoctorReady(failedAgent)).toBe(false);
  });

  it('still blocks saving a workflow that references the failed agent', () => {
    const workflow: WorkflowDsl = {
      version: '0.1',
      id: 'failed-agent-workflow',
      entry: 'cursor-node',
      control: {},
      nodes: [{
        type: 'worker',
        id: 'cursor-node',
        provider: 'cursor',
        profile: 'developer',
        goal: 'Implement the change',
      }],
      edges: [{ from: 'cursor-node', to: '$end', on: 'success' }],
    };

    const validation = validateWorkflowForSave(
      workflow,
      [{ id: 'developer', name: 'Developer' }],
      [],
      (key) => key,
    );

    expect(validation.valid).toBe(false);
    expect(validation.issues.map((issue) => issue.message)).toContain('workflowEditor.validationNodeProviderUnavailable');
  });
});
