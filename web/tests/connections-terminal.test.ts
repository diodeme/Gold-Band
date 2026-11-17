import { describe, expect, it } from 'vitest';
import {
  MAX_TERMINAL_SESSIONS,
  isTerminalSessionLimitReached,
  sessionLabel,
  terminalFailedKey,
  terminalFailedMessage,
} from '../src/components/terminal/sessionModel';
import type { TerminalSessionVm } from '../src/types';

const t = (key: string) => key;

describe('terminal session helpers', () => {
  it('labels a local session by its working-directory basename', () => {
    const session: TerminalSessionVm = {
      sessionId: 'sess-1',
      kind: 'local',
      cwd: '/Users/me/repos/gold-band',
      status: 'Connected',
    };
    expect(sessionLabel(t as never, session, 0)).toBe('gold-band');
  });

  it('falls back to the local label for a session without a cwd', () => {
    const session: TerminalSessionVm = { sessionId: 'sess-2', kind: 'local', cwd: null, status: 'Connected' };
    expect(sessionLabel(t as never, session, 1)).toBe('terminal.localLabel');
  });

  it('labels a remote session by its backend label', () => {
    const session: TerminalSessionVm = { sessionId: 'sess-3', kind: 'remote', label: 'web-01 (10.0.0.5:22)', status: 'Connected' };
    expect(sessionLabel(t as never, session, 0)).toBe('web-01 (10.0.0.5:22)');
  });

  it('falls back to a numbered label for a remote session without one', () => {
    const session: TerminalSessionVm = { sessionId: 'sess-4', kind: 'remote', label: null, status: 'Connected' };
    expect(sessionLabel(t as never, session, 3)).toBe('terminal.sessionLabel');
  });

  it('enforces the shared session ceiling', () => {
    expect(isTerminalSessionLimitReached(0, MAX_TERMINAL_SESSIONS)).toBe(false);
    expect(isTerminalSessionLimitReached(MAX_TERMINAL_SESSIONS - 1, MAX_TERMINAL_SESSIONS)).toBe(false);
    expect(isTerminalSessionLimitReached(MAX_TERMINAL_SESSIONS, MAX_TERMINAL_SESSIONS)).toBe(true);
  });
});

describe('terminal failed-code → message mapping', () => {
  it('maps known structured codes to a stable i18n key', () => {
    expect(terminalFailedKey('terminal.host-key-mismatch')).toBe('terminal.failed.hostKeyMismatch');
    expect(terminalFailedKey('terminal.credential-missing')).toBe('terminal.failed.credentialMissing');
    expect(terminalFailedKey('terminal.connect-timeout')).toBe('terminal.failed.connectTimeout');
  });

  it('falls back to the generic error for unknown or missing codes', () => {
    expect(terminalFailedKey('terminal.unknown-code')).toBeNull();
    expect(terminalFailedKey(null)).toBeNull();
    expect(terminalFailedMessage(t as never, 'terminal.unknown-code')).toBe('terminal.error');
    expect(terminalFailedMessage(t as never, null)).toBe('terminal.error');
  });
});
