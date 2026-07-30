import type { AvatarPreferencesVm, AvatarProfileVm, AvatarShape } from '@/types';

export function createDefaultAvatarProfile(): AvatarProfileVm {
  return {
    shape: 'circle',
    selectedAvatarId: null,
    recentAvatars: [],
  };
}

export function createDefaultAvatarPreferences(): AvatarPreferencesVm {
  return {
    agent: createDefaultAvatarProfile(),
    user: createDefaultAvatarProfile(),
  };
}

export function selectedAvatar(profile: AvatarProfileVm) {
  return profile.recentAvatars.find((avatar) => avatar.id === profile.selectedAvatarId) ?? null;
}

export function avatarShapeClass(shape: AvatarShape) {
  return shape === 'circle' ? 'rounded-full' : 'rounded-md';
}
