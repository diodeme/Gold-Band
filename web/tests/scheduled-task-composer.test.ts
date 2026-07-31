import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { validateScheduledConversationInput } from '@/lib/scheduled-task-validation';
import type { AgentRegistryVm, ConversationCreateInput } from '@/types';
import { displayAppError } from '@/i18n';

describe('scheduled task composer entry', () => {
  it('does not disable the schedule arrow when the prompt is empty', () => {
    const source = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)), 'utf8');
    expect(source).not.toContain('disabled={!canSubmit || !onCreateScheduledTask}');
    expect(source).toContain('disabled={busy || submittingAttachments || !onCreateScheduledTask}');
    expect(source).not.toContain('if (!canSubmit || !onCreateScheduledTask) return;');
  });

  it('keeps the schedule entry as a menu-driven action with a gear configuration trigger', () => {
    const source = readFileSync(fileURLToPath(new URL('../src/components/conversation/ConversationComposer.tsx', import.meta.url)), 'utf8');
    expect(source).toContain('DropdownMenu');
    expect(source).toContain('scheduledMode');
    expect(source).toContain('Settings2');
    expect(source).toContain('创建定时任务');
    expect(source).toContain('scheduledMode ? createScheduledTask() : handleSubmit()');
  });

  it('passes the selected timezone through one-time and interval schedule payloads', () => {
    const source = readFileSync(fileURLToPath(new URL('../src/components/conversation/ScheduledTaskDialog.tsx', import.meta.url)), 'utf8');
    expect(source).toContain("kind: 'At', at: zonedDateTimeToUtcIso(atDate, atTime, timezone), timezone");
    expect(source).toContain("kind: 'Every', every: { value: Math.max(1, Number(everyValue) || 1), unit: everyUnit }, anchorAt: new Date().toISOString(), timezone");
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
