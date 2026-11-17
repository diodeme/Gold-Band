/** @vitest-environment jsdom */
// Verifies TerminalView's input wiring contract: when xterm fires `onData`
// (a keystroke or paste reaching the terminal), TerminalView must call
// `writeTerminalInput(sessionId, base64)` with the session id.
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';
import { TerminalView } from '@/components/terminal/TerminalView';

const h = vi.hoisted(() => {
  const terms: { handler: ((d: string) => void) | null }[] = [];
  const writes: string[][] = [];
  return { terms, writes };
});

vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    handler: ((d: string) => void) | null = null;
    element = document.createElement('div');
    options = {};
    constructor() { h.terms.push(this); }
    loadAddon() {}
    open() {}
    onData(cb: (d: string) => void) { this.handler = cb; return { dispose() {} }; }
    onResize() { return { dispose() {} }; }
    write() {}
    focus() {}
    dispose() {}
  },
}));

vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { loadAddon() {} fit() {} } }));

vi.mock('@/api', () => ({
  subscribeTerminalEvents: () => Promise.resolve(() => {}),
  resizeTerminal: () => Promise.resolve(),
  getTerminalHistory: () => Promise.resolve(''),
  writeTerminalInput: (sessionId: string, base64: string) => {
    h.writes.push([sessionId, base64]);
    return Promise.resolve();
  },
}));

const decodeBase64 = (base64: string) => {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return new TextDecoder().decode(bytes);
};

describe('TerminalView input wiring', () => {
  beforeEach(() => {
    h.terms.length = 0;
    h.writes.length = 0;
    document.body.replaceChildren();
  });

  it('routes xterm onData to writeTerminalInput with the session id', async () => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true, ResizeObserver: class { observe() {} disconnect() {} } });
    const host = document.createElement('div');
    document.body.appendChild(host);
    let root: Root = createRoot(host);
    await act(async () => { root.render(<TerminalView sessionId="sess-1" />); });

    const term = h.terms[0];
    expect(term).toBeDefined();
    expect(term.handler).not.toBeNull();

    await act(async () => {
      term.handler?.('hello');
    });

    expect(h.writes.length).toBe(1);
    expect(h.writes[0][0]).toBe('sess-1');
    expect(decodeBase64(h.writes[0][1])).toBe('hello');
  });
});
