/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import { ConversationComposer } from '@/components/conversation/ConversationComposer';
import { ConversationComposerDraftBoundary } from '@/components/conversation/ConversationComposerDraftBoundary';
import { mockAgentRegistry } from '@/mockData';
vi.mock('@/components/git/GitBranchSelector', () => ({ GitBranchSelector: () => null }));

vi.mock('@/components/workspace/workspace-file-reference-bridge', () => ({
  useWorkspaceFileReferenceBridge: () => ({ register: () => () => {} }),
  useWorkspaceFileReferenceCommands: () => null,
  useWorkspaceFileReferencePresentation: () => ({ isDocked: false }),
  useComposerQuoteCommand: () => null,
}));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
it('receiving model capabilities updates choices without automatically saving user preferences', async () => {
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  const host = document.body.appendChild(document.createElement('div'));
  const root = createRoot(host);
  const save = vi.fn();
  const registry = structuredClone(mockAgentRegistry);
  const agent = registry.agents[0];
  agent.configOptions = [];
  agent.modelBoundCatalogs = {};
  agent.supportedModels = [{ id: 'target', name: 'Target' }];
  const props: React.ComponentProps<typeof ConversationComposer> = {
    projectId: 'project', workspaceName: 'Project', workspaces: [],
    runMode: { mode: 'direct', directConfig: { agentType: agent.agentType, modelId: 'target', configOptions: { context: '500k' } } },
    agentRegistry: registry, workflowTemplates: null, profiles: [], busy: false,
    inlineContentMaxBytes: 10000, workLocation: 'main', onRunModeChange: save,
    onLoadProfiles: async () => [], onSubmit: vi.fn(), onOpenAgentManagement: vi.fn(),
    onOpenScheduledTasks: vi.fn(), onOpenRunModeSettings: vi.fn(), onWorkspaceChange: vi.fn(), onWorkLocationChange: vi.fn(),
  };
  try {
    await act(async () => root.render(<ConversationComposerDraftBoundary><ConversationComposer {...props} /></ConversationComposerDraftBoundary>));
    save.mockClear();
    const observed = { ...registry, agents: [{ ...agent, modelBoundCatalogs: { target: [] } }, ...registry.agents.slice(1)] };
    await act(async () => root.render(<ConversationComposerDraftBoundary><ConversationComposer {...props} agentRegistry={observed} /></ConversationComposerDraftBoundary>));
    expect(save).not.toHaveBeenCalled();
  } finally { await act(async () => root.unmount()); host.remove(); vi.unstubAllGlobals(); }
});
