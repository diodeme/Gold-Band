import { describe, expect, it, vi } from 'vitest';

async function loadBrowserApi() {
  vi.resetModules();
  const module = await import('./browser');
  return module.browserApi;
}

describe('browserApi', () => {
  it('keeps built-in profiles readonly in preview mode', async () => {
    const browserApi = await loadBrowserApi();
    const builtIn = (await browserApi.getProfiles()).profiles.find((profile) => profile.isBuiltIn);

    expect(builtIn).toBeDefined();
    await expect(browserApi.deleteProfile(builtIn!.id)).rejects.toEqual({
      code: 'profile.readonly-built-in',
      params: {},
    });
  });

  it('requires explicit force before deleting confirmation-gated preview profiles', async () => {
    const browserApi = await loadBrowserApi();
    const created = await browserApi.createProfile({
      scope: 'user',
      name: 'Needs confirmation',
      summary: 'preview role [requires-confirmation]',
      content: 'temp',
    });

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
