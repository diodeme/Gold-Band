import { describe, expect, it } from 'vitest';
import { validateWorkflowForSave } from '../src/components/WorkflowEditor';
import type { ManagedAgentVm, ProfileVm, WorkflowDsl } from '../src/types';

const t = (key: string, options?: Record<string, unknown>) => `${key}${options ? `:${JSON.stringify(options)}` : ''}`;

const profiles: ProfileVm[] = [
  { id: 'developer', name: 'Developer', summary: '', content: '', dynamicTemplate: false, scope: 'user', isBuiltIn: false, createdAt: '', updatedAt: '', path: '' },
];

const agents: ManagedAgentVm[] = [
  {
    agentType: 'claude-acp',
    displayName: 'Claude',
    command: 'claude',
    args: [],
    env: [],
    iconKey: 'claude',
    primaryAgentDir: '.claude',
    projectPrimaryAgentDir: null,
    compatibleAgentDirs: [],
    supportsSystemPrompt: true,
    externalSessionSyncSupported: false,
    externalSessionSyncEnabled: false,
    diagnostic: { status: 'ok', available: true, checkedAt: '' },
    supportedModes: [],
    supportedModels: [],
  },
];

function worker(id: string) {
  return {
    type: 'worker' as const,
    id,
    executionSlotId: `slot-${id}`,
    profile: 'developer',
    goal: `Run ${id}`,
  };
}

function validate(workflow: WorkflowDsl) {
  return validateWorkflowForSave(
    workflow,
    profiles,
    agents,
    t,
    null,
    null,
    null,
    false,
    {
      definitionRevision: '',
      bindingRevision: 0,
      bindings: workflow.nodes.flatMap((node) => node.type === 'worker' && node.executionSlotId
        ? [{ executionSlotId: node.executionSlotId, agentId: 'claude-acp' }]
        : []),
    },
  );
}

describe('WorkflowEditor new round entry validation', () => {
  it('derives the saved entry from the only node without ordinary incoming edges', () => {
    const validation = validate({
      version: '0.1',
      id: 'derived-entry',
      entry: 'dev',
      control: {},
      nodes: [worker('dev'), worker('plan')],
      edges: [
        { from: 'plan', to: 'dev', on: 'success' },
        { from: 'dev', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(true);
    expect(validation.sanitizedWorkflow.entry).toBe('plan');
  });

  it('accepts a failure back edge into the initial entry node', () => {
    const validation = validate({
      version: '0.1',
      id: 'failure-back-edge-entry',
      entry: 'dev',
      control: {},
      nodes: [worker('dev'), worker('test'), worker('accept')],
      edges: [
        { from: 'dev', to: 'test', on: 'success' },
        { from: 'test', to: 'accept', on: 'success' },
        { from: 'test', to: 'dev', on: 'failure' },
        { from: 'accept', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(true);
    expect(validation.sanitizedWorkflow.entry).toBe('dev');
  });

  it('counts a forward failure branch as an ordinary incoming edge', () => {
    const validation = validate({
      version: '0.1',
      id: 'forward-failure-branch-entry',
      entry: 'dev',
      control: {},
      nodes: [worker('dev'), worker('fallback')],
      edges: [
        { from: 'dev', to: '$end', on: 'success' },
        { from: 'dev', to: 'fallback', on: 'failure' },
        { from: 'fallback', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(true);
    expect(validation.sanitizedWorkflow.entry).toBe('dev');
  });

  it('rejects multiple nodes without ordinary incoming edges', () => {
    const validation = validate({
      version: '0.1',
      id: 'multiple-entry-candidates',
      entry: 'dev',
      control: {},
      nodes: [worker('plan'), worker('dev')],
      edges: [
        { from: 'plan', to: '$end', on: 'success' },
        { from: 'dev', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(false);
    expect(validation.issues[0].message).toContain('validationEntryCandidateMultiple');
    expect(validation.issues[0].nodeIds).toEqual(['plan', 'dev']);
    expect(validation.sanitizedWorkflow.entry).toBe('');
  });

  it('rejects workflows without any node lacking ordinary incoming edges', () => {
    const validation = validate({
      version: '0.1',
      id: 'missing-entry-candidate',
      entry: 'plan',
      control: {},
      nodes: [worker('plan'), worker('dev')],
      edges: [
        { from: 'plan', to: 'dev', on: 'success' },
        { from: 'dev', to: 'plan', on: 'success' },
        { from: 'dev', to: '$end', on: 'failure' },
      ],
    });

    expect(validation.valid).toBe(false);
    expect(validation.issues[0].message).toContain('validationEntryCandidateMissing');
    expect(validation.sanitizedWorkflow.entry).toBe('');
  });

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

  it('accepts a real node as the next round start when the initial entry is still unique', () => {
    const validation = validate({
      version: '0.1',
      id: 'custom-new-round-entry',
      entry: 'accept',
      control: {},
      nodes: [worker('accept'), worker('dev')],
      edges: [
        { from: 'accept', to: '$new-round', on: 'failure', new_round_entry: 'dev' },
        { from: 'accept', to: 'dev', on: 'success' },
        { from: 'dev', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(true);
  });

  it('does not count new_round_entry as an ordinary incoming edge for initial entry derivation', () => {
    const validation = validate({
      version: '0.1',
      id: 'new-round-entry-is-not-incoming',
      entry: 'accept',
      control: {},
      nodes: [worker('accept'), worker('dev')],
      edges: [
        { from: 'accept', to: '$new-round', on: 'failure', new_round_entry: 'dev' },
        { from: 'accept', to: '$end', on: 'success' },
        { from: 'dev', to: '$end', on: 'success' },
      ],
    });

    expect(validation.valid).toBe(false);
    expect(validation.issues[0].message).toContain('validationEntryCandidateMultiple');
    expect(validation.issues[0].nodeIds).toEqual(['accept', 'dev']);
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
