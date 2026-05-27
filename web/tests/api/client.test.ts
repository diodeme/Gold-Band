import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../../src/api/browser', () => ({
  browserApi: { runtime: 'browser' },
}));

vi.mock('../../src/api/desktop', () => ({
  desktopApi: { runtime: 'desktop' },
}));

vi.mock('../../src/api/shared', () => ({
  isTauriRuntime: vi.fn(),
}));

import { browserApi } from '../../src/api/browser';
import { getRuntimeApi } from '../../src/api/client';
import { desktopApi } from '../../src/api/desktop';
import { isTauriRuntime } from '../../src/api/shared';

describe('getRuntimeApi', () => {
  beforeEach(() => {
    vi.mocked(isTauriRuntime).mockReset();
  });

  it('returns the desktop implementation in Tauri runtime', () => {
    vi.mocked(isTauriRuntime).mockReturnValue(true);

    expect(getRuntimeApi()).toBe(desktopApi);
  });

  it('returns the browser implementation outside Tauri runtime', () => {
    vi.mocked(isTauriRuntime).mockReturnValue(false);

    expect(getRuntimeApi()).toBe(browserApi);
  });
});
