import type { RuntimeApi } from '@/api/client';
import type { PreferencesVm } from '@/types';

export function personalizeSource(base: RuntimeApi): Partial<RuntimeApi> {
  const project = (preferences: PreferencesVm): PreferencesVm => {
    const value = structuredClone(preferences);
    for (const kind of ['agent', 'user'] as const) {
      const shape = value.personalization.avatars[kind].shape;
      value.avatars[kind].shape = shape.source === 'custom' ? shape.value : 'circle';
    }
    return value;
  };
  return {
    async getAppBootstrap() {
      const bootstrap = await base.getAppBootstrap();
      return { ...bootstrap, preferences: project(bootstrap.preferences) };
    },
    async saveDesktopPreferences(...args) { return project(await base.saveDesktopPreferences(...args)); },
    async saveDesktopAvatarShape(kind, shape) {
      const current = (await base.getAppBootstrap()).preferences;
      current.personalization.avatars[kind].shape = shape === null ? { source: 'theme' } : { source: 'custom', value: shape };
      return project(await base.saveDesktopPreferences(current.appearance, current.personalization, current.language, current.useLocalClaude, current.verboseLogging));
    },
  };
}
