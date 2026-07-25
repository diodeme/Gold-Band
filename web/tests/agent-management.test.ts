import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import i18n from '../src/i18n';
import { buildAgentCardSummary, buildAgentInput, ExternalSessionSyncHeading, hasManagedAgentInputChanged } from '../src/pages/AgentManagementPage';
import type { ManagedAgentVm } from '../src/types';

describe('Agent management input mapping', () => {
  it('keeps the main card limited to operational summary fields', () => {
    const agent: ManagedAgentVm = {
      agentType: 'claude-acp',
      displayName: 'Claude',
      command: '  npx  ',
      args: ['-y', '@agentclientprotocol/claude-agent-acp@latest'],
      env: [],
      iconKey: 'claude',
      skillsDirName: '.claude',
      skillsDirOverride: '.claude-custom',
      externalSessionSyncEnabled: true,
      supported: true,
      diagnostic: null,
    };

    expect(buildAgentCardSummary(agent, (key) => key).map((item) => item.key)).toEqual([
      'command',
      'args',
      'env',
      'lastChecked',
    ]);
  });

  it('keeps every ManagedAgentConfig field editable when saving', () => {
    expect(buildAgentInput({
      displayName: 'Claude',
      command: 'npx',
      args: ['stale'],
      env: { STALE: '1' },
      skillsDirOverride: '  .claude-custom  ',
      externalSessionSyncEnabled: true,
    }, '-y\nagent', 'TOKEN=value')).toEqual({
      displayName: 'Claude',
      command: 'npx',
      args: ['-y', 'agent'],
      env: { TOKEN: 'value' },
      skillsDirOverride: '.claude-custom',
      externalSessionSyncEnabled: true,
    });
  });

  it('normalizes an empty Skill directory override to null', () => {
    const input = buildAgentInput({
      displayName: 'Codex',
      command: 'codex-acp',
      args: [],
      env: {},
      skillsDirOverride: '   ',
      externalSessionSyncEnabled: false,
    }, '', '');

    expect(input.skillsDirOverride).toBeNull();
    expect(input.externalSessionSyncEnabled).toBe(false);
  });

  it('does not treat equivalent argument whitespace or environment ordering as a change', () => {
    const initial = buildAgentInput({
      displayName: 'Claude',
      command: 'npx',
      args: [],
      env: {},
      skillsDirOverride: null,
      externalSessionSyncEnabled: false,
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
      skillsDirOverride: null,
      externalSessionSyncEnabled: false,
    }, '-y\nagent', 'TOKEN=value');

    expect(hasManagedAgentInputChanged(initial, {
      ...initial,
      externalSessionSyncEnabled: true,
    })).toBe(true);
  });

  it('marks external session sync as beta and explains the shared-context risk', () => {
    const markup = renderToStaticMarkup(createElement(ExternalSessionSyncHeading, {
      label: i18n.t('agentManagement.externalSessionSync', { lng: 'zh-CN' }),
      betaLabel: i18n.t('agentManagement.externalSessionSyncBeta', { lng: 'zh-CN' }),
      helpLabel: i18n.t('agentManagement.externalSessionSyncHelpLabel', { lng: 'zh-CN' }),
      helpText: i18n.t('agentManagement.externalSessionSyncHelp', { lng: 'zh-CN' }),
    }));

    expect(markup).toContain('data-slot="badge"');
    expect(markup).toContain('Beta');
    expect(markup).toContain('aria-label="了解外部会话同步"');
    expect(i18n.t('agentManagement.externalSessionSyncHelp', { lng: 'zh-CN' }))
      .toBe('同步同一个 Session 在其他客户端中发生过的对话。');
    expect(i18n.t('agentManagement.externalSessionSyncDescription', { lng: 'zh-CN' }))
      .toContain('否则可能造成历史顺序或上下文理解错误');
    expect(i18n.t('agentManagement.externalSessionSyncDescription', { lng: 'en' }))
      .toContain('otherwise history order or context may be misinterpreted');
  });
});
