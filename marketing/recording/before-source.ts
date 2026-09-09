import type { RuntimeApi } from '@/api/client';
import type { Language } from '../site/content';
import { workflowDefinition } from './workflow-source';

// The configuration scene selects the same authoring definition used by the execution scene.
export function beforeSource(base: RuntimeApi, language: Language): Partial<RuntimeApi> {
  return {
    async getWorkflowTemplates() {
      const store = await base.getWorkflowTemplates();
      const workflow = workflowDefinition();
      return { ...store, templates: [...store.templates, {
        id: workflow.id, name: language === 'en' ? 'Workspace configuration and docs' : '为工作区补充配置与说明',
        isBuiltIn: false, workflow,
        modelBindings: { definitionRevision: 'recording-1', bindingRevision: 1,
          bindings: workflow.nodes.flatMap(node => node.type === 'worker' && node.executionSlotId
            ? [{ executionSlotId: node.executionSlotId, agentId: 'claude-acp' }] : []) },
        createdAt: store.templates[0].createdAt, updatedAt: store.templates[0].updatedAt,
      }] };
    },
  };
}
