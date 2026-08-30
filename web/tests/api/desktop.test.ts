import { beforeEach, describe, expect, it, vi } from 'vitest';

const openerMocks = vi.hoisted(() => ({
  openPath: vi.fn(() => Promise.resolve()),
  openUrl: vi.fn(() => Promise.resolve()),
}));
const dialogMocks = vi.hoisted(() => ({
  save: vi.fn<() => Promise<string | null>>(() => Promise.resolve(null)),
}));

vi.mock('../../src/api/shared', () => ({
  invokeCommand: vi.fn(() => Promise.resolve({
    profiles: [],
    preferences: { wallpapers: { recentWallpapers: [] } },
  })),
  toRoundSelectionInput: vi.fn((selection) => selection),
}));
vi.mock('@tauri-apps/plugin-opener', () => openerMocks);
vi.mock('@tauri-apps/plugin-dialog', () => dialogMocks);

import { desktopApi } from '../../src/api/desktop';
import { invokeCommand } from '../../src/api/shared';

describe('desktopApi', () => {
  beforeEach(() => {
    vi.mocked(invokeCommand).mockClear();
    openerMocks.openUrl.mockClear();
    dialogMocks.save.mockReset();
    dialogMocks.save.mockResolvedValue(null);
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

  it('forwards the visible input and attachments with one runtime continue command', async () => {
    const input = {
      displayText: '请继续并补充测试',
      quotes: [{ id: 'quote-1', sourceMessageKey: 'answer-1', text: '原始回答' }],
    };

    await desktopApi.continueConversationRuntime(
      'project-1',
      'task-1',
      'run-1',
      'round-1',
      'node-1',
      'attempt-1',
      'outer-node-1',
      'outer-attempt-1',
      input,
      'prompt-1',
      ['C:/attachments/example.png'],
    );

    expect(invokeCommand).toHaveBeenCalledWith('continue_conversation_runtime', {
      projectId: 'project-1',
      taskId: 'task-1',
      runId: 'run-1',
      roundId: 'round-1',
      nodeId: 'node-1',
      attemptId: 'attempt-1',
      outerNodeId: 'outer-node-1',
      outerAttemptId: 'outer-attempt-1',
      input,
      promptId: 'prompt-1',
      attachmentPaths: ['C:/attachments/example.png'],
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

  it('forwards frontend fatal diagnostics through the bounded Tauri command contract', async () => {
    const input = {
      kind: 'react-uncaught' as const,
      message: 'render failed',
      stack: 'at ConversationPage',
      pathname: '/conversation/task-001',
    };

    await desktopApi.reportFrontendError(input);

    expect(invokeCommand).toHaveBeenCalledWith('report_frontend_error', { input });
  });

  it('copies a path-backed image through the native clipboard command', async () => {
    const input = {
      source: { kind: 'path' as const, path: 'D:/images/shot.png' },
      fileName: 'shot.png',
      mime: 'image/png',
    };

    await desktopApi.copyImageToClipboard(input);

    expect(invokeCommand).toHaveBeenCalledWith('copy_image_to_clipboard', {
      source: input.source,
    });
  });

  it('saves through the system dialog and treats cancellation as a normal result', async () => {
    const input = {
      source: { kind: 'bytes' as const, dataBase64: 'AQID' },
      fileName: 'pasted.png',
      mime: 'image/png',
    };
    dialogMocks.save.mockResolvedValueOnce('D:/exports/pasted.png');

    await expect(desktopApi.saveImageAs(input)).resolves.toBe(true);
    expect(dialogMocks.save).toHaveBeenCalledWith({
      defaultPath: 'pasted.png',
      filters: [{ name: 'Image', extensions: ['png'] }],
    });
    expect(invokeCommand).toHaveBeenCalledWith('save_image_as', {
      input: { source: input.source, destinationPath: 'D:/exports/pasted.png' },
    });

    vi.mocked(invokeCommand).mockClear();
    await expect(desktopApi.saveImageAs(input)).resolves.toBe(false);
    expect(invokeCommand).not.toHaveBeenCalled();
  });

  it('acknowledges the exact Direct terminal event through a scoped command input', async () => {
    await desktopApi.acknowledgeConversationTerminalResult('project-1', 'task-1', 'event-1');

    expect(invokeCommand).toHaveBeenCalledWith('acknowledge_conversation_terminal_result', {
      input: {
        projectId: 'project-1',
        taskId: 'task-1',
        eventId: 'event-1',
      },
    });
  });

  it('loads workspace options without requesting the full conversation sidebar', async () => {
    await desktopApi.getConversationWorkspaces();

    expect(invokeCommand).toHaveBeenCalledWith('get_conversation_workspaces');
  });

  it('loads the conversation sidebar through bounded progressive commands', async () => {
    await desktopApi.getConversationSidebarBootstrap();
    expect(invokeCommand).toHaveBeenCalledWith('get_conversation_sidebar_bootstrap');

    await desktopApi.getConversationTaskPage('project-1', 'task-24', 24);
    expect(invokeCommand).toHaveBeenCalledWith('get_conversation_task_page', {
      projectId: 'project-1',
      cursor: 'task-24',
      limit: 24,
    });

    await desktopApi.getConversationPinnedTaskPage('pin-24', 24);
    expect(invokeCommand).toHaveBeenCalledWith('get_conversation_pinned_task_page', {
      cursor: 'pin-24',
      limit: 24,
    });

    await desktopApi.getConversationRunSummaryPage('project-1', 'task-1', 'run-20', 20);
    expect(invokeCommand).toHaveBeenCalledWith('get_conversation_run_summary_page', {
      projectId: 'project-1',
      taskId: 'task-1',
      cursor: 'run-20',
      limit: 20,
    });
  });

  it('exposes branch picker reads and mutations through the desktop runtime contract', async () => {
    await desktopApi.getGitBranchPickerSnapshot('project-1', 'D:/repo');
    expect(invokeCommand).toHaveBeenCalledWith('get_git_branch_picker_snapshot', {
      projectId: 'project-1',
      workspacePath: 'D:/repo',
    });

    const input = { kind: 'switch' as const, name: 'feature/test', expectedRevision: 'revision-1' };
    await desktopApi.changeGitBranch('project-1', 'D:/repo', input);
    expect(invokeCommand).toHaveBeenCalledWith('change_git_branch', {
      projectId: 'project-1',
      workspacePath: 'D:/repo',
      input,
    });
  });

  it('forwards scheduled occurrence diagnostics commands', async () => {
    await desktopApi.listScheduledTaskOccurrences('project-1', 'scheduled-1', 'cursor-1', 'failed');
    expect(invokeCommand).toHaveBeenCalledWith('list_scheduled_task_occurrences', {
      projectId: 'project-1',
      scheduledTaskId: 'scheduled-1',
      cursor: 'cursor-1',
      status: 'failed',
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
