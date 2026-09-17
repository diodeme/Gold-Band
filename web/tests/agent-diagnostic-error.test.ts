import { describe, expect, it } from 'vitest';
import i18n from '../src/i18n';
import {
  agentDiagnosticBannerReason,
  agentDiagnosticDetail,
  agentDiagnosticHelpReason,
  agentDiagnosticMessage,
  agentDiagnosticRawReason,
} from '../src/lib/agent-diagnostic';
import type { ManagedAgentDiagnosticVm } from '../src/types';

const stderrDiagnostic: ManagedAgentDiagnosticVm = {
  status: 'unhealthy',
  available: false,
  checkedAt: '2026-09-18T00:00:00Z',
  error: {
    code: 'acp.adapter-exited',
    params: {
      method: 'initialize',
      exitCode: 1,
      reason:
        'npm error code ENOENT\nnpm error path C:\\Users\\Administrator\\AppData\\Local\\npm-cache\\_npx\\dead\\package.json',
    },
  },
};

describe('agent diagnostic copy', () => {
  it('localizes the doctor sentence and keeps bounded stderr as raw detail', () => {
    const t = i18n.t.bind(i18n);
    const message = agentDiagnosticMessage(t, stderrDiagnostic);
    expect(message).toBe(
      i18n.t('errors.acp.adapter-exited-with-code', { method: 'initialize', exitCode: 1 }),
    );
    expect(message).not.toContain('ACP adapter transport interrupted');
    expect(message).not.toContain('ENOENT');
    expect(agentDiagnosticDetail(stderrDiagnostic)).toContain('ENOENT');
    expect(agentDiagnosticDetail(stderrDiagnostic)).toContain('package.json');
    expect(agentDiagnosticRawReason(stderrDiagnostic)).toContain('ENOENT');
    expect(agentDiagnosticRawReason(stderrDiagnostic)).toContain('package.json');
    expect(agentDiagnosticBannerReason(t, stderrDiagnostic)).toBe('npm error code ENOENT');
    expect(agentDiagnosticHelpReason(t, stderrDiagnostic)).toContain('package.json');
    expect(agentDiagnosticHelpReason(t, stderrDiagnostic)).not.toBe(message);
  });

  it('surfaces ACP session/new raw message instead of the generic localized sentence', () => {
    const t = i18n.t.bind(i18n);
    const diagnostic: ManagedAgentDiagnosticVm = {
      status: 'unhealthy',
      available: false,
      checkedAt: '2026-09-18T00:00:00Z',
      error: {
        code: 'acp.session-request-failed',
        params: { method: 'session/new' },
        raw: {
          code: -32000,
          message: 'Authentication required',
          data: { category: 'auth' },
        },
      },
    };
    expect(agentDiagnosticMessage(t, diagnostic)).toBe(i18n.t('errors.acp.session-request-failed'));
    expect(agentDiagnosticRawReason(diagnostic)).toBe('Authentication required');
    expect(agentDiagnosticBannerReason(t, diagnostic)).toBe('Authentication required');
    expect(agentDiagnosticHelpReason(t, diagnostic)).toBe('Authentication required');
  });
});
