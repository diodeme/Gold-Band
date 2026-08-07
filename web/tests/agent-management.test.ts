import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import i18n from '../src/i18n';
import { buildAgentCardSummary, buildAgentInput, ExternalSessionSyncHeading, hasManagedAgentInputChanged } from '../src/pages/AgentManagementPage';
import type { ManagedAgentInput, ManagedAgentVm } from '../src/types';

function agentInput(overrides: Partial<ManagedAgentInput> = {}): ManagedAgentInput {
  return {
    displayName: 'Claude',
    icon: 'claude',
    command: 'npx',
    args: [],
    env: {},
    primaryAgentDir: '.claude',
    compatibleAgentDirs: [],
    externalSessionSyncSupported: false,
    externalSessionSyncEnabled: false,
    ...overrides,
  };
}

describe('Agent management input mapping', () => {
  it('keeps the main card limited to operational summary fields', () => {
    const agent: ManagedAgentVm = {
      agentType: 'claude-acp',
      displayName: 'Claude',
      command: '  npx  ',
      args: ['-y', '@agentclientprotocol/claude-agent-acp@latest'],
      env: [],
      iconKey: 'claude',
      primaryAgentDir: '.claude-custom',
      compatibleAgentDirs: [],
      supportsSystemPrompt: true,
      externalSessionSyncSupported: true,
      externalSessionSyncEnabled: true,
      diagnostic: null,
    };

    expect(buildAgentCardSummary(agent, (key) => key).map((item) => item.key)).toEqual([
      'command',
      'args',
      'env',
      'lastChecked',
    ]);
  });

  it('keeps every user-editable ManagedAgentConfig field when saving', () => {
    expect(buildAgentInput(agentInput({
      primaryAgentDir: '  .claude-custom  ',
      externalSessionSyncSupported: true,
      externalSessionSyncEnabled: true,
    }), '-y\nagent', 'TOKEN=value', ' .agents\n.agents\n.claude-custom ')).toEqual({
      displayName: 'Claude',
      icon: 'claude',
      command: 'npx',
      args: ['-y', 'agent'],
      env: { TOKEN: 'value' },
      primaryAgentDir: '.claude-custom',
      compatibleAgentDirs: ['.agents'],
      externalSessionSyncSupported: true,
      externalSessionSyncEnabled: true,
    });
  });

  it('normalizes Agent directories and removes the primary directory from compatibility reads', () => {
    const input = buildAgentInput(agentInput({
      displayName: 'Codex',
      command: 'codex-acp',
      primaryAgentDir: ' .codex ',
    }), '', '', ' .agents, .codex, .agents ');

    expect(input.primaryAgentDir).toBe('.codex');
    expect(input.compatibleAgentDirs).toEqual(['.agents']);
    expect(input.externalSessionSyncEnabled).toBe(false);
  });

  it('uses the default icon and allows an Agent without a Skills directory', () => {
    const input = buildAgentInput(agentInput({
      icon: '   ',
      primaryAgentDir: '   ',
    }), '', '', '');

    expect(input.icon).toBe('agent');
    expect(input.primaryAgentDir).toBe('');
    expect(input.compatibleAgentDirs).toEqual([]);
  });

  it('disables external session sync when the Agent capability is unavailable', () => {
    const input = buildAgentInput(agentInput({
      externalSessionSyncSupported: false,
      externalSessionSyncEnabled: true,
    }), '', '');

    expect(input.externalSessionSyncSupported).toBe(false);
    expect(input.externalSessionSyncEnabled).toBe(false);
  });

  it('does not treat equivalent argument whitespace or environment ordering as a change', () => {
    const initial = buildAgentInput(agentInput(), '-y\nagent', 'A=1\nB=2');
    const current = buildAgentInput({ ...initial, command: '  npx  ' }, '  -y   agent  ', 'B=2\nA=1');

    expect(hasManagedAgentInputChanged(initial, current)).toBe(false);
  });

  it('detects a persisted Agent configuration change', () => {
    const initial = buildAgentInput(agentInput({ externalSessionSyncSupported: true }), '-y\nagent', 'TOKEN=value');

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

  it('presents the stable identifier as an Agent ID instead of a user-selectable type', () => {
    expect(i18n.t('agentManagement.agentId', { lng: 'zh-CN' })).toBe('Agent ID');
    expect(i18n.t('agentManagement.agentIdDescription', { lng: 'zh-CN' })).toContain('创建后不可修改');
    expect(i18n.exists('agentManagement.systemPromptSupport', { lng: 'zh-CN' })).toBe(false);
  });
});
