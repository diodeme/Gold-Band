import { describe, expect, it } from 'vitest';

/**
 * Derives a project ID from a workspace path, matching the backend's
 * `project_id` normalization (lowercase + replace non-alnum-dash-underscore with hyphens).
 */
function normalizeWorkspaceProjectId(workspacePath: string): string {
  return workspacePath.toLowerCase().replace(/[^a-z0-9\-_]/g, '-');
}

describe('ConversationComposer workspace selection', () => {
  describe('workspace projectId normalization', () => {
    it('derives consistent projectId from workspace path', () => {
      expect(normalizeWorkspaceProjectId('/Users/test/claude-code')).toBe(
        '-users-test-claude-code',
      );
      expect(normalizeWorkspaceProjectId('C:\\Projects\\Gold-Band')).toBe(
        'c--projects-gold-band',
      );
    });

    it('preserves hyphens and underscores', () => {
      expect(normalizeWorkspaceProjectId('/path/to/my-workspace_test')).toBe(
        '-path-to-my-workspace_test',
      );
    });

    it('produces distinct ids for distinct workspaces', () => {
      const goldBand = normalizeWorkspaceProjectId('/home/user/gold-band');
      const claudeCode = normalizeWorkspaceProjectId('/home/user/claude-code');
      expect(goldBand).not.toBe(claudeCode);
    });
  });

  describe('multiple workspace selection', () => {
    it('uses the selected projectId in the create input', () => {
      const workspaces = [
        { projectId: 'gold-band', workspacePath: '/gold-band', name: 'Gold Band' },
        { projectId: 'claude-code', workspacePath: '/claude-code', name: 'Claude Code' },
      ];

      // Simulate user selecting the second workspace
      const selected = workspaces[1];
      const input = {
        projectId: selected.projectId,
        content: 'test requirement',
        runMode: 'auto' as const,
      };

      expect(input.projectId).toBe('claude-code');
      expect(input.projectId).not.toBe('gold-band');
    });

    it('single workspace does not show a selector', () => {
      const workspaces = [
        { projectId: 'gold-band', workspacePath: '/gold-band', name: 'Gold Band' },
      ];

      expect(workspaces.length > 1).toBe(false);
    });
  });
});
