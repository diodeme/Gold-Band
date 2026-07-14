import { describe, expect, it } from 'vitest';
import { browserApi } from '../../src/api/browser';

describe('browserApi', () => {
  it('keeps the current preview workspace in the recent list', async () => {
    const bootstrap = await browserApi.getAppBootstrap();

    await expect(browserApi.removeRecentWorkspace(bootstrap.repoRoot)).rejects.toEqual({
      code: 'workspace.recent-current-locked',
      params: { workspace: bootstrap.repoRoot },
    });
  });

  it('keeps built-in profiles readonly in preview mode', async () => {
    const builtIn = (await browserApi.getProfiles()).profiles.find((profile) => profile.isBuiltIn);

    expect(builtIn).toBeDefined();
    await expect(browserApi.deleteProfile(builtIn!.id)).rejects.toEqual({
      code: 'profile.readonly-built-in',
      params: {},
    });
  });

  it('requires explicit force before deleting confirmation-gated preview profiles', async () => {
    const created = await browserApi.createProfile({
      name: 'Needs confirmation',
      summary: 'preview role [requires-confirmation]',
      content: 'temp',
    });
    expect(created.scope).toBe('user');

    await expect(browserApi.deleteProfile(created.id)).rejects.toEqual({
      code: 'profile.delete-confirmation-required',
      params: {
        templateCount: 1,
        taskCount: 1,
        runCount: 0,
      },
    });

    const list = await browserApi.deleteProfile(created.id, true);
    expect(list.profiles.some((profile) => profile.id === created.id)).toBe(false);
  });
});
