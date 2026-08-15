import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import i18n from '../src/i18n';
import { agentAddMenuItemClassName, agentDeleteActionDisabled, agentEditorSheetPresentation, agentIdInputValue, buildAgentCardSummary, buildAgentInput, closeAgentDeleteDialogState, closeAgentEditorState, ExternalSessionSyncHeading, hasManagedAgentInputChanged, isAgentIdEditable, type AgentDeleteDialogState, type AgentEditorState } from '../src/pages/AgentManagementPage';
import type { AgentBindingUsageVm, ManagedAgentInput, ManagedAgentVm } from '../src/types';

const agentManagementSource = readFileSync(
  fileURLToPath(new URL('../src/pages/AgentManagementPage.tsx', import.meta.url)),
  'utf8',
);

function agentInput(overrides: Partial<ManagedAgentInput> = {}): ManagedAgentInput {
  return {
    displayName: 'Claude',
    icon: 'claude',
    command: 'npx',
    args: [],
    env: {},
    primaryAgentDir: '.claude',
    projectPrimaryAgentDir: null,
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
      projectPrimaryAgentDir: null,
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

  it('keeps cross-client session controls out of Agent cards and create/edit UI', () => {
    expect(agentManagementSource).not.toContain('external-session-sync-support');
    expect(agentManagementSource).not.toContain('id="external-session-sync"');
    expect(agentManagementSource).not.toContain('ExternalSessionSyncHeading');
    expect(agentManagementSource).not.toContain("t('agentManagement.externalSessionSyncSupport')");
    expect(agentManagementSource).not.toContain("t('agentManagement.externalSessionSync')");
    expect(buildAgentCardSummary(agentInputVm({
      externalSessionSyncSupported: true,
      externalSessionSyncEnabled: true,
    }), (key) => key).map((item) => item.key)).not.toContain('externalSessionSync');
  });

  it('preserves hidden external-session settings when saving other Agent fields', () => {
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
      projectPrimaryAgentDir: null,
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

  it('preserves split global and project primary directories as one directory policy', () => {
    const input = buildAgentInput(agentInput({
      primaryAgentDir: ' .pi/agent ',
      projectPrimaryAgentDir: ' .pi ',
    }), '', '', '.agents\n.pi\n.pi/agent');

    expect(input.primaryAgentDir).toBe('.pi/agent');
    expect(input.projectPrimaryAgentDir).toBe('.pi');
    expect(input.compatibleAgentDirs).toEqual(['.agents']);
    expect(hasManagedAgentInputChanged(input, {
      ...input,
      projectPrimaryAgentDir: null,
    })).toBe(true);
    expect(i18n.t('agentManagement.splitPrimaryAgentDirs', { lng: 'zh-CN' }))
      .toBe('拆分全局/项目主目录');
  });

  it('uses the default icon and allows an Agent without a Skills directory', () => {
    const input = buildAgentInput(agentInput({
      icon: '   ',
      primaryAgentDir: '   ',
    }), '', '', '');

    expect(input.icon).toBe('gold-band');
    expect(input.primaryAgentDir).toBe('');
    expect(input.compatibleAgentDirs).toEqual([]);
    expect(i18n.t('agentManagement.iconDescription', { lng: 'zh-CN' })).toContain('Gold Band Logo');
    expect(i18n.t('agentManagement.catalogIconDescription', { lng: 'zh-CN', agent: 'Claude' })).toContain('Claude 图标');
    expect(i18n.t('agentManagement.iconDescription', { lng: 'zh-CN' })).not.toContain('data URI');
    expect(i18n.t('agentManagement.useDefaultIcon', { lng: 'en' })).toBe('Restore Default Icon');
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

  it('presents the stable identifier as an Agent ID instead of a user-selectable type', () => {
    expect(i18n.t('agentManagement.agentId', { lng: 'zh-CN' })).toBe('Agent ID');
    expect(i18n.t('agentManagement.agentIdDescription', { lng: 'zh-CN' })).toContain('创建后不可修改');
    expect(i18n.exists('agentManagement.systemPromptSupport', { lng: 'zh-CN' })).toBe(false);
  });

  it('keeps custom Agent IDs editable independently from Catalog ID text', () => {
    expect(isAgentIdEditable({ mode: 'create', source: 'custom' })).toBe(true);
    expect(isAgentIdEditable({ mode: 'create', source: 'catalog' })).toBe(false);
    expect(isAgentIdEditable({ mode: 'edit', source: 'catalog' })).toBe(false);
    expect(isAgentIdEditable({ mode: 'edit', source: 'custom' })).toBe(false);
  });

  it('keeps the current Agent draft intact while the Sheet exit animation is running', () => {
    const form = agentInput({ icon: 'data:image/png;base64,custom-icon' });
    const state: AgentEditorState = {
      open: true,
      context: {
        mode: 'edit',
        source: 'catalog',
        defaultIconKey: 'claude',
        defaultIconLabel: 'Claude',
      },
      selectedType: 'claude-acp',
      form,
      argsText: '-y\n@agentclientprotocol/claude-agent-acp@latest',
      envText: 'TOKEN=value',
      compatibleAgentDirsText: '.agents',
      initialEditInput: form,
    };

    expect(closeAgentEditorState(state)).toEqual({ ...state, open: false });
  });

  it('keeps the side editor non-modal without a page-dimming overlay', () => {
    expect(agentEditorSheetPresentation).toEqual({
      modal: false,
      showOverlay: false,
    });
  });

  it('keeps the deleted Agent name available throughout the confirmation exit animation', () => {
    const target: ManagedAgentVm = {
      agentType: 'claude-acp',
      displayName: 'Claude',
      command: 'npx',
      args: [],
      env: [],
      iconKey: 'claude',
      primaryAgentDir: '.claude',
      projectPrimaryAgentDir: null,
      compatibleAgentDirs: ['.agents'],
      supportsSystemPrompt: true,
      externalSessionSyncSupported: false,
      externalSessionSyncEnabled: false,
      diagnostic: null,
    };
    const state: AgentDeleteDialogState = { open: true, target };

    expect(closeAgentDeleteDialogState(state)).toEqual({ open: false, target });
  });

  it('allows Agent deletion only after binding usage loads successfully', () => {
    const usage: AgentBindingUsageVm = {
      workflowTemplateCount: 1,
      taskCount: 2,
      scheduledTaskCount: 3,
    };

    expect(agentDeleteActionDisabled(true, null, null)).toBe(true);
    expect(agentDeleteActionDisabled(false, null, 'read failed')).toBe(true);
    expect(agentDeleteActionDisabled(false, null, null)).toBe(true);
    expect(agentDeleteActionDisabled(false, usage, null)).toBe(false);
    expect(i18n.t('agentManagement.deleteUsageRetry', { lng: 'zh-CN' })).toBe('重新统计');
    expect(i18n.t('agentManagement.deleteUsageRetry', { lng: 'en' })).toBe('Retry count');
  });

  it('does not rewrite an Agent ID while an IME composition is active', () => {
    expect(agentIdInputValue('入-my', true)).toBe('入-my');
    expect(agentIdInputValue('入-my', false)).toBe('-my');
    expect(agentIdInputValue('KIMI-For-Mine', false)).toBe('kimi-for-mine');
  });

  it('keeps the add-Agent command active state invisible until pointer hover', () => {
    expect(agentAddMenuItemClassName).toContain('data-[selected=true]:!bg-transparent');
    expect(agentAddMenuItemClassName).toContain('data-[selected=true]:!text-foreground');
    expect(agentAddMenuItemClassName).toContain('hover:bg-accent');
    expect(agentAddMenuItemClassName).toContain('hover:text-accent-foreground');
    expect(agentAddMenuItemClassName).toContain('data-[selected=true]:hover:!bg-accent');
    expect(agentAddMenuItemClassName).toContain('data-[selected=true]:hover:!text-accent-foreground');
    expect(agentAddMenuItemClassName).not.toContain('shadow-[');
  });
});

function agentInputVm(overrides: Partial<ManagedAgentVm> = {}): ManagedAgentVm {
  return {
    agentType: 'claude-acp',
    displayName: 'Claude',
    command: 'npx',
    args: [],
    env: [],
    iconKey: 'claude',
    primaryAgentDir: '.claude',
    projectPrimaryAgentDir: null,
    compatibleAgentDirs: [],
    supportsSystemPrompt: true,
    externalSessionSyncSupported: false,
    externalSessionSyncEnabled: false,
    diagnostic: null,
    ...overrides,
  };
}
