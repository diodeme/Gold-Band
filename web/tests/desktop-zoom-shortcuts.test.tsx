/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const applyDesktopZoom = vi.fn(async (percent: number) => percent);

vi.mock('@/api/client', () => ({
  getRuntimeApi: () => ({ applyDesktopZoom }),
}));

const installedDrivers: Array<{ adjust: (delta: number) => void; reset: () => void }> = [];
vi.mock('@/lib/desktop-page-zoom', () => ({
  setDesktopPageZoomDriver: (driver: { adjust: (delta: number) => void; reset: () => void } | null) => {
    if (driver) installedDrivers.push(driver);
  },
}));

import { useDesktopZoomShortcuts } from '@/hooks/use-desktop-zoom-shortcuts';
import { defaultPersonalizationPreference } from '@/theme';
import type { PersonalizationPreference, PreferencesVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function preferencesWith(zoom: number): PreferencesVm {
  return {
    appearance: { schemaVersion: 2, themeId: 'builtin.gold-band', colorScheme: 'system', visualQualityByTheme: {} },
    personalization: { ...(defaultPersonalizationPreference as PersonalizationPreference), interfaceZoomPercent: zoom },
    language: 'zh-cn',
    useLocalClaude: false,
    verboseLogging: false,
    browser: { schemaVersion: 1, searchEngine: 'baidu', openLocalLinksInBrowser: true, openWebLinksInBrowser: true },
    avatars: {} as never,
    wallpapers: {} as never,
  };
}

function mountHarness(zoom: number, onPersistZoom: (percent: number) => void, enabled = true) {
  const container = document.createElement('div');
  document.body.append(container);
  const root: Root = createRoot(container);
  const render = (nextZoom: number) => act(() => {
    root.render(<Harness zoom={nextZoom} onPersistZoom={onPersistZoom} />);
  });
  function Harness({ zoom, onPersistZoom: persistZoom }: { zoom: number; onPersistZoom: (percent: number) => void }) {
    useDesktopZoomShortcuts(preferencesWith(zoom), persistZoom, enabled);
    return null;
  }
  render(zoom);
  const driver = () => installedDrivers[installedDrivers.length - 1]!;
  return { root, render, driver };
}

beforeEach(() => {
  vi.useFakeTimers();
  installedDrivers.length = 0;
  applyDesktopZoom.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
  document.body.replaceChildren();
});

describe('useDesktopZoomShortcuts', () => {
  it('applies each step immediately and debounces persistence to the final value', async () => {
    const onPersistZoom = vi.fn();
    const { root, driver } = mountHarness(100, onPersistZoom);
    try {
      await act(async () => {
        driver().adjust(5);
        driver().adjust(5);
        driver().adjust(5);
      });
      expect(applyDesktopZoom).toHaveBeenCalledTimes(3);
      expect(applyDesktopZoom).toHaveBeenCalledWith(115);
      expect(onPersistZoom).not.toHaveBeenCalled();
      await act(async () => {
        vi.advanceTimersByTime(700);
      });
      // Persistence reports the single zoom percent; the owner persists it
      // through the single-field save so startup defaults can never round-trip
      // a full preference object.
      expect(onPersistZoom).toHaveBeenCalledTimes(1);
      expect(onPersistZoom).toHaveBeenCalledWith(115);
    } finally {
      act(() => root.unmount());
    }
  });

  it('cancels a pending step persist when a newer zoom is committed externally', async () => {
    const onPersistZoom = vi.fn();
    const { root, render, driver } = mountHarness(100, onPersistZoom);
    try {
      await act(async () => {
        driver().adjust(5); // schedules persist(105) in 600ms
      });
      // The settings slider commits 150% and the persisted preference syncs in.
      await act(async () => {
        render(150);
      });
      await act(async () => {
        vi.advanceTimersByTime(800);
      });
      expect(onPersistZoom).not.toHaveBeenCalled();

      // Later steps continue from the externally committed value.
      await act(async () => {
        driver().adjust(5);
        vi.advanceTimersByTime(700);
      });
      expect(onPersistZoom).toHaveBeenCalledTimes(1);
      expect(onPersistZoom).toHaveBeenCalledWith(155);
    } finally {
      act(() => root.unmount());
    }
  });

  it('re-applies an adopted zoom that a pending shortcut step previewed away from', async () => {
    const onPersist = vi.fn();
    // Startup defaults (100) are live before the preferences bootstrap lands.
    const { root, render, driver } = mountHarness(100, onPersist);
    try {
      await act(async () => {
        driver().adjust(5); // webview previews 105; persist(105) scheduled
      });
      // Bootstrap lands with the persisted 180 while that step is pending.
      await act(async () => {
        render(180);
      });
      await act(async () => {
        vi.advanceTimersByTime(800);
      });
      expect(onPersist).not.toHaveBeenCalled();
      // The adopted percent must win visually too, not only in the preference.
      expect(applyDesktopZoom).toHaveBeenCalledWith(180);

      // Later steps continue from the adopted value.
      await act(async () => {
        driver().adjust(5);
      });
      expect(applyDesktopZoom).toHaveBeenLastCalledWith(185);
    } finally {
      act(() => root.unmount());
    }
  });

  it('re-applies an adopted manual zoom percent without step snapping', async () => {
    const onPersistZoom = vi.fn();
    const { root, render, driver } = mountHarness(100, onPersistZoom);
    try {
      await act(async () => {
        driver().adjust(5); // pending step debounce exists
      });
      // A manually typed 153 commits externally (clamp-only, exact integer).
      await act(async () => {
        render(153);
      });
      // The re-apply must honor the exact percent: the manual input contract
      // keeps the typed integer, so stepping it to 155 would desync the
      // webview from the persisted preference.
      expect(applyDesktopZoom).toHaveBeenLastCalledWith(153);
    } finally {
      act(() => root.unmount());
    }
  });

  it('lets a newer pending step survive the echo of its own earlier commit', async () => {
    const onPersistZoom = vi.fn();
    const { root, render, driver } = mountHarness(100, onPersistZoom);
    try {
      await act(async () => {
        driver().adjust(5); // schedules persist(105)
      });
      await act(async () => {
        vi.advanceTimersByTime(700); // 105 commits
      });
      expect(onPersistZoom).toHaveBeenCalledWith(105);
      // A new step lands before the 105 commit's state echo arrives.
      await act(async () => {
        driver().adjust(5); // schedules persist(110)
      });
      // The 105 echo must NOT cancel the pending 110 step or snap the
      // webview back: it is this hook's own commit, not an external one.
      await act(async () => {
        render(105);
        vi.advanceTimersByTime(700);
      });
      expect(onPersistZoom).toHaveBeenCalledTimes(2);
      expect(onPersistZoom).toHaveBeenLastCalledWith(110);
      expect(applyDesktopZoom).toHaveBeenLastCalledWith(110);
    } finally {
      act(() => root.unmount());
    }
  });

  it('ignores steps until the persisted preferences have hydrated', async () => {
    const onPersistZoom = vi.fn();
    const { root, driver } = mountHarness(100, onPersistZoom, false);
    try {
      await act(async () => {
        driver().adjust(5);
        driver().reset();
        vi.advanceTimersByTime(1000);
      });
      // Startup defaults must not seed the step base: the persisted zoom
      // (applied by the backend before first paint) would be overwritten by
      // a value computed from a base the user never saw.
      expect(applyDesktopZoom).not.toHaveBeenCalled();
      expect(onPersistZoom).not.toHaveBeenCalled();
    } finally {
      act(() => root.unmount());
    }
  });

  it('does not touch the webview when preferences first mount', async () => {
    const onPersist = vi.fn();
    const { root } = mountHarness(100, onPersist);
    try {
      // The backend already applied the persisted zoom before the first paint;
      // adopting startup defaults must not clobber it with a 100% preview.
      expect(applyDesktopZoom).not.toHaveBeenCalled();
    } finally {
      act(() => root.unmount());
    }
  });

  it('resets to the default immediately and persists right away', async () => {
    const onPersistZoom = vi.fn();
    const { root, driver } = mountHarness(180, onPersistZoom);
    try {
      await act(async () => {
        driver().reset();
      });
      expect(applyDesktopZoom).toHaveBeenCalledWith(100);
      expect(onPersistZoom).toHaveBeenCalledTimes(1);
      expect(onPersistZoom).toHaveBeenCalledWith(100);
    } finally {
      act(() => root.unmount());
    }
  });
});
