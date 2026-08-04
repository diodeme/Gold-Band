/** @vitest-environment jsdom */

import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/api', async () => {
  const actual = await vi.importActual<typeof import('@/api')>('@/api');
  return { ...actual, getAcpActivityDetail: vi.fn(), getAcpToolDetail: vi.fn() };
});

import { getAcpActivityDetail, getAcpToolDetail } from '@/api';
import { ACPMessageList, buildAcpTimelineProjection } from '@/components/acp/ACPChatDialog';
import type { AgentTranscriptLocator } from '@/components/workspace/right-workspace-context';
import type { AcpActivityDetailVm, AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

const locator: AgentTranscriptLocator = {
  projectId: 'project-1',
  taskId: 'task-1',
  runId: 'run-1',
  roundId: 'round-1',
  nodeId: 'node-1',
  attemptId: 'attempt-1',
  branchId: 'agent-1',
};

function activitySummary(): AcpUiEventVm {
  return {
    id: 'activity-10',
    seq: 10,
    timestamp: '10Z',
    kind: 'activitySummary',
    sessionId: 'session-1',
    content: null,
    title: null,
    toolCallId: null,
    status: 'completed',
    startedSeq: 10,
    endedSeq: 109,
    raw: {
      goldBandActivity: {
        activityStartSeq: 10,
        activityEndSeq: 109,
        totalEventCount: 100,
        toolCallCount: 100,
        detailAvailable: true,
      },
    },
  };
}

afterEach(() => {
  vi.clearAllMocks();
  document.body.replaceChildren();
});

describe('ACP activity detail loading', () => {
  it('requests tool output only after the individual audit tool is expanded', async () => {
    vi.mocked(getAcpToolDetail).mockResolvedValue({ event: null });
    const tool: AcpUiEventVm = {
      id: 'tool-10',
      seq: 10,
      timestamp: '10Z',
      kind: 'toolCall',
      sessionId: 'session-1',
      content: null,
      title: 'Read file',
      toolCallId: 'call-10',
      status: 'completed',
      raw: {
        rawInput: { path: 'README.md' },
        _meta: { goldBandConversation: { toolName: 'Read', toolDetailAvailable: true } },
      },
    };
    const projection = buildAcpTimelineProjection([tool], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={projection.timeline} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      const activityTrigger = container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      await act(async () => {
        activityTrigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(getAcpToolDetail).not.toHaveBeenCalled();

      const triggers = container.querySelectorAll<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      await act(async () => {
        triggers[1]?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);
      expect(getAcpToolDetail).toHaveBeenCalledWith(
        'project-1',
        'task-1',
        'run-1',
        'round-1',
        'node-1',
        'attempt-1',
        { branchId: 'agent-1', eventId: 'tool-10', toolCallId: 'call-10' },
        undefined,
        undefined,
      );
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('issues exactly one initial detail request across rapid reopen attempts', async () => {
    let resolveDetail!: (value: AcpActivityDetailVm) => void;
    const detailPromise = new Promise<AcpActivityDetailVm>((resolve) => {
      resolveDetail = resolve;
    });
    vi.mocked(getAcpActivityDetail).mockReturnValue(detailPromise);
    const projection = buildAcpTimelineProjection([activitySummary()], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(React.createElement(ACPMessageList, {
          timeline: projection.timeline,
          sessionStatus: 'completed',
          sending: false,
          branchLocator: locator,
        }));
      });
      const trigger = container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      expect(trigger).not.toBeNull();

      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      expect(getAcpActivityDetail).toHaveBeenCalledTimes(1);
      expect(getAcpActivityDetail).toHaveBeenCalledWith(
        'project-1',
        'task-1',
        'run-1',
        'round-1',
        'node-1',
        'attempt-1',
        {
          branchId: 'agent-1',
          activityStartSeq: 10,
          activityEndSeq: 109,
          earlierCursor: null,
          limit: 40,
        },
        undefined,
        undefined,
      );

      await act(async () => {
        resolveDetail({ items: [], hasMoreEarlier: false, earlierCursor: null });
        await detailPromise;
      });
      expect(getAcpActivityDetail).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows a localized activity-detail failure and retries the same cursor', async () => {
    vi.mocked(getAcpActivityDetail)
      .mockRejectedValueOnce({ code: 'acp.activity-detail-query-failed', params: {} })
      .mockResolvedValueOnce({ items: [], hasMoreEarlier: false, earlierCursor: null });
    const projection = buildAcpTimelineProjection([activitySummary()], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={projection.timeline} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      const trigger = container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await Promise.resolve();
      });
      const retry = container.querySelector<HTMLButtonElement>('[data-acp-activity-detail-retry="true"]');
      expect(retry).not.toBeNull();
      await act(async () => {
        retry?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await Promise.resolve();
      });
      expect(getAcpActivityDetail).toHaveBeenCalledTimes(2);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('shows a tool-detail failure and allows retry without collapsing the tool', async () => {
    vi.mocked(getAcpToolDetail)
      .mockRejectedValueOnce({ code: 'acp.tool-detail-query-failed', params: {} })
      .mockResolvedValueOnce({ event: null });
    const tool: AcpUiEventVm = {
      id: 'tool-retry', seq: 20, timestamp: '20Z', kind: 'toolCall',
      sessionId: 'session-1', content: null, title: 'Read file', toolCallId: 'call-retry',
      status: 'completed',
      raw: {
        rawInput: { path: 'README.md' },
        _meta: { goldBandConversation: { toolName: 'Read', toolDetailAvailable: true } },
      },
    };
    const projection = buildAcpTimelineProjection([tool], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={projection.timeline} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      let triggers = container.querySelectorAll<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      await act(async () => {
        triggers[0]?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
      });
      triggers = container.querySelectorAll<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      await act(async () => {
        triggers[1]?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await Promise.resolve();
      });
      const retry = container.querySelector<HTMLButtonElement>('[data-acp-tool-detail-retry="true"]');
      expect(retry).not.toBeNull();
      await act(async () => {
        retry?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await Promise.resolve();
      });
      expect(getAcpToolDetail).toHaveBeenCalledTimes(2);
    } finally {
      await act(async () => root.unmount());
    }
  });
});
