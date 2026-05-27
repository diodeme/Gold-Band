import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('./browser', () => ({
  browserApi: { runtime: 'browser' },
}));

vi.mock('./desktop', () => ({
  desktopApi: { runtime: 'desktop' },
}));

vi.mock('./shared', () => ({
  isTauriRuntime: vi.fn(),
}));

import { browserApi } from './browser';
import { getRuntimeApi } from './client';
import { desktopApi } from './desktop';
import { isTauriRuntime } from './shared';

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
