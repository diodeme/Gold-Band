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
    expect(composer).toContain("composerDraft.draft.submission.kind === 'scheduled-task'");
    expect(composer).toContain('composerDraft.enterScheduledTask();');
    expect(composer).toContain('openScheduledConfig();');
    expect(home).toContain('initialScheduledMode={initialScheduledMode}');
    expect(app).toContain("onCreate={() => onSelectConversation({ kind: 'scheduled-task-create' })}");
    expect(app).toContain("initialScheduledMode={conversationPage.kind === 'scheduled-task-create'}");
  });

  it('keeps scheduled mode and configuration in the shared composer draft owner', () => {
    const composer = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)), 'utf8');
    const draft = readFileSync(fileURLToPath(new URL('../src/lib/conversation-composer-draft.ts', import.meta.url)), 'utf8');

    expect(composer).toContain('composerDraft.setScheduledTaskConfig(config)');
    expect(composer).toContain('composerDraft.exitScheduledTask()');
    expect(composer).not.toContain('useState<ScheduledTaskConfig | null>');
    expect(draft).toContain("{ kind: 'scheduled-task'; config: ScheduledTaskConfig | null }");
  });

  it('uses the ordinary composer workspace surface without offering worktree selection', () => {
    const composer = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)), 'utf8');
    expect(composer).toContain('showWorkLocation={!scheduledMode}');
    expect(composer).toContain('const scheduledConversationInput = () => ({');
    const scheduledInputSource = composer.slice(
      composer.indexOf('const scheduledConversationInput = () => ({'),
      composer.indexOf('const createScheduledTask = async () => {'),
    );
    expect(scheduledInputSource).not.toContain('workLocation');
    expect(composer).not.toMatch(/\{scheduledMode \? \(\s*<ConversationWorkspaceControl/u);
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

  it('keeps static scheduled-task icons readable in both themes', () => {
    const sources = [
      '../src/components/conversation/ConversationComposer.tsx',
      '../src/components/conversation/ConversationRunHeader.tsx',
      '../src/components/conversation/ConversationSidebar.tsx',
      '../src/components/conversation/ScheduledTaskDialog.tsx',
      '../src/pages/ScheduledTaskDetailPage.tsx',
    ].map((path) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8'));

    const scheduledIconWithPrimary = /<(?:AlarmClock|CalendarClock|ListChecks)[^>]*text-primary/;
    for (const source of sources) {
      expect(source).not.toMatch(scheduledIconWithPrimary);
    }
    expect(sources[1]).toContain('<TooltipContent>{t(\'scheduled.conversationMarker\')}</TooltipContent>');
    expect(sources[1]).not.toContain('title="定时任务会话"');
  });
});
