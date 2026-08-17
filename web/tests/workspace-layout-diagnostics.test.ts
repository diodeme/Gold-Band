/** @vitest-environment jsdom */

import { afterEach, describe, expect, it, vi } from 'vitest';
import type { WorkspaceLayoutDiagnosticRecord } from '@/lib/workspace-layout-diagnostics';

const WORKSPACE_LAYOUT_DEBUG_STORAGE_KEY = 'goldBand.debug.workspaceLayout';

function diagnostic(sequence: number): WorkspaceLayoutDiagnosticRecord {
  return {
    sequence,
    recordedAt: `2026-08-16T00:00:0${sequence}.000Z`,
    elapsedMs: sequence,
    stage: 'group-layout-change',
    details: { layout: { center: sequence } },
  };
}

async function loadDiagnostics(enabled: boolean) {
  vi.resetModules();
  window.localStorage.removeItem(WORKSPACE_LAYOUT_DEBUG_STORAGE_KEY);
  if (enabled) window.localStorage.setItem(WORKSPACE_LAYOUT_DEBUG_STORAGE_KEY, '1');
  return import('@/lib/workspace-layout-diagnostics');
}

afterEach(() => {
  window.localStorage.removeItem(WORKSPACE_LAYOUT_DEBUG_STORAGE_KEY);
  delete window.__goldBandWorkspaceLayoutDiagnostics;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.resetModules();
});

describe('workspace layout diagnostics', () => {
  it('keeps a bounded deep-copy-safe diagnostic window', async () => {
    const { createWorkspaceLayoutDiagnosticBuffer } = await loadDiagnostics(false);
    const buffer = createWorkspaceLayoutDiagnosticBuffer(2);
    buffer.append(diagnostic(1));
    buffer.append(diagnostic(2));
    buffer.append(diagnostic(3));

    const snapshot = buffer.snapshot();
    expect(snapshot.map((record) => record.sequence)).toEqual([2, 3]);
    (snapshot[0].details.layout as { center: number }).center = 99;
    expect(buffer.snapshot()[0].details.layout).toEqual({ center: 2 });
  });

  it('does not collect details or register the shortcut when the local switch is absent', async () => {
    const diagnostics = await loadDiagnostics(false);
    const createDetails = vi.fn(() => ({ page: 'conversation-home' }));
    const addEventListener = vi.spyOn(window, 'addEventListener');

    diagnostics.recordWorkspaceLayoutDiagnostic('presentation-committed', createDetails);
    const dispose = diagnostics.installWorkspaceLayoutDiagnosticShortcut();

    expect(diagnostics.isWorkspaceLayoutDiagnosticsEnabled()).toBe(false);
    expect(createDetails).not.toHaveBeenCalled();
    expect(addEventListener).not.toHaveBeenCalledWith(
      'keydown',
      expect.any(Function),
      expect.anything(),
    );
    expect(window.__goldBandWorkspaceLayoutDiagnostics).toBeUndefined();
    dispose();
  });

  it('reads the local switch once and requires a reload to change it', async () => {
    const diagnostics = await loadDiagnostics(false);

    window.localStorage.setItem(WORKSPACE_LAYOUT_DEBUG_STORAGE_KEY, '1');

    expect(diagnostics.isWorkspaceLayoutDiagnosticsEnabled()).toBe(false);
    expect((await loadDiagnostics(true)).isWorkspaceLayoutDiagnosticsEnabled()).toBe(true);
  });

  it('exports and copies structured records through the enabled keyboard shortcut', async () => {
    const diagnostics = await loadDiagnostics(true);
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
    vi.spyOn(console, 'info').mockImplementation(() => undefined);
    diagnostics.recordWorkspaceLayoutDiagnostic('presentation-committed', () => ({
      page: 'conversation-home',
      panels: { left: 304, center: 800, right: 768 },
    }));
    const dispose = diagnostics.installWorkspaceLayoutDiagnosticShortcut();
    try {
      window.dispatchEvent(new KeyboardEvent('keydown', {
        code: 'KeyL',
        ctrlKey: true,
        altKey: true,
        shiftKey: true,
      }));
      await vi.waitFor(() => expect(writeText).toHaveBeenCalledOnce());
      const exported = JSON.parse(String(writeText.mock.calls[0][0])) as {
        version: number;
        records: WorkspaceLayoutDiagnosticRecord[];
      };
      expect(exported.version).toBe(1);
      expect(exported.records.at(-1)).toMatchObject({
        stage: 'presentation-committed',
        details: { page: 'conversation-home' },
      });
      expect(diagnostics.exportWorkspaceLayoutDiagnostics()).toContain('presentation-committed');
      await expect(diagnostics.copyWorkspaceLayoutDiagnostics()).resolves.toContain('presentation-committed');
    } finally {
      dispose();
    }
  });

  it('falls back to the synchronous clipboard command when the async API is denied', async () => {
    const diagnostics = await loadDiagnostics(true);
    vi.stubGlobal('navigator', {
      ...navigator,
      clipboard: { writeText: vi.fn().mockRejectedValue(new DOMException('denied', 'NotAllowedError')) },
    });
    const execCommand = vi.fn().mockReturnValue(true);
    Object.defineProperty(document, 'execCommand', { configurable: true, value: execCommand });
    vi.spyOn(console, 'info').mockImplementation(() => undefined);
    diagnostics.recordWorkspaceLayoutDiagnostic('presentation-committed', () => ({ page: 'conversation-home' }));
    try {
      await expect(diagnostics.copyWorkspaceLayoutDiagnostics()).resolves.toContain('presentation-committed');
      expect(execCommand).toHaveBeenCalledWith('copy');
      expect(document.querySelector('textarea')).toBeNull();
    } finally {
      delete (document as Document & { execCommand?: (command: string) => boolean }).execCommand;
    }
  });
});
