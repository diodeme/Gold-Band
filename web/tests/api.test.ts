import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../src/api/client', () => ({
  getRuntimeApi: vi.fn(),
}));

import { getRuntimeApi } from '../src/api/client';
import { deleteProfile } from '../src/api';

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
});
