import type { DesktopPlatform } from '../types';

export interface WindowControlsPolicy {
  decorations: boolean;
  titleBarStyle: 'overlay' | null;
  showCustomControls: boolean;
  leadingInsetClassName: string;
}

export function shouldApplyRuntimeWindowPolicy(platform: DesktopPlatform): boolean {
  return platform !== 'macos';
}

export function resolveWindowControlsPolicy(platform: DesktopPlatform): WindowControlsPolicy {
  if (platform === 'macos') {
    return {
      decorations: true,
      titleBarStyle: 'overlay',
      showCustomControls: false,
      leadingInsetClassName: 'pl-[72px]',
    };
  }

  return {
    decorations: false,
    titleBarStyle: null,
    showCustomControls: true,
    leadingInsetClassName: '',
  };
}
