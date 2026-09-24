/** @vitest-environment jsdom */

import { act, useEffect, useRef } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  WorkspaceFileReferenceBridgeProvider,
  useWorkspaceFileReferenceBridgeState,
  type AddWorkspaceFileRefResult,
} from '@/components/workspace/workspace-file-reference-bridge';
import type { ComposerWorkspaceFileRef } from '@/lib/composer-context';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function Harness({
  command,
  onResult,
}: {
  command: Parameters<ReturnType<typeof useWorkspaceFileReferenceBridgeState>['bridge']['register']>[0];
  onResult: (result: AddWorkspaceFileRefResult) => void;
}) {
  const bridgeState = useWorkspaceFileReferenceBridgeState();
  const commandRef = useRef(command);
  commandRef.current = command;
  const reference: ComposerWorkspaceFileRef = {
    id: 'project-1:src/a.ts',
    projectId: 'project-1',
    relativePath: 'src/a.ts',
    name: 'a.ts',
    byteLength: 1,
    mimeType: 'text/plain',
  };
  useEffect(
    () => bridgeState.bridge.register(
      command
        ? (reference, options) => commandRef.current(reference, options)
        : null,
    ),
    [bridgeState.bridge],
  );
  return (
    <WorkspaceFileReferenceBridgeProvider
      bridge={bridgeState.bridge}
      commands={bridgeState.commands}
    >
      <button
        type="button"
        onClick={() => onResult(
          bridgeState.commands.addWorkspaceFileRef(reference, { isDocked: true }),
        )}
      />
    </WorkspaceFileReferenceBridgeProvider>
  );
}

describe('workspace file reference bridge', () => {
  let root: Root | null = null;
  let container: HTMLElement | null = null;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    root = null;
    container = null;
  });

  it('returns an unavailable result before a composer registers', async () => {
    container = document.body.appendChild(document.createElement('div'));
    root = createRoot(container);
    const result = vi.fn();

    await act(async () => {
      root?.render(<Harness command={null} onResult={result} />);
    });
    await act(async () => {
      container?.querySelector('button')?.click();
    });

    expect(result).toHaveBeenCalledWith({ kind: 'unavailable' });
  });

  it('passes the dock presentation through to the registered composer command', async () => {
    container = document.body.appendChild(document.createElement('div'));
    root = createRoot(container);
    const command = vi.fn(() => ({ kind: 'added' }));
    const result = vi.fn();

    await act(async () => {
      root?.render(
        <Harness
          command={(reference, options) => {
            expect(options).toEqual({ isDocked: true });
            return command(reference, options);
          }}
          onResult={result}
        />,
      );
    });
    await act(async () => {
      container?.querySelector('button')?.click();
    });

    expect(command).toHaveBeenCalledWith(
      expect.objectContaining({ projectId: 'project-1' }),
      { isDocked: true },
    );
    expect(result).toHaveBeenCalledWith({ kind: 'added' });
  });
});
