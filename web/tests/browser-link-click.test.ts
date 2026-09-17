/** @vitest-environment jsdom */

import fs from 'node:fs';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';

const script = fs.readFileSync(
  path.resolve(__dirname, '../src/components/workspace/browser/browser-link-click.js'),
  'utf8',
);

function installScript() {
  window.eval(script);
}

describe('in-page browser link clicks', () => {
  afterEach(() => {
    document.body.replaceChildren();
    vi.restoreAllMocks();
  });

  it('navigates the current page when a search result link is left-clicked', () => {
    document.body.innerHTML = '<a href="https://github.com/">GitHub</a>';
    const assign = vi.fn();
    vi.spyOn(window, 'location', 'get').mockReturnValue({
      href: 'https://www.google.com/search?q=github',
      assign,
    } as unknown as Location);
    installScript();
    const link = document.querySelector('a');
    const event = new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 });
    const prevented = !link!.dispatchEvent(event);
    expect(prevented || event.defaultPrevented).toBe(true);
    expect(assign).toHaveBeenCalledWith('https://github.com/');
  });

  it('beats site click handlers that preventDefault and try to open a new window', () => {
    document.body.innerHTML = '<a href="https://github.com/">GitHub</a>';
    const assign = vi.fn();
    const open = vi.fn();
    vi.spyOn(window, 'location', 'get').mockReturnValue({
      href: 'https://www.google.com/search?q=github',
      assign,
    } as unknown as Location);
    vi.stubGlobal('open', open);
    document.querySelector('a')!.addEventListener('click', (event) => {
      event.preventDefault();
      window.open('https://github.com/');
    });
    installScript();
    document.querySelector('a')!.dispatchEvent(new MouseEvent('click', {
      bubbles: true,
      cancelable: true,
      button: 0,
    }));
    expect(open).not.toHaveBeenCalled();
    expect(assign).toHaveBeenCalledWith('https://github.com/');
  });

  it('leaves ctrl-click to the native new-window path', () => {
    document.body.innerHTML = '<a href="https://github.com/">GitHub</a>';
    const assign = vi.fn();
    vi.spyOn(window, 'location', 'get').mockReturnValue({
      href: 'https://www.google.com/search?q=github',
      assign,
    } as unknown as Location);
    installScript();
    const event = new MouseEvent('click', {
      bubbles: true,
      cancelable: true,
      button: 0,
      ctrlKey: true,
    });
    event.preventDefault();
    document.querySelector('a')!.dispatchEvent(event);
    expect(assign).not.toHaveBeenCalled();
  });
});
