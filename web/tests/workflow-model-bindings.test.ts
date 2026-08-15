import { describe, expect, it } from 'vitest';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

import { WorkflowEditor } from '@/components/WorkflowEditor';
import { normalizeWorkflowModelBindings } from '@/lib/workflow-model-bindings';
import type { WorkflowDsl, WorkflowModelBindings } from '@/types';

describe('workflow model bindings boundary', () => {
  it('normalizes an omitted bindings collection to an empty array', () => {
    const legacyPayload = {
      definitionRevision: '',
      bindingRevision: 0,
    } as WorkflowModelBindings;

    expect(normalizeWorkflowModelBindings(legacyPayload)).toEqual({
      definitionRevision: '',
      bindingRevision: 0,
      bindings: [],
    });
  });

  it('preserves a complete binding snapshot by identity', () => {
    const snapshot: WorkflowModelBindings = {
      definitionRevision: 'revision',
      bindingRevision: 2,
      bindings: [{ executionSlotId: 'slot-a', agentId: 'agent-a' }],
    };

    expect(normalizeWorkflowModelBindings(snapshot)).toBe(snapshot);
  });

  it('renders the workflow editor when a legacy payload omits bindings', () => {
    const workflow: WorkflowDsl = {
      version: '0.1',
      id: 'empty-bindings',
      entry: '',
      control: {},
      nodes: [],
      edges: [],
    };
    const legacyPayload = {
      definitionRevision: '',
      bindingRevision: 0,
    } as WorkflowModelBindings;

    expect(() => renderToStaticMarkup(React.createElement(WorkflowEditor, {
      value: workflow,
      modelBindings: legacyPayload,
      agentRegistry: null,
      onSave: () => undefined,
    }))).not.toThrow();
  });
});
