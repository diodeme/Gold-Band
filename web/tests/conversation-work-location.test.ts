import { describe, expect, it } from 'vitest';

import {
  CONVERSATION_WORK_LOCATION_PREFERENCE_SCHEMA_VERSION,
  conversationWorkLocationForProject,
  parseConversationWorkLocationPreference,
  setConversationWorkLocationForProject,
} from '@/lib/conversation-work-location';

describe('conversation work location preference', () => {
  it('defaults invalid and unversioned values to the main workspace', () => {
    expect(conversationWorkLocationForProject(parseConversationWorkLocationPreference(null), 'project-1'))
      .toBe('main');
    expect(conversationWorkLocationForProject(parseConversationWorkLocationPreference({ byProjectId: { 'project-1': 'worktree' } }), 'project-1'))
      .toBe('main');
    expect(conversationWorkLocationForProject(parseConversationWorkLocationPreference({ schemaVersion: 2, byProjectId: { 'project-1': 'worktree' } }), 'project-1'))
      .toBe('main');
  });

  it('persists valid selections independently for each project', () => {
    const first = setConversationWorkLocationForProject(
      parseConversationWorkLocationPreference(null),
      'project-1',
      'worktree',
    );
    const second = setConversationWorkLocationForProject(first, 'project-2', 'main');

    expect(second).toEqual({
      schemaVersion: CONVERSATION_WORK_LOCATION_PREFERENCE_SCHEMA_VERSION,
      byProjectId: { 'project-1': 'worktree', 'project-2': 'main' },
    });
    expect(conversationWorkLocationForProject(second, 'project-1')).toBe('worktree');
    expect(conversationWorkLocationForProject(second, 'project-2')).toBe('main');
  });

  it('drops malformed project entries without discarding valid entries', () => {
    const parsed = parseConversationWorkLocationPreference({
      schemaVersion: 1,
      byProjectId: { valid: 'worktree', broken: 'remote', '': 'main' },
    });
    expect(parsed.byProjectId).toEqual({ valid: 'worktree' });
  });
});
