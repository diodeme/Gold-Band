/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', () => ({ getProfiles: vi.fn() }));

import { getProfiles } from '@/api';
import { resetProfileCatalogForTests, revalidateProfileCatalog, useProfileCatalog } from '@/lib/profile-catalog';
import type { ProfileListVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

function ProfileCatalogProbe({ enabled = true }: { enabled?: boolean }) {
  const catalog = useProfileCatalog(enabled);
  return (
    <div data-status={catalog.status} data-profile-ids={catalog.profiles.map((profile) => profile.id).join(',')}>
      {catalog.status === 'error' ? (
        <button type="button" data-error-code={catalog.error.code} onClick={catalog.retry}>retry</button>
      ) : null}
    </div>
  );
}

function list(...ids: string[]) {
  return { profiles: ids.map((id) => ({ id, name: id })) } as ProfileListVm;
}

const probes = () => [...host.querySelectorAll<HTMLElement>('[data-status]')]
  .map((probe) => `${probe.dataset.status}:${probe.dataset.profileIds}`);

beforeEach(() => {
  resetProfileCatalogForTests();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
  vi.clearAllMocks();
});

describe('profile catalog', () => {
  it('exposes a structured error and reloads through the same state contract', async () => {
    vi.mocked(getProfiles)
      .mockRejectedValueOnce({ code: 'profile.list-failed', params: {} })
      .mockResolvedValueOnce(list('developer'));

    await act(async () => root.render(<ProfileCatalogProbe />));

    expect(probes()).toEqual(['error:']);
    const retry = host.querySelector<HTMLButtonElement>('button');
    expect(retry?.dataset.errorCode).toBe('profile.list-failed');

    await act(async () => retry?.click());

    expect(probes()).toEqual(['ready:developer']);
  });

  it('shares one read between consumers and publishes a revalidation to every consumer', async () => {
    vi.mocked(getProfiles).mockResolvedValueOnce(list('old')).mockResolvedValueOnce(list('old', 'new'));

    await act(async () => root.render(<><ProfileCatalogProbe /><ProfileCatalogProbe /></>));
    expect(getProfiles).toHaveBeenCalledTimes(1);
    expect(probes()).toEqual(['ready:old', 'ready:old']);

    await act(async () => { await revalidateProfileCatalog(); });

    expect(probes()).toEqual(['ready:old,new', 'ready:old,new']);
  });

  it('coalesces revalidations during a read into one trailing read and never shows the superseded list', async () => {
    let finishFirst!: (value: ProfileListVm) => void;
    vi.mocked(getProfiles)
      .mockReturnValueOnce(new Promise((resolve) => { finishFirst = resolve; }))
      .mockResolvedValueOnce(list('current'));

    await act(async () => root.render(<ProfileCatalogProbe />));
    let latest: Promise<unknown> = Promise.resolve();
    await act(async () => {
      for (let i = 0; i < 5; i += 1) latest = revalidateProfileCatalog();
    });
    expect(getProfiles).toHaveBeenCalledTimes(1);

    await act(async () => {
      finishFirst(list('stale'));
      await latest;
    });

    expect(getProfiles).toHaveBeenCalledTimes(2);
    expect(probes()).toEqual(['ready:current']);
  });

  it('keeps the loaded list when a background revalidation fails', async () => {
    vi.mocked(getProfiles)
      .mockResolvedValueOnce(list('developer'))
      .mockRejectedValueOnce({ code: 'profile.list-failed', params: {} });

    await act(async () => root.render(<ProfileCatalogProbe />));
    await act(async () => { await revalidateProfileCatalog().catch(() => undefined); });

    expect(probes()).toEqual(['ready:developer']);
  });

  it('does not read when disabled', async () => {
    await act(async () => root.render(<ProfileCatalogProbe enabled={false} />));

    expect(getProfiles).not.toHaveBeenCalled();
    expect(probes()).toEqual(['loading:']);
  });
});
