import { describe, expect, it } from 'vitest';
import { BrowserPreviewState } from '../../src/api/browserState';

describe('BrowserPreviewState', () => {
  it('returns defensive profile copies', () => {
    const state = new BrowserPreviewState();
    const initial = state.getProfiles();
    const target = initial.profiles.find((profile) => !profile.isBuiltIn) ?? initial.profiles[0];
    const originalName = target.name;

    initial.profiles[0].name = 'mutated outside state';

    expect(state.getProfile(target.id)?.name).toBe(originalName);
  });

  it('deep clones workflow templates', () => {
    const state = new BrowserPreviewState();
    const store = state.getWorkflowTemplates();
    const template = store.templates[0];

    if (!template) {
      expect(store.templates).toHaveLength(0);
      return;
    }

    const clone = state.getWorkflowTemplates();
    clone.templates[0].workflow.id = 'mutated-workflow';

    expect(state.getWorkflowTemplates().templates[0]?.workflow.id).toBe(template.workflow.id);
  });
});
