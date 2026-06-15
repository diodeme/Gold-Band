import { describe, expect, it } from 'vitest';
import { resolveWindowControlsPolicy, shouldApplyRuntimeWindowPolicy } from '../src/lib/window-controls';

describe('resolveWindowControlsPolicy', () => {
  it('uses native macOS traffic lights with overlay title bar', () => {
    expect(resolveWindowControlsPolicy('macos')).toEqual({
      decorations: true,
      titleBarStyle: 'overlay',
      showCustomControls: false,
      leadingInsetClassName: 'pl-[72px]',
    });
    expect(shouldApplyRuntimeWindowPolicy('macos')).toBe(false);
  });

  it('keeps undecorated custom controls on windows', () => {
    expect(resolveWindowControlsPolicy('windows')).toEqual({
      decorations: false,
      titleBarStyle: null,
      showCustomControls: true,
      leadingInsetClassName: '',
    });
    expect(shouldApplyRuntimeWindowPolicy('windows')).toBe(true);
  });
});
