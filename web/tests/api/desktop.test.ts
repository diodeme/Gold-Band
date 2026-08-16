import { beforeEach, describe, expect, it, vi } from 'vitest';

const openerMocks = vi.hoisted(() => ({
  openPath: vi.fn(() => Promise.resolve()),
  openUrl: vi.fn(() => Promise.resolve()),
}));

vi.mock('../../src/api/shared', () => ({
  invokeCommand: vi.fn(() => Promise.resolve({ profiles: [] })),
  toRoundSelectionInput: vi.fn((selection) => selection),
}));
vi.mock('@tauri-apps/plugin-opener', () => openerMocks);

import { desktopApi } from '../../src/api/desktop';
import { invokeCommand } from '../../src/api/shared';

describe('desktopApi', () => {
  beforeEach(() => {
    vi.mocked(invokeCommand).mockClear();
    openerMocks.openUrl.mockClear();
  });

  it('opens Markdown web targets with the desktop URL opener', async () => {
    await desktopApi.openExternalUrl('https://github.com/diodeme/Gold-Band/stargazers');

    expect(openerMocks.openUrl).toHaveBeenCalledWith('https://github.com/diodeme/Gold-Band/stargazers');
  });

  it('forwards deleteProfile directly to the Tauri command path', async () => {
    await desktopApi.deleteProfile('pf-missing', true);

    expect(invokeCommand).toHaveBeenCalledWith('delete_profile', { id: 'pf-missing', force: true });
  });

  it('forwards profile creation without a scope selector', async () => {
    const input = { name: 'Custom role', summary: 'Reusable role', content: 'body' };

    await desktopApi.createProfile(input);

    expect(invokeCommand).toHaveBeenCalledWith('create_profile', { input });
  });

  it('forwards profile folder imports through the typed command contract', async () => {
    await desktopApi.importProfilesFromFolder('D:/roles', true);

    expect(invokeCommand).toHaveBeenCalledWith('import_profiles_from_folder', {
      input: { folderPath: 'D:/roles', dynamicTemplate: true },
    });
  });

  it('forwards recent workspace removal to the Tauri command path', async () => {
    vi.mocked(invokeCommand).mockResolvedValueOnce({
      preferences: {
        wallpapers: { recentWallpapers: [] },
      },
    });

    await desktopApi.removeRecentWorkspace('D:/Projects/code/ai/Gold-Band');

    expect(invokeCommand).toHaveBeenCalledWith('remove_recent_workspace', {
      workspace: 'D:/Projects/code/ai/Gold-Band',
    });
  });

  it('loads task authoring from the requested conversation workspace', async () => {
    await desktopApi.getWorkflow('task-1', 'project-1');

    expect(invokeCommand).toHaveBeenCalledWith('get_workflow', {
      projectId: 'project-1',
      taskId: 'task-1',
    });
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

  it('routes ordinary run stop to the Tauri pause command', async () => {
    await desktopApi.pauseRun('task-1', 'run-1', 'project-1');

    expect(invokeCommand).toHaveBeenCalledWith('pause_run', {
      taskId: 'task-1',
      runId: 'run-1',
      projectId: 'project-1',
    });
  });

  it('forwards conversation search to the desktop command contract', async () => {
    await desktopApi.searchConversationTasks('hello', 20);

    expect(invokeCommand).toHaveBeenCalledWith('search_conversation_tasks', {
      query: 'hello',
      limit: 20,
    });
  });

  it('loads workspace options without requesting the full conversation sidebar', async () => {
    await desktopApi.getConversationWorkspaces();

    expect(invokeCommand).toHaveBeenCalledWith('get_conversation_workspaces');
  });

  it('forwards scheduled occurrence diagnostics commands', async () => {
    await desktopApi.listScheduledTaskOccurrences('project-1', 'scheduled-1', 25);
    expect(invokeCommand).toHaveBeenCalledWith('list_scheduled_task_occurrences', {
      projectId: 'project-1',
      scheduledTaskId: 'scheduled-1',
      limit: 25,
    });

    await desktopApi.getScheduledTaskDiagnostics('project-1', 'scheduled-1');
    expect(invokeCommand).toHaveBeenCalledWith('get_scheduled_task_diagnostics', {
      projectId: 'project-1',
      scheduledTaskId: 'scheduled-1',
    });

    await desktopApi.runScheduledTaskNow('project-1', 'scheduled-1');
    expect(invokeCommand).toHaveBeenCalledWith('run_scheduled_task_now', {
      projectId: 'project-1',
      scheduledTaskId: 'scheduled-1',
    });
  });

  it('queries a captured turn change set with the complete branch locator', async () => {
    const locator = {
      projectId: 'project-1', taskId: 'task-1', runId: 'run-1', roundId: 'round-1',
      nodeId: 'node-1', attemptId: 'attempt-1', outerNodeId: 'dynamic-1',
      outerAttemptId: 'dynamic-attempt-1', branchId: 'agent-1',
    };

    await desktopApi.getTurnFileChangeSet(locator, 'change-set-1');

    expect(invokeCommand).toHaveBeenCalledWith('get_turn_file_change_set', {
      ...locator,
      changeSetId: 'change-set-1',
    });
  });

  it('queries a historical comparison by change-set and change identity', async () => {
    const locator = {
      projectId: 'project-1', taskId: 'task-1', runId: 'run-1', roundId: 'round-1',
      nodeId: 'node-1', attemptId: 'attempt-1', branchId: 'root',
    };

    await desktopApi.getFileComparison(locator, 'change-set-1', 'change-1');

    expect(invokeCommand).toHaveBeenCalledWith('get_file_comparison', {
      ...locator,
      changeSetId: 'change-set-1',
      changeId: 'change-1',
    });
  });
});
