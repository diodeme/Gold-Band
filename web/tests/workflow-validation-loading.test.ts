import { describe, expect, it, vi } from 'vitest';

vi.mock('@/components/WorkflowEditor', () => {
  throw new Error('Pure run-mode validation loaded the workflow editor UI');
});
vi.mock('@/components/GraphView', () => {
  throw new Error('Registering run resources loaded the workflow graph UI');
});
vi.mock('@/components/workflowGraph', () => {
  throw new Error('Pure run-mode validation loaded graph layout dependencies');
});
vi.mock('@/components/acp/ACPChatDialog', () => ({ RawFrameViewer: () => null, SystemPromptPanel: () => null }));

describe('run-mode validation loading boundary', () => {
  it('loads and validates Direct configuration without evaluating editor UI', async () => {
    const validation = await import('@/lib/run-mode-validation');
    expect(validation.validateDirectConfig(null, null, key => key)).toEqual(['conversation.home.selectAgent']);
    expect(validation.groupSelectableAgentOptions([])).toEqual({ selectable: [], unavailable: [] });
  });
  it('registers run resource panels without evaluating graph or editor UI', async () => {
    const resources = await import('@/components/workspace/ConversationRunWorkspaceResourcePanel');
    expect(resources.confirmCloseConversationRunWorkspaceResource).toBeTypeOf('function');
  });
});
