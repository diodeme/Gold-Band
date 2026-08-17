import { describe, expect, it } from 'vitest';
import { browserApi } from '@/api/browser';

describe('browser conversation run mode API', () => {
  it('round-trips optional entry preferences without sharing workspace state', async () => {
    const projectA = `browser-run-mode-a-${Date.now()}`;
    const projectB = `browser-run-mode-b-${Date.now()}`;
    const mode = {
      mode: 'workflow' as const,
      workflowTemplateId: 'default-lightweight',
      optionalEntryPreferences: {
        default: true,
        'default-lightweight': false,
      },
    };

    await browserApi.saveConversationRunMode(projectA, mode);
    mode.optionalEntryPreferences.default = false;

    expect(await browserApi.getConversationRunMode(projectA)).toEqual({
      mode: 'workflow',
      workflowTemplateId: 'default-lightweight',
      optionalEntryPreferences: {
        default: true,
        'default-lightweight': false,
      },
    });
    expect(await browserApi.getConversationRunMode(projectB)).toEqual({ mode: 'auto' });
  });

  it('returns a clone so callers cannot mutate the saved preferences', async () => {
    const projectId = `browser-run-mode-clone-${Date.now()}`;
    await browserApi.saveConversationRunMode(projectId, {
      mode: 'workflow',
      workflowTemplateId: 'default',
      optionalEntryPreferences: { default: true },
    });

    const loaded = await browserApi.getConversationRunMode(projectId);
    if (loaded?.optionalEntryPreferences) loaded.optionalEntryPreferences.default = false;

    expect((await browserApi.getConversationRunMode(projectId))?.optionalEntryPreferences).toEqual({
      default: true,
    });
  });
});
