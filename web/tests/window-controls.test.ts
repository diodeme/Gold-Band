import { describe, expect, it } from 'vitest';
import { resolveWindowControlsPolicy } from '../src/lib/window-controls';

describe('resolveWindowControlsPolicy', () => {
  it('does not reserve native window controls in a browser presentation', () => {
    expect(resolveWindowControlsPolicy(null, 'browser')).toEqual({
      showCustomControls: false, leadingInsetClassName: '',
    });
    expect(resolveWindowControlsPolicy(null).leadingInsetClassName).toBe('pl-[72px]');
  });
  it('uses native macOS traffic lights without custom controls', () => {
    expect(resolveWindowControlsPolicy('macos')).toEqual({
      showCustomControls: false,
      leadingInsetClassName: 'pl-[72px]',
    });
  });

  it('keeps custom controls on non-macOS platforms', () => {
    expect(resolveWindowControlsPolicy('windows')).toEqual({
      showCustomControls: true,
      leadingInsetClassName: '',
    });
    expect(resolveWindowControlsPolicy('linux')).toEqual({
      showCustomControls: true,
      leadingInsetClassName: '',
    });
    expect(resolveWindowControlsPolicy('unknown')).toEqual({
      showCustomControls: true,
      leadingInsetClassName: '',
    });
  });
});
