import type { DesktopPlatform } from '../types';

export interface WindowControlsPolicy {
  showCustomControls: boolean;
  leadingInsetClassName: string;
}

export function resolveWindowControlsPolicy(platform?: DesktopPlatform | null): WindowControlsPolicy {
  if (platform === 'macos') {
    return {
      showCustomControls: false,
      leadingInsetClassName: 'pl-[72px]',
    };
  }

  if (!platform) {
    return {
      showCustomControls: false,
      leadingInsetClassName: 'pl-[72px]',
    };
  }

  return {
    showCustomControls: true,
    leadingInsetClassName: '',
  };
}
