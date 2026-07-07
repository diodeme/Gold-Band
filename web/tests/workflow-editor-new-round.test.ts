import { describe, expect, it } from 'vitest';
import { validateWorkflowForSave } from '../src/components/WorkflowEditor';
import type { ManagedAgentVm, ProfileVm, WorkflowDsl } from '../src/types';

const t = (key: string, options?: Record<string, unknown>) => `${key}${options ? `:${JSON.stringify(options)}` : ''}`;

const profiles: ProfileVm[] = [
  { id: 'developer', name: 'Developer', summary: '', content: '', scope: 'project', isBuiltIn: false, createdAt: '', updatedAt: '', path: '' },
];

const agents: ManagedAgentVm[] = [
  {
    agentType: 'claude-acp',
    displayName: 'Claude',
    command: 'claude',
    args: [],
    env: [],
    iconKey: 'claude',
    supported: true,
    diagnostic: { status: 'ok', available: true, checkedAt: '' },
    supportedModes: [],
    supportedModels: [],
  },
];

function worker(id: string) {
  return {
    type: 'worker' as const,
    id,
    provider: 'claude-acp',
    profile: 'developer',
    goal: `Run ${id}`,
  };
}

function validate(workflow: WorkflowDsl) {
  return validateWorkflowForSave(workflow, profiles, agents, t, null, null, null, false);
}

describe('WorkflowEditor new round entry validation', () => {
  it('requires new_round_entry on edges targeting new round', () => {
    const validation = validate({
      version: '0.1',
      id: 'missing-new-round-entry',
      entry: 'accept',
      control: {},
      nodes: [worker('accept')],
      edges: [
        { from: 'accept', to: '$new-round', on: 'failure' },
        { from: 'accept', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(false);
    expect(validation.fieldErrors['edge:0:new_round_entry']?.[0]).toContain('validationNewRoundEntryRequired');
  });

  it('accepts a real node as the next round start even when it has no ordinary incoming edge', () => {
    const validation = validate({
      version: '0.1',
      id: 'custom-new-round-entry',
      entry: 'accept',
      control: {},
      nodes: [worker('accept'), worker('dev')],
      edges: [
        { from: 'accept', to: '$new-round', on: 'failure', new_round_entry: 'dev' },
        { from: 'accept', to: '$end', on: 'success' },
        { from: 'dev', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(true);
  });

  it('rejects a missing real node selected as the next round start', () => {
    const validation = validate({
      version: '0.1',
      id: 'missing-custom-new-round-entry',
      entry: 'accept',
      control: {},
      nodes: [worker('accept')],
      edges: [
        { from: 'accept', to: '$new-round', on: 'failure', new_round_entry: 'dev' },
        { from: 'accept', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(false);
    expect(validation.fieldErrors['edge:0:new_round_entry']?.[0]).toContain('validationNewRoundEntryMissing');
  });

  it('removes new_round_entry from non-new-round edges in the saved workflow', () => {
    const validation = validate({
      version: '0.1',
      id: 'cleanup-non-new-round-edge',
      entry: 'dev',
      control: {},
      nodes: [worker('dev')],
      edges: [
        { from: 'dev', to: '$end', on: 'success', new_round_entry: '$entry' },
      ],
    });

    expect(validation.valid).toBe(true);
    expect(validation.sanitizedWorkflow.edges[0]).not.toHaveProperty('new_round_entry');
  });
});
