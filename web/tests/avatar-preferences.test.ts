import { describe, expect, it } from 'vitest';
import fs from 'node:fs';
import path from 'node:path';
import { browserApi } from '../src/api/browser';
import { avatarShapeClass, createDefaultAvatarPreferences, selectedAvatar } from '../src/lib/avatar';

describe('avatar preferences', () => {
  it('starts with separate unset Agent and personal avatars', () => {
    const preferences = createDefaultAvatarPreferences();
    expect(preferences.agent).toEqual({ shape: 'circle', selectedAvatarId: null, recentAvatars: [] });
    expect(preferences.user).toEqual({ shape: 'circle', selectedAvatarId: null, recentAvatars: [] });
  });

  it('resolves the selected avatar and frame class from the profile contract', () => {
    const profile = {
      shape: 'square' as const,
      selectedAvatarId: 'avatar-2',
      recentAvatars: [
        { id: 'avatar-1', dataUrl: 'data:image/webp;base64,AQ==', createdAt: '2026-01-01T00:00:00Z' },
        { id: 'avatar-2', dataUrl: 'data:image/webp;base64,Ag==', createdAt: '2026-01-02T00:00:00Z' },
      ],
    };
    expect(selectedAvatar(profile)?.id).toBe('avatar-2');
    expect(avatarShapeClass(profile.shape)).toBe('rounded-md');
    expect(avatarShapeClass('circle')).toBe('rounded-full');
  });

  it('persists upload, recent selection, shape, and the recent-10 limit through the browser API', async () => {
    let avatars = createDefaultAvatarPreferences();
    for (let index = 0; index < 11; index += 1) {
      avatars = await browserApi.saveDesktopAvatar({
        kind: 'agent',
        shape: 'circle',
        mimeType: 'image/webp',
        dataBase64: btoa(`avatar-${index}`),
      });
    }
    expect(avatars.agent.recentAvatars).toHaveLength(10);
    const selectedId = avatars.agent.recentAvatars[5].id;
    avatars = await browserApi.selectRecentDesktopAvatar('agent', selectedId);
    expect(avatars.agent.selectedAvatarId).toBe(selectedId);
    expect(avatars.agent.recentAvatars[0].id).toBe(selectedId);
    avatars = await browserApi.saveDesktopAvatarShape('agent', 'square');
    expect(avatars.agent.shape).toBe('square');
    const recentIds = avatars.agent.recentAvatars.map((avatar) => avatar.id);
    avatars = await browserApi.clearDesktopAvatar('agent');
    expect(avatars.agent.selectedAvatarId).toBeNull();
    expect(avatars.agent.recentAvatars.map((avatar) => avatar.id)).toEqual(recentIds);
  });

  it('keeps real message avatars at 36px and structured ACP rows on an aligned spacer', () => {
    const avatarSource = fs.readFileSync(path.resolve(__dirname, '../src/components/acp/AcpAvatarWithTime.tsx'), 'utf8');
    const chatSource = fs.readFileSync(path.resolve(__dirname, '../src/components/acp/ACPChatDialog.tsx'), 'utf8');
    expect(avatarSource).toContain("'mt-0.5 size-9'");
    expect(chatSource).toContain('<div className="w-9 shrink-0" aria-hidden="true" />');
  });

  it('places compact avatar settings after appearance and typography', () => {
    const settingsSource = fs.readFileSync(path.resolve(__dirname, '../src/pages/SettingsPage.tsx'), 'utf8');
    const avatarSettingsSource = fs.readFileSync(path.resolve(__dirname, '../src/components/settings/AvatarSettings.tsx'), 'utf8');
    const appSource = fs.readFileSync(path.resolve(__dirname, '../src/App.tsx'), 'utf8');
    const appearanceIndex = settingsSource.indexOf("<SettingsSection title={t('settings.appearance')}>");
    const typographyIndex = settingsSource.indexOf("<SettingsSection title={t('settings.typography')} divided>");
    const avatarIndex = settingsSource.indexOf("<SettingsSection title={t('settings.avatar.title')} divided>");

    expect(appearanceIndex).toBeGreaterThan(-1);
    expect(typographyIndex).toBeGreaterThan(appearanceIndex);
    expect(avatarIndex).toBeGreaterThan(typographyIndex);
    expect(avatarSettingsSource).toContain('className="size-12 transition group-hover:brightness-90"');
    expect(avatarSettingsSource).toContain('grid min-w-0 grid-cols-[auto_minmax(0,1fr)]');
    expect(avatarSettingsSource).toContain('<DropdownMenuContent align="start" sideOffset={6}');
    expect(avatarSettingsSource).toContain('group-hover/avatar:opacity-100');
    expect(avatarSettingsSource).toContain('<Minus className="size-2.5" />');
    expect(avatarSettingsSource).not.toContain('group-focus-within/avatar:opacity-100');
    expect(avatarSettingsSource).not.toContain('<Trash2');
    expect(avatarSettingsSource).toContain("onClick={() => void updateProfile(() => onClearAvatar(kind))}");
    expect(avatarSettingsSource).not.toContain("{t('settings.avatar.shape')}");
    expect(avatarSettingsSource).toContain('const updatingRef = useRef(false);');
    expect(avatarSettingsSource).toContain('if (updatingRef.current) return;');
    expect(avatarSettingsSource).toContain('const disabled = busy || saving;');
    expect(avatarSettingsSource).not.toContain('busy || saving || updating');
    const avatarHandlers = appSource.slice(appSource.indexOf('const onSaveAvatar ='), appSource.indexOf('const onSaveUpdaterSettings ='));
    expect(avatarHandlers).not.toContain('setBusy(true)');
  });
});
