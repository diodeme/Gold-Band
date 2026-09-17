import { describe, expect, it } from 'vitest';
import i18n from '../src/i18n';
import { agentDiagnosticDetail, agentDiagnosticMessage } from '../src/lib/agent-diagnostic';
import type { ManagedAgentDiagnosticVm } from '../src/types';

const diagnostic: ManagedAgentDiagnosticVm = {
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
    const message = agentDiagnosticMessage(t, diagnostic);
    expect(message).toBe(
      i18n.t('errors.acp.adapter-exited-with-code', { method: 'initialize', exitCode: 1 }),
    );
    expect(message).not.toContain('ACP adapter transport interrupted');
    expect(message).not.toContain('ENOENT');
    expect(agentDiagnosticDetail(diagnostic)).toContain('ENOENT');
    expect(agentDiagnosticDetail(diagnostic)).toContain('package.json');
  });
});
