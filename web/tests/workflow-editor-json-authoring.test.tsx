/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('react-i18next', async (importOriginal) => ({
  ...await importOriginal<typeof import('react-i18next')>(),
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock('@xyflow/react', async () => {
  const { Fragment } = await import('react');
  const PassThrough = ({ children }: { children?: React.ReactNode }) => <Fragment>{children}</Fragment>;
  return {
    Background: () => null,
    BaseEdge: () => null,
    EdgeLabelRenderer: PassThrough,
    Handle: () => null,
    MarkerType: { ArrowClosed: 'arrowclosed' },
    NodeToolbar: PassThrough,
    Panel: PassThrough,
    Position: { Bottom: 'bottom', Left: 'left', Right: 'right', Top: 'top' },
    ReactFlow: PassThrough,
    getSmoothStepPath: () => ['', 0, 0],
    useUpdateNodeInternals: () => () => undefined,
  };
});

vi.mock('@/components/ui/resizable', async () => {
  const { Fragment } = await import('react');
  const PassThrough = ({ children }: { children?: React.ReactNode }) => <Fragment>{children}</Fragment>;
  return {
    ResizableHandle: () => null,
    ResizablePanel: PassThrough,
    ResizablePanelGroup: PassThrough,
  };
});

import { WorkflowEditor, type WorkflowEditorSessionDraft } from '@/components/WorkflowEditor';
import { TooltipProvider } from '@/components/ui/tooltip';
import { readyWorkflowProfileCatalog } from '@/lib/workflow-profile-catalog';
import type { AgentRegistryVm, WorkflowDsl, WorkflowModelBindings } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const workflow: WorkflowDsl = {
  version: '0.1',
  id: 'json-authoring-workflow',
  entry: 'accept',
  control: {},
  nodes: [{
    type: 'worker',
    id: 'accept',
    executionSlotId: 'slot-accept',
    profile: 'pf-builtin-accept',
  }],
  edges: [{ from: 'accept', to: '$end', on: 'success' }],
};

const modelBindings: WorkflowModelBindings = {
  definitionRevision: 'definition-1',
  bindingRevision: 1,
  bindings: [{ executionSlotId: 'slot-accept', agentId: 'claude-acp' }],
};

const agentRegistry = {
  agents: [{
    agentType: 'claude-acp',
    displayName: 'Claude',
    diagnostic: { available: true },
    supportedModels: [],
    supportedModes: [],
    configOptions: [],
  }],
  catalog: [],
} as AgentRegistryVm;

let host: HTMLDivElement;
let root: Root;
let originalClientWidth: PropertyDescriptor | undefined;

beforeEach(() => {
  originalClientWidth = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth');
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', { configurable: true, get: () => 1024 });
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    unobserve() {}
    disconnect() {}
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  document.body.replaceChildren();
  if (originalClientWidth) Object.defineProperty(HTMLElement.prototype, 'clientWidth', originalClientWidth);
  else delete (HTMLElement.prototype as HTMLElement & { clientWidth?: number }).clientWidth;
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

describe('WorkflowEditor JSON authoring', () => {
  it('preserves a worker slot when pasted JSON is projected to the canvas', async () => {
    const onSessionDraftChange = vi.fn<(draft: WorkflowEditorSessionDraft) => void>();
    const initialSessionDraft: WorkflowEditorSessionDraft = {
      workflow,
      modelBindings,
      tab: 'json',
      jsonDraft: JSON.stringify(workflow, null, 2),
    };

    await act(async () => root.render(
      <TooltipProvider>
        <WorkflowEditor
          value={workflow}
          modelBindings={modelBindings}
          agentRegistry={agentRegistry}
          profileCatalog={readyWorkflowProfileCatalog([{ id: 'pf-builtin-accept', name: 'Accept' }])}
          initialSessionDraft={initialSessionDraft}
          onSessionDraftChange={onSessionDraftChange}
          onSave={() => undefined}
          showSaveAction={false}
        />
      </TooltipProvider>,
    ));

    const pastedWorkflow: WorkflowDsl = {
      ...workflow,
      nodes: workflow.nodes.map((node) => node.type === 'worker'
        ? { ...node, executionSlotId: undefined }
        : node),
    };
    const textarea = host.querySelector<HTMLTextAreaElement>('textarea');
    expect(textarea).not.toBeNull();

    await act(async () => {
      setNativeTextareaValue(textarea!, JSON.stringify(pastedWorkflow, null, 2));
      textarea!.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertFromPaste' }));
    });

    const canvasTab = Array.from(host.querySelectorAll<HTMLButtonElement>('[role="tab"]'))
      .find((button) => button.textContent === 'workflowEditor.canvas');
    expect(canvasTab).toBeDefined();
    await act(async () => canvasTab!.click());
    await act(async () => new Promise((resolve) => window.setTimeout(resolve, 380)));

    const latestDraft = onSessionDraftChange.mock.calls.at(-1)?.[0];
    const accept = latestDraft?.workflow.nodes.find((node) => node.id === 'accept');
    expect(accept).toMatchObject({ type: 'worker', executionSlotId: 'slot-accept' });
    expect(host.textContent).not.toContain('workflowEditor.validationSlotRequired');
  });
});

function setNativeTextareaValue(textarea: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')?.set;
  setter?.call(textarea, value);
}
