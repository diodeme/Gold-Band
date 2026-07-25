import { describe, expect, it } from 'vitest';
import { buildAgentInput, hasManagedAgentInputChanged } from '../src/pages/AgentManagementPage';

describe('Agent management input mapping', () => {
  it('trims command boundaries and normalizes editable config input', () => {
    expect(buildAgentInput({
      displayName: 'Claude',
      command: '  npx  ',
      args: ['stale'],
      env: { STALE: '1' },
    }, '-y\nagent', 'TOKEN=value')).toEqual({
      displayName: 'Claude',
      command: 'npx',
      args: ['-y', 'agent'],
      env: { TOKEN: 'value' },
    });
  });

  it('does not treat equivalent persisted values as a change', () => {
    const initial = buildAgentInput({
      displayName: 'Claude',
      command: 'npx',
      args: [],
      env: {},
    }, '-y\nagent', 'A=1\nB=2');
    const current = buildAgentInput({ ...initial, command: '  npx  ' }, '  -y   agent  ', 'B=2\nA=1');

    expect(hasManagedAgentInputChanged(initial, current)).toBe(false);
  });

  it('detects a persisted Agent configuration change', () => {
    const initial = buildAgentInput({
      displayName: 'Claude',
      command: 'npx',
      args: [],
      env: {},
    }, '-y\nagent', 'TOKEN=value');

    expect(hasManagedAgentInputChanged(initial, {
      ...initial,
      command: 'npx-test',
    })).toBe(true);
  });
});
