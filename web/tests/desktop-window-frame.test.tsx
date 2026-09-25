// @vitest-environment jsdom

import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { DesktopWindowFrame } from '../src/components/DesktopWindowFrame';

describe('DesktopWindowFrame', () => {
  it.each(['app-outline', 'native-compositor'] as const)('separates %s chrome from themed content', (frameStyle) => {
    const container = document.createElement('div');
    container.innerHTML = renderToStaticMarkup(
      <DesktopWindowFrame frameStyle={frameStyle}>
        <div className="app-window-shell" data-theme-role="shell" data-theme-wallpaper-slot="app">
          <header data-window-occludes-desktop="false"><button>Close</button></header>
        </div>
      </DesktopWindowFrame>,
    );
    const frame = container.firstElementChild!;
    const shell = frame.firstElementChild!;
    expect(frame.getAttribute('data-window-frame-style')).toBe(frameStyle);
    expect(frame.hasAttribute('data-theme-wallpaper-slot')).toBe(false);
    expect(frame.hasAttribute('data-theme-role')).toBe(false);
    expect(frame.classList.contains('overflow-hidden')).toBe(true);
    expect(shell.getAttribute('data-theme-wallpaper-slot')).toBe('app');
    expect(frame.querySelectorAll('button')).toHaveLength(1);
    const header = shell.querySelector('header')!;
    const hiddenSelector = ':scope > .app-window-shell > [data-window-occludes-desktop="true"]';
    expect(frame.querySelector(hiddenSelector)).toBeNull();
    header.setAttribute('data-window-occludes-desktop', 'true');
    expect(frame.querySelector(hiddenSelector)).toBe(header);
  });
});
