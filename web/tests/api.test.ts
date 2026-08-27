import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../src/api/client', () => ({
  getRuntimeApi: vi.fn(),
}));

import { getRuntimeApi } from '../src/api/client';
import {
  changeGitBranch,
  deleteProfile,
  getGitBranchPickerSnapshot,
  materializeConversationAttachments,
  pauseRun,
  removeRecentWorkspace,
  stopActiveSession,
} from '../src/api';

describe('api facade', () => {
  beforeEach(() => {
    vi.mocked(getRuntimeApi).mockReset();
  });

  it('passes the force flag through to the selected runtime API', async () => {
    const deleteProfileImpl = vi.fn().mockResolvedValue({ profiles: [] });
    vi.mocked(getRuntimeApi).mockReturnValue({ deleteProfile: deleteProfileImpl } as never);

    await deleteProfile('pf-123', true);

    expect(deleteProfileImpl).toHaveBeenCalledWith('pf-123', true);
  });

  it('defaults force to false when callers omit it', async () => {
    const deleteProfileImpl = vi.fn().mockResolvedValue({ profiles: [] });
    vi.mocked(getRuntimeApi).mockReturnValue({ deleteProfile: deleteProfileImpl } as never);

    await deleteProfile('pf-456');

    expect(deleteProfileImpl).toHaveBeenCalledWith('pf-456', false);
  });

  it('passes materialized attachment files through to the selected runtime API', async () => {
    const materializeImpl = vi.fn().mockResolvedValue([
      { path: 'C:/tmp/shot.png', name: 'shot.png', size: 4 },
    ]);
    vi.mocked(getRuntimeApi).mockReturnValue({ materializeConversationAttachments: materializeImpl } as never);
    const files = [{ name: 'shot.png', mime: 'image/png', dataBase64: 'AQIDBA==' }];

    const result = await materializeConversationAttachments(files);

    expect(materializeImpl).toHaveBeenCalledWith(files);
    expect(result).toEqual([{ path: 'C:/tmp/shot.png', name: 'shot.png', size: 4 }]);
  });

  it('passes recent workspace removal through to the selected runtime API', async () => {
    const removeRecentWorkspaceImpl = vi.fn().mockResolvedValue({ recentWorkspaces: [] });
    vi.mocked(getRuntimeApi).mockReturnValue({ removeRecentWorkspace: removeRecentWorkspaceImpl } as never);

    await removeRecentWorkspace('D:/Projects/code/ai/Gold-Band');

    expect(removeRecentWorkspaceImpl).toHaveBeenCalledWith('D:/Projects/code/ai/Gold-Band');
  });

  it('passes active session fallback and locator to the runtime API', async () => {
    const stopImpl = vi.fn().mockResolvedValue({ kind: 'session-cancelled', run: null, session: null });
    const fallback = { status: 'running' };
    vi.mocked(getRuntimeApi).mockReturnValue({ stopActiveSession: stopImpl } as never);

    await stopActiveSession('project-1', 'task-1', 'run-1', 'round-1', 'node-1', 'attempt-1', fallback as never, null, null);

    expect(stopImpl).toHaveBeenCalledWith('project-1', 'task-1', 'run-1', 'round-1', 'node-1', 'attempt-1', fallback, null, null);
  });

  it('routes ordinary run stop through pauseRun', async () => {
    const pauseImpl = vi.fn().mockResolvedValue({ id: 'run-1', status: 'paused' });
    vi.mocked(getRuntimeApi).mockReturnValue({ pauseRun: pauseImpl } as never);

    await pauseRun('task-1', 'run-1', 'project-1');

    expect(pauseImpl).toHaveBeenCalledWith('task-1', 'run-1', 'project-1');
  });

  it('routes branch picker reads and mutations through the selected runtime API', async () => {
    const snapshot = { revision: 'revision-1', currentBranch: 'main' };
    const getSnapshotImpl = vi.fn().mockResolvedValue(snapshot);
    const changeBranchImpl = vi.fn().mockResolvedValue({ ...snapshot, currentBranch: 'feature/test' });
    vi.mocked(getRuntimeApi).mockReturnValue({
      getGitBranchPickerSnapshot: getSnapshotImpl,
      changeGitBranch: changeBranchImpl,
    } as never);

    await getGitBranchPickerSnapshot('project-1', 'D:/repo');
    const input = { kind: 'switch' as const, name: 'feature/test', expectedRevision: 'revision-1' };
    await changeGitBranch('project-1', 'D:/repo', input);

    expect(getSnapshotImpl).toHaveBeenCalledWith('project-1', 'D:/repo');
    expect(changeBranchImpl).toHaveBeenCalledWith('project-1', 'D:/repo', input);
  });
});
