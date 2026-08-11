import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { validateScheduledConversationInput } from '@/lib/scheduled-task-validation';
import type { AgentRegistryVm, ConversationCreateInput } from '@/types';
import { displayAppError } from '@/i18n';

describe('scheduled task composer entry', () => {
  it('disables only the scheduled primary action when the prompt is empty', () => {
    const source = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)), 'utf8');
    expect(source).toContain('const canCreateScheduledTask = canSubmit && Boolean(onCreateScheduledTask);');
    expect(source).toContain('disabled={!canCreateScheduledTask}');
    expect(source).toContain('if (!canCreateScheduledTask || !onCreateScheduledTask) return;');
    expect(source).toContain('disabled={busy || submittingAttachments || !onCreateScheduledTask}');
  });

  it('keeps send and schedule as compact split buttons with symmetric mode switching', () => {
    const source = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)), 'utf8');
    expect(source).toContain('DropdownMenu');
    expect(source).toContain('scheduledMode');
    expect(source).toContain('Settings2');
    expect(source).toContain("t('scheduled.composer.create')");
    expect(source).toContain('scheduledMode ? createScheduledTask() : handleSubmit()');
    expect(source.match(/w-6 rounded-none px-0 shadow-none/g)).toHaveLength(2);
    expect(source).not.toContain('border-primary-foreground/20');
    expect(source).not.toContain('variant="secondary" className="h-8 rounded-l-none');
    expect(source).toContain('onClick={exitScheduledMode}');
    expect(source).toContain('onSelect={exitScheduledMode}');
  });

  it('consumes the scheduled creation route as an initial composer mode', () => {
    const composer = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)), 'utf8');
    const home = readFileSync(fileURLToPath(new URL('../src/pages/ConversationHomePage.tsx', import.meta.url)), 'utf8');
    const app = readFileSync(fileURLToPath(new URL('../src/App.tsx', import.meta.url)), 'utf8');
    expect(composer).toContain('useState(initialScheduledMode)');
    expect(composer).toContain('openScheduledConfig();');
    expect(home).toContain('initialScheduledMode={initialScheduledMode}');
    expect(app).toContain("onCreate={() => onSelectConversation({ kind: 'scheduled-task-create' })}");
    expect(app).toContain("initialScheduledMode={conversationPage.kind === 'scheduled-task-create'}");
  });

  it('opens scheduled authoring as a right-workspace tab instead of a composer dialog', () => {
    const composer = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)), 'utf8');
    const editor = readFileSync(fileURLToPath(new URL('../src/components/conversation/ScheduledTaskDialog.tsx', import.meta.url)), 'utf8');
    expect(composer).toContain("registerResourceRenderer('scheduled-task-config'");
    expect(composer).toContain('scheduledTaskConfigWorkspaceResourceKey');
    expect(composer).toContain('presentation="workspace"');
    expect(composer).not.toContain('scheduledDialogOpen');
    expect(editor).toContain('data-scheduled-task-config-panel="true"');
  });

  it('submits local authoring fields without guessing a UTC offset', () => {
    const source = readFileSync(fileURLToPath(new URL('../src/components/conversation/ScheduledTaskDialog.tsx', import.meta.url)), 'utf8');
    expect(source).not.toContain('function zonedDateTimeToUtcIso');
    expect(source).toContain('getPreferredScheduledTimezone');
    expect(source).toContain('analyzeScheduledLocalTime');
    expect(source).toContain('disambiguation: atDisambiguation');
    expect(source).toContain('validationIssue');
    expect(source).toContain('disabled={!canSave || saving}');
  });

  it('rejects a scheduled Direct task before calling the desktop command when no Agent is selected', async () => {
    const input: ConversationCreateInput = { projectId: 'project-a', content: '检查项目', runMode: 'direct', directConfig: null };
    const issues = await validateScheduledConversationInput(input, {
      agentRegistry: null,
      workflowTemplates: null,
      profiles: [],
      loadProfiles: async () => [],
      t: (key: string) => key,
    });

    expect(issues).toEqual(['conversation.home.selectAgent']);
  });

  it('turns backend validation codes into actionable messages', () => {
    const message = displayAppError((key, options) => String(options?.defaultValue ?? key), {
      code: 'conversation.validation-failed',
      params: { codes: ['direct.agent.required', 'workflow.not-found'] },
    });

    expect(message).toBe('conversation.validation.direct.agent.required\nconversation.validation.workflow.not-found');
  });
  it('uses an AlarmClock marker for scheduled conversation records', () => {
    const header = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationRunHeader.tsx', import.meta.url)), 'utf8');
    const sidebar = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationSidebar.tsx', import.meta.url)), 'utf8');
    expect(header).toContain('run.scheduledTaskId');
    expect(sidebar).toContain('task.scheduledTaskId');
  });
});
