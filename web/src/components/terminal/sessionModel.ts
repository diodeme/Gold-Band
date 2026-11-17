import type { TFunction } from 'i18next';
import type { TerminalSessionVm } from '@/types';

/** Session ceiling, mirrored from the backend `MAX_TERMINAL_SESSIONS`. The
 *  backend enforces the real limit; this drives the disabled state and hint. */
export const MAX_TERMINAL_SESSIONS = 8;

export function isTerminalSessionLimitReached(count: number, limit: number): boolean {
  return count >= limit;
}

/** Short label for the session tab bar. Local sessions are titled by the
 *  working-directory basename so a workspace root is recognizable at a glance;
 *  remote sessions carry the backend `连接名 (host:port)` label. */
export function sessionLabel(t: TFunction, session: TerminalSessionVm, index: number): string {
  if (session.kind === 'remote') {
    return session.label?.trim() || t('terminal.sessionLabel', { index: index + 1 });
  }
  const cwd = session.cwd?.trim();
  if (cwd) {
    const segments = cwd.split('/').filter(Boolean);
    return segments[segments.length - 1] ?? cwd;
  }
  return t('terminal.localLabel');
}

/** Maps a structured backend terminal error code to a `terminal.failed.*` i18n
 *  key. Copy never lives in the backend; the frontend owns all terminal copy. */
const TERMINAL_FAILED_KEYS: Record<string, string> = {
  'terminal.session-not-found': 'terminal.failed.sessionNotFound',
  'terminal.session-limit': 'terminal.failed.sessionLimit',
  'terminal.jump-forward-failed': 'terminal.failed.jumpForwardFailed',
  'terminal.auth-required': 'terminal.failed.authRequired',
  'terminal.credential-missing': 'terminal.failed.credentialMissing',
  'terminal.ssh-auth-failed': 'terminal.failed.authFailed',
  'terminal.host-key-mismatch': 'terminal.failed.hostKeyMismatch',
  'terminal.connect-timeout': 'terminal.failed.connectTimeout',
  'terminal.connection-lost': 'terminal.failed.connectionLost',
  'terminal.channel-open-failed': 'terminal.failed.channelFailed',
  'terminal.local-start-failed': 'terminal.failed.localStartFailed',
  'terminal.invalid-cwd': 'terminal.failed.invalidCwd',
  'terminal.io': 'terminal.failed.io',
};

export function terminalFailedKey(code: string | null | undefined): string | null {
  return code ? TERMINAL_FAILED_KEYS[code] ?? null : null;
}

export function terminalFailedMessage(t: TFunction, code: string | null | undefined): string {
  const key = terminalFailedKey(code);
  return key ? t(key) : t('terminal.error');
}
