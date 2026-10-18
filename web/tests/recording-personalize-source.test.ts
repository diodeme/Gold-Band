import { expect, it } from 'vitest';
import { createRecordingApi } from '../../marketing/recording/runtime';

it('keeps avatar shape in the recording preference source and isolates other instances', async () => {
  const source = createRecordingApi({ scene: 'personalize', language: 'en' });
  const other = createRecordingApi();
  const saved = await source.saveDesktopAvatarShape('user', 'square');
  expect(saved.personalization.avatars.user.shape).toEqual({ source: 'custom', value: 'square' });
  expect(saved.avatars.user.shape).toBe('square');
  expect((await source.getAppBootstrap()).preferences).toEqual(saved);
  expect((await other.getAppBootstrap()).preferences.avatars.user.shape).toBe('circle');
  const updated = await source.saveDesktopPreferences(saved.appearance, saved.personalization, 'en', false, false);
  expect(updated.avatars.user.shape).toBe('square');
  expect((await source.saveDesktopAvatarShape('user', null)).personalization.avatars.user.shape).toEqual({ source: 'theme' });
});
