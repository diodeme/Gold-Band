/** @vitest-environment jsdom */

import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';

import {
  RightWorkspaceProvider,
  createDraftConversationWorkspaceScope,
  useOptionalRightWorkspace,
  useOptionalRightWorkspaceCommands,
  type RightWorkspaceResource,
} from '@/components/workspace/right-workspace-context';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

describe('right workspace context boundaries', () => {
  let root: Root | null = null;
  let container: HTMLElement | null = null;

  afterEach(() => {
    act(() => root?.unmount());
    container?.remove();
    root = null;
    container = null;
  });

  it('does not rerender commands-only consumers when workspace state changes', async () => {
    container = document.body.appendChild(document.createElement('div'));
    root = createRoot(container);
    let commandsRenderCount = 0;
    let stateRenderCount = 0;

    function CommandsProbe() {
      const commands = useOptionalRightWorkspaceCommands();
      commandsRenderCount += 1;
      const resource = {
        kind: 'browser',
        key: 'browser:project-1',
        scopeKey: 'draft:project-1',
        title: 'Example',
        description: '',
        attention: false,
        projectId: 'project-1',
        url: 'about:blank',
      } as RightWorkspaceResource;
      return (
        <button type="button" onClick={() => void commands.openResource(resource)}>
          open
        </button>
      );
    }

    function StateProbe() {
      useOptionalRightWorkspace();
      stateRenderCount += 1;
      return null;
    }

    await act(async () => {
      root?.render(
        <RightWorkspaceProvider scope={createDraftConversationWorkspaceScope('project-1')}>
          <CommandsProbe />
          <StateProbe />
        </RightWorkspaceProvider>,
      );
    });
    const initialCommandsRenderCount = commandsRenderCount;
    const initialStateRenderCount = stateRenderCount;

    await act(async () => {
      container?.querySelector('button')?.click();
    });

    expect(commandsRenderCount).toBe(initialCommandsRenderCount);
    expect(stateRenderCount).toBeGreaterThan(initialStateRenderCount);
  });
});
