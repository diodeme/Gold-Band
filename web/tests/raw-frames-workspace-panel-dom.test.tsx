/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { expect, it, vi } from 'vitest';
import type { AcpRawFramePageVm, AcpRawFrameQueryInput, ConversationRunVm } from '@/types';

const mocks = vi.hoisted(() => ({ read: vi.fn(), t: (key: string) => key }));
vi.mock('@/api', () => ({ getAcpRawFrames: mocks.read }));
vi.mock('react-i18next', async importOriginal => ({ ...await importOriginal<typeof import('react-i18next')>(), useTranslation: () => ({ t: mocks.t }) }));
vi.mock('@/components/GraphView', () => ({ GraphView: () => null }));
vi.mock('@/components/WorkflowEditor', () => ({ WorkflowEditor: () => null, parseWorkflowJson: vi.fn() }));
vi.mock('@/components/acp/ACPChatDialog', () => ({
  SystemPromptPanel: () => null,
  RawFrameViewer: ({ query, page, onQueryChange }: { query: AcpRawFrameQueryInput; page: AcpRawFramePageVm | null; onQueryChange: (query: AcpRawFrameQueryInput) => void }) => (
    <div><output>{JSON.stringify({ query, result: page?.search })}</output>
      <button onClick={() => onQueryChange({ page: 2, pageSize: 50, order: 'asc', search: 'new' })}>query</button>
    </div>
  ),
}));

import { ConversationRunWorkspaceResourcePanel } from '@/components/workspace/ConversationRunWorkspaceResourcePanel';
import { RightWorkspaceProvider, useRightWorkspace, type RawFramesWorkspaceResource } from '@/components/workspace/right-workspace-context';
globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const resource: RawFramesWorkspaceResource = { kind: 'raw-frames', key: 'raw', scopeKey: 'draft:project', title: 'Raw', attention: false,
  locator: { projectId: 'project', taskId: 'task', runId: 'run', roundId: 'round', nodeId: 'node', attemptId: 'attempt', branchId: 'root' } };
function result(query: AcpRawFrameQueryInput): AcpRawFramePageVm {
  return { ...query, page: query.page ?? 0, pageSize: query.pageSize ?? 100, order: query.order ?? 'desc', items: [], total: 200, hasPrevious: false, hasNext: false };
}

it.each([false, true])('restores submitted query after presentation remount (late old response: %s)', async late => {
  let finishOld!: (page: AcpRawFramePageVm) => void;
  mocks.read.mockReset().mockImplementation((_p, _t, _r, _round, _n, _a, query) => Promise.resolve(result(query)));
  if (late) mocks.read.mockImplementationOnce(() => new Promise(resolve => { finishOld = resolve; }));
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  let workspace!: ReturnType<typeof useRightWorkspace>;
  function Panel({ visible }: { visible: boolean }) {
    workspace = useRightWorkspace();
    const active = workspace.tabs[0] as RawFramesWorkspaceResource | undefined;
    return visible && active ? <ConversationRunWorkspaceResourcePanel resource={active} run={{} as ConversationRunVm} agentRegistry={null} /> : null;
  }
  const render = (visible: boolean) => root.render(<RightWorkspaceProvider scope={{ kind: 'draft', projectId: 'project', key: 'draft:project' }}><Panel visible={visible} /></RightWorkspaceProvider>);
  try {
    await act(async () => render(true));
    await act(async () => workspace.openResource(resource));
    await act(async () => container.querySelector('button')!.click());
    expect(container.querySelector('output')?.textContent).toContain('"result":"new"');
    if (late) await act(async () => finishOld(result({ search: 'old' })));
    expect(container.querySelector('output')?.textContent).toContain('"result":"new"');
    await act(async () => render(false));
    await act(async () => render(true));
    expect(mocks.read.mock.calls.at(-1)?.[6]).toMatchObject({ page: 2, pageSize: 50, order: 'asc', search: 'new' });
    expect(container.querySelector('output')?.textContent).toContain('"result":"new"');
  } finally { await act(async () => root.unmount()); container.remove(); }
});
