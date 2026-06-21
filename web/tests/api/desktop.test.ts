import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../src/api/shared', () => ({
  invokeCommand: vi.fn(() => Promise.resolve({ profiles: [] })),
  toRoundSelectionInput: vi.fn((selection) => selection),
}));

import { desktopApi } from '../../src/api/desktop';
import { invokeCommand } from '../../src/api/shared';

describe('desktopApi', () => {
  beforeEach(() => {
    vi.mocked(invokeCommand).mockClear();
  });

  it('forwards deleteProfile directly to the Tauri command path', async () => {
    await desktopApi.deleteProfile('pf-missing', true);

    expect(invokeCommand).toHaveBeenCalledWith('delete_profile', { id: 'pf-missing', force: true });
  });

  it('normalizes updater override URL before invoking Tauri', async () => {
    await desktopApi.saveUpdaterSettings('  https://example.com/feed.json  ');

    expect(invokeCommand).toHaveBeenCalledWith('save_updater_settings', {
      overrideUrl: 'https://example.com/feed.json',
    });
  });

  it('passes active session locator to Tauri stop command', async () => {
    await desktopApi.stopActiveSession('project-1', 'task-1', 'run-1', 'round-1', 'node-1', 'attempt-1', null, null, null);

    expect(invokeCommand).toHaveBeenCalledWith('stop_active_session', {
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
      roundId: 'round-1',
      nodeId: 'node-1',
      attemptId: 'attempt-1',
      outerNodeId: null,
      outerAttemptId: null,
    });
  });
});
