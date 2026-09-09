import { describe, expect, it } from 'vitest';
import { createRecordingApi } from '../../marketing/recording/runtime';
import { workflowDefinition } from '../../marketing/recording/workflow-source';

describe('before recording configuration', () => {
  it.each(['zh', 'en'] as const)('selects the execution definition without changing the base factory (%s)', async language => {
    const base = createRecordingApi();
    const source = createRecordingApi({ scene: 'before', language });
    const templates = await source.getWorkflowTemplates();
    const definition = workflowDefinition();
    const selected = templates.templates.find(template => template.id === definition.id)!;
    expect(selected.workflow).toEqual(definition);
    expect(selected.modelBindings.bindings.map(binding => binding.executionSlotId)).toEqual(definition.nodes.map(node => node.executionSlotId));
    expect((await base.getWorkflowTemplates()).templates.some(template => template.id === definition.id)).toBe(false);
    expect(Object.getOwnPropertyDescriptor(source, 'scenario')?.value).toBeUndefined();
    const preferences = (await source.getAppBootstrap()).preferences;
    await source.saveDesktopPreferences(preferences.appearance, preferences.personalization, language === 'en' ? 'en' : 'zh-cn', false, false);
    const profiles = (await source.getProfiles()).profiles;
    expect(profiles.find(profile => !profile.isBuiltIn)?.content.length).toBeGreaterThan(40);
    const skills = await source.listSkills();
    expect(skills.global[0].syncedAgentTypes).toEqual(expect.arrayContaining(['claude-acp', 'codex-acp']));
  });
});
