import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../src/api/client', () => ({
  getRuntimeApi: vi.fn(),
}));

import { getRuntimeApi } from '../src/api/client';
import { deleteProfile, materializeConversationAttachments, pauseRun, stopActiveSession } from '../src/api';

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
    const files = [{ name: 'shot.png', mime: 'image/png', size: 4, dataBase64: 'AQIDBA==' }];

    const result = await materializeConversationAttachments(files);

    expect(materializeImpl).toHaveBeenCalledWith(files);
    expect(result).toEqual([{ path: 'C:/tmp/shot.png', name: 'shot.png', size: 4 }]);
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

    await pauseRun('task-1', 'run-1');

    expect(pauseImpl).toHaveBeenCalledWith('task-1', 'run-1');
  });
});
