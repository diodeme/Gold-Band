/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', () => ({ getProfiles: vi.fn() }));

import { getProfiles } from '@/api';
import { useWorkflowProfileCatalog } from '@/lib/workflow-profile-catalog';
import type { ProfileListVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

function ProfileCatalogProbe({ enabled = true }: { enabled?: boolean }) {
  const catalog = useWorkflowProfileCatalog(enabled);
  return (
    <div data-status={catalog.status} data-profile-ids={catalog.profiles.map((profile) => profile.id).join(',')}>
      {catalog.status === 'error' ? (
        <button type="button" data-error-code={catalog.error.code} onClick={catalog.retry}>retry</button>
      ) : null}
    </div>
  );
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
  vi.clearAllMocks();
});

describe('workflow profile catalog lifecycle', () => {
  it('exposes a structured error and reloads through the same state contract', async () => {
    vi.mocked(getProfiles)
      .mockRejectedValueOnce({ code: 'profile.list-failed', params: {} })
      .mockResolvedValueOnce({ profiles: [{ id: 'developer', name: 'Developer' }] } as ProfileListVm);

    await act(async () => root.render(<ProfileCatalogProbe />));

    expect(host.firstElementChild?.getAttribute('data-status')).toBe('error');
    const retry = host.querySelector<HTMLButtonElement>('button');
    expect(retry?.dataset.errorCode).toBe('profile.list-failed');

    await act(async () => retry?.click());

    expect(host.firstElementChild?.getAttribute('data-status')).toBe('ready');
    expect(host.firstElementChild?.getAttribute('data-profile-ids')).toBe('developer');
  });

  it('ignores a late response after the owning entry disables and reloads the catalog', async () => {
    let resolveFirst: (value: ProfileListVm) => void = () => undefined;
    const first = new Promise<ProfileListVm>((resolve) => { resolveFirst = resolve; });
    vi.mocked(getProfiles)
      .mockReturnValueOnce(first)
      .mockResolvedValueOnce({ profiles: [{ id: 'current', name: 'Current' }] } as ProfileListVm);

    await act(async () => root.render(<ProfileCatalogProbe />));
    await act(async () => root.render(<ProfileCatalogProbe enabled={false} />));
    await act(async () => root.render(<ProfileCatalogProbe enabled />));
    expect(host.firstElementChild?.getAttribute('data-profile-ids')).toBe('current');

    await act(async () => resolveFirst({ profiles: [{ id: 'stale', name: 'Stale' }] } as ProfileListVm));

    expect(host.firstElementChild?.getAttribute('data-status')).toBe('ready');
    expect(host.firstElementChild?.getAttribute('data-profile-ids')).toBe('current');
  });
});
