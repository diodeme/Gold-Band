/** @vitest-environment jsdom */
// Agent-driven terminal sessions share the backend TerminalSessionManager with
// the user's own sessions, so their events arrive on the same stream. The panel
// must surface a session it did not open itself: when an event names an unknown
// session, the session list refreshes (deduped while in flight) so the tab
// appears, and an auth prompt for it activates that tab for OTP 代输.

import { describe, expect, it, vi, beforeEach } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react';

const h = vi.hoisted(() => {
  const state: {
    listener: ((event: { event: string; sessionId: string; message?: string; code?: string }) => void) | null;
    sessionList: { sessionId: string; kind: string; status: string; cwd: string | null }[];
    loads: number[];
  } = { listener: null, sessionList: [], loads: [] };
  return {
    state,
    emit(event: { event: string; sessionId: string; message?: string; code?: string }) {
      state.listener?.(event);
    },
    setSessionList(next: typeof state.sessionList) { state.sessionList = next; },
  };
});

vi.mock('@/api', () => ({
  listTerminalSessions: () => {
    h.state.loads.push(h.state.loads.length + 1);
    return Promise.resolve(h.state.sessionList);
  },
  listServerConnections: () => Promise.resolve([]),
  listDetachedWindows: () => Promise.resolve([]),
  subscribeDetachedWindowClosed: () => Promise.resolve(() => {}),
  subscribeTerminalEvents: (listener: (event: { event: string; sessionId: string }) => void) => {
    h.state.listener = listener;
    return Promise.resolve(() => {});
  },
  openLocalTerminal: () => Promise.reject(new Error('not used')),
  openRemoteTerminal: () => Promise.reject(new Error('not used')),
  closeTerminalSession: () => Promise.resolve(),
  openDetachedWindow: () => Promise.resolve(),
  pickDirectory: () => Promise.resolve(null),
  provideTerminalAuth: () => Promise.resolve(),
  resizeTerminal: () => Promise.resolve(),
  writeTerminalInput: () => Promise.resolve(),
  getTerminalHistory: () => Promise.resolve(''),
}));

vi.mock('@/lib/feature-availability', () => ({
  useFeatureAvailability: () => ({ remoteConnectionsEnabled: false }),
}));

vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    handler: ((d: string) => void) | null = null;
    selectionHandler: ((e: unknown) => void) | null = null;
    element = document.createElement('div');
    options = {};
    loadAddon() {}
    open() {}
    onData(cb: (d: string) => void) { this.handler = cb; return { dispose() {} }; }
    onSelectionChange(cb: (e: unknown) => void) { this.selectionHandler = cb; return { dispose() {} }; }
    onResize() { return { dispose() {} }; }
    hasSelection() { return false; }
    getSelection() { return ''; }
    clearSelection() {}
    write() {}
    focus() {}
    dispose() {}
  },
}));

vi.mock('@xterm/addon-fit', () => ({ FitAddon: class { loadAddon() {} fit() {} dispose() {} } }));

import { TerminalPanel } from '@/components/terminal/TerminalPanel';
import { TooltipProvider } from '@/components/ui/tooltip';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  document.body.replaceChildren();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  h.setSessionList([]);
  h.state.loads.length = 0;
  h.state.listener = null;
});

const render = async () => {
  Object.assign(globalThis, {
    ResizeObserver: class { observe() {} disconnect() {} unobserve() {} },
  });
  await act(async () => {
    root.render(
      <TooltipProvider>
        <TerminalPanel workspaceRoot="/repo" />
      </TooltipProvider>,
    );
  });
};

const flush = async () => {
  await act(async () => {
    await Promise.resolve();
  });
};

describe('TerminalPanel agent-session visibility', () => {
  it('refreshes the session list when an event names an unknown session', async () => {
    await render();
    await flush();
    const loadsAfterMount = h.state.loads.length;
    expect(loadsAfterMount).toBeGreaterThan(0);

    // The agent opens a session through the shared MCP tools: output events
    // start arriving for a session this panel has never seen.
    h.setSessionList([
      { sessionId: 'sess-agent-1', kind: 'remote', status: 'Connected', cwd: null, label: 'agent-session' },
    ]);
    await act(async () => {
      h.emit({ event: 'output', sessionId: 'sess-agent-1' });
    });
    await flush();

    expect(h.state.loads.length).toBe(loadsAfterMount + 1);
    expect(host.textContent).toContain('agent-session');
  });

  it('dedupes concurrent refreshes while one is in flight', async () => {
    await render();
    await flush();
    const loadsAfterMount = h.state.loads.length;
    h.setSessionList([
      { sessionId: 'sess-agent-2', kind: 'remote', status: 'Connected', cwd: null, label: 'agent-session-2' },
    ]);
    await act(async () => {
      h.emit({ event: 'output', sessionId: 'sess-agent-2' });
      h.emit({ event: 'output', sessionId: 'sess-agent-2' });
      h.emit({ event: 'output', sessionId: 'sess-agent-2' });
    });
    await flush();
    expect(h.state.loads.length).toBe(loadsAfterMount + 1);
  });

  it('does not refresh for sessions it already knows', async () => {
    h.setSessionList([
      { sessionId: 'sess-known', kind: 'local', status: 'Connected', cwd: '/repo' },
    ]);
    await render();
    await flush();
    const loadsAfterMount = h.state.loads.length;
    await act(async () => {
      h.emit({ event: 'output', sessionId: 'sess-known', base64: btoa('hi') });
    });
    await flush();
    expect(h.state.loads.length).toBe(loadsAfterMount);
  });

  it('activates an unknown awaiting-auth session so the OTP prompt is reachable', async () => {
    await render();
    await flush();
    h.setSessionList([
      { sessionId: 'sess-otp', kind: 'remote', status: 'AwaitingAuth', cwd: null, label: 'agent-otp' },
    ]);
    await act(async () => {
      h.emit({
        event: 'authPrompt',
        sessionId: 'sess-otp',
        message: 'Enter the one-time code',
      });
    });
    await flush();
    expect(h.state.loads.length).toBeGreaterThan(1);
    expect(host.textContent).toContain('agent-otp');
    expect(host.textContent).toContain('Enter the one-time code');
  });
});
