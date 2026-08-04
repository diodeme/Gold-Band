/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import {
  createDraftConversationWorkspaceScope,
  RightWorkspaceProvider,
  useRightWorkspace,
  useRightWorkspaceCommands,
  type FileWorkspaceResource,
  type RightWorkspaceCommands,
} from '@/components/workspace/right-workspace-context';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

function fileResource(index: number): FileWorkspaceResource {
  return {
    kind: 'file',
    key: `file:project-1:D:/repo/file-${index}.ts`,
    scopeKey: 'draft:project-1',
    projectId: 'project-1',
    title: `file-${index}.ts`,
    description: `file-${index}.ts`,
    attention: false,
    locator: {
      projectId: 'project-1',
      canonicalPath: `D:/repo/file-${index}.ts`,
      relativePath: `file-${index}.ts`,
      scope: 'workspace',
    },
    target: null,
    targetRevision: 1,
  };
}

afterEach(() => document.body.replaceChildren());

describe('right workspace stable command interface', () => {
  it('does not rerender command consumers when tabs, active tab, or width change', async () => {
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    let commandRenders = 0;
    let initialCommands: RightWorkspaceCommands | null = null;
    let currentCommands: RightWorkspaceCommands | null = null;
    let workspaceState: ReturnType<typeof useRightWorkspace> | null = null;

    function CommandConsumer() {
      const commands = useRightWorkspaceCommands();
      commandRenders += 1;
      initialCommands ??= commands;
      currentCommands = commands;
      return null;
    }
    function StateController() {
      workspaceState = useRightWorkspace();
      return <output data-tabs={workspaceState.tabs.length} data-width={workspaceState.width} />;
    }

    try {
      await act(async () => root.render(
        <RightWorkspaceProvider scope={createDraftConversationWorkspaceScope('project-1')}>
          <CommandConsumer />
          <StateController />
        </RightWorkspaceProvider>,
      ));
      for (let index = 0; index < 15; index += 1) {
        await act(async () => { await currentCommands!.openResource(fileResource(index)); });
      }
      await act(async () => { await workspaceState!.activateTab(fileResource(0).key); });
      await act(async () => workspaceState!.setWidth(720));

      expect(container.querySelector('output')?.dataset).toMatchObject({ tabs: '15', width: '720' });
      expect(workspaceState!.activeTabKey).toBe(fileResource(0).key);
      expect(commandRenders).toBe(1);
      expect(currentCommands).toBe(initialCommands);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
