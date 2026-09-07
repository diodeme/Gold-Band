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

function activitySummary(sessionId = 'session-1', totalEventCount = 100): AcpUiEventVm {
  const activityEndSeq = 9 + totalEventCount;
  return {
    id: 'activity-10',
    seq: 10,
    timestamp: '10Z',
    kind: 'activitySummary',
    sessionId,
    content: null,
    title: null,
    toolCallId: null,
    status: 'completed',
    startedSeq: 10,
    endedSeq: activityEndSeq,
    raw: {
      goldBandActivity: {
        activityStartSeq: 10,
        activityEndSeq,
        totalEventCount,
        toolCallCount: totalEventCount,
        detailAvailable: true,
      },
    },
  };
}

function activityToolEvent(seq: number, sessionId = 'session-1'): AcpUiEventVm {
  return {
    id: `tool-${seq}`,
    seq,
    timestamp: `${seq}Z`,
    kind: 'toolCall',
    sessionId,
    content: null,
    title: `Tool ${seq}`,
    toolCallId: `call-${seq}`,
    status: 'completed',
    startedSeq: seq,
    endedSeq: seq,
    raw: {
      output: `output-${seq}`,
      _meta: { goldBandConversation: { toolDetailAvailable: false } },
    },
  };
}

function activityDetailPage(from: number, to: number, earlierCursor: string | null): AcpActivityDetailVm {
  return {
    items: Array.from({ length: to - from + 1 }, (_, index) => activityToolEvent(from + index)),
    hasMoreEarlier: earlierCursor !== null,
    earlierCursor,
  };
}

async function clickButton(button: HTMLButtonElement | null | undefined) {
  expect(button).not.toBeNull();
  await act(async () => {
    button?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await Promise.resolve();
  });
}

function findButtonByText(container: HTMLElement, text: string) {
  return Array.from(container.querySelectorAll<HTMLButtonElement>('button'))
    .find((button) => button.textContent?.includes(text)) ?? null;
}

afterEach(() => {
  vi.clearAllMocks();
  document.body.replaceChildren();
});

describe('ACP activity detail loading', () => {
  it('loads the authoritative detail when a compact summary is mixed with only a partial live tail', async () => {
    const partialThought: AcpUiEventVm = {
      id: 'thought-partial',
      seq: 20,
      timestamp: '20Z',
      kind: 'thoughtDelta',
      sessionId: 'session-1',
      content: 'partial live thought',
      title: null,
      toolCallId: null,
      status: 'completed',
      startedSeq: 10,
      endedSeq: 20,
      raw: {},
    };
    const summary = activitySummary();
    summary.raw = {
      goldBandActivity: {
        activityStartSeq: 10,
        activityEndSeq: 109,
        totalEventCount: 3,
        toolCallCount: 1,
        thoughtCount: 2,
        detailAvailable: true,
      },
    };
    const editTool: AcpUiEventVm = {
      id: 'tool-edit',
      seq: 30,
      timestamp: '30Z',
      kind: 'toolCall',
      sessionId: 'session-1',
      content: null,
      title: 'Edit file',
      toolCallId: 'call-edit',
      status: 'completed',
      startedSeq: 30,
      endedSeq: 30,
      raw: { _meta: { goldBandConversation: { toolName: 'Edit' } } },
    };
    const finalThought: AcpUiEventVm = {
      ...partialThought,
      id: 'thought-final',
      seq: 40,
      timestamp: '40Z',
      content: 'final thought',
      startedSeq: 40,
      endedSeq: 40,
    };
    vi.mocked(getAcpActivityDetail).mockResolvedValue({
      items: [partialThought, editTool, finalThought],
      hasMoreEarlier: false,
      earlierCursor: null,
    });
    const liveProjection = buildAcpTimelineProjection([partialThought], 'completed');
    const mixedProjection = buildAcpTimelineProjection([summary, partialThought], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={liveProjection.timeline} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      await act(async () => {
        root.render(<ACPMessageList timeline={mixedProjection.timeline} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      const trigger = container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]');
      await act(async () => {
        trigger?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
        await Promise.resolve();
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
          sessionId: 'session-1',
          activityStartSeq: 10,
          activityEndSeq: 109,
          earlierCursor: null,
          limit: 40,
        },
        undefined,
        undefined,
      );
      expect(container.textContent).toContain('Edit');
      expect(container.textContent?.match(/思考过程/g)).toHaveLength(2);
    } finally {
      await act(async () => root.unmount());
    }
  });

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
        {
          branchId: 'agent-1',
          sessionId: 'session-1',
          eventId: 'tool-10',
          toolCallId: 'call-10',
        },
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
          sessionId: 'session-1',
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

  it('coalesces activity revisions to one in-flight request and one latest trailing request', async () => {
    let resolveInitial!: (value: AcpActivityDetailVm) => void;
    let resolveLatest!: (value: AcpActivityDetailVm) => void;
    const initialRequest = new Promise<AcpActivityDetailVm>((resolve) => {
      resolveInitial = resolve;
    });
    const latestRequest = new Promise<AcpActivityDetailVm>((resolve) => {
      resolveLatest = resolve;
    });
    vi.mocked(getAcpActivityDetail)
      .mockReturnValueOnce(initialRequest)
      .mockReturnValueOnce(latestRequest);
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        const projection = buildAcpTimelineProjection([activitySummary('session-1', 100)], 'running');
        root.render(<ACPMessageList timeline={projection.timeline} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      expect(getAcpActivityDetail).toHaveBeenCalledTimes(1);

      await act(async () => {
        const projection = buildAcpTimelineProjection([activitySummary('session-1', 101)], 'running');
        root.render(<ACPMessageList timeline={projection.timeline} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await act(async () => {
        const projection = buildAcpTimelineProjection([activitySummary('session-1', 102)], 'running');
        root.render(<ACPMessageList timeline={projection.timeline} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
        await Promise.resolve();
      });
      expect(getAcpActivityDetail).toHaveBeenCalledTimes(1);

      await act(async () => {
        resolveInitial({ items: [], hasMoreEarlier: false, earlierCursor: null });
        await initialRequest;
      });
      await vi.waitFor(() => {
        expect(getAcpActivityDetail).toHaveBeenCalledTimes(2);
      });
      expect(vi.mocked(getAcpActivityDetail).mock.calls[1]?.[6]).toMatchObject({
        activityStartSeq: 10,
        activityEndSeq: 111,
        earlierCursor: null,
      });

      await act(async () => {
        resolveLatest({ items: [], hasMoreEarlier: false, earlierCursor: null });
        await latestRequest;
      });
      expect(getAcpActivityDetail).toHaveBeenCalledTimes(2);
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

  it('keeps four activity detail pages within a three-page window and can return to latest', async () => {
    vi.mocked(getAcpActivityDetail)
      .mockResolvedValueOnce(activityDetailPage(161, 200, 'before-161'))
      .mockResolvedValueOnce(activityDetailPage(121, 160, 'before-121'))
      .mockResolvedValueOnce(activityDetailPage(81, 120, 'before-81'))
      .mockResolvedValueOnce(activityDetailPage(41, 80, 'before-41'))
      .mockResolvedValueOnce(activityDetailPage(161, 200, 'before-161'));
    const projection = buildAcpTimelineProjection([activitySummary('session-1', 200)], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={projection.timeline} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });

      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      await clickButton(findButtonByText(container, '显示更早'));
      await clickButton(findButtonByText(container, '显示更早'));
      await clickButton(findButtonByText(container, '显示更早'));

      expect(container.querySelectorAll('[data-prompt-kit-tool="true"]')).toHaveLength(120);
      const returnToLatest = findButtonByText(container, '回到最新活动');
      expect(returnToLatest).not.toBeNull();

      await clickButton(returnToLatest);
      expect(container.querySelectorAll('[data-prompt-kit-tool="true"]')).toHaveLength(40);
      expect(container.textContent).toContain('Tool200');
      expect(container.textContent).not.toContain('Tool80');
      expect(getAcpActivityDetail).toHaveBeenCalledTimes(5);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('ignores an activity detail response owned by the previous session', async () => {
    let resolveDetail!: (value: AcpActivityDetailVm) => void;
    const detailPromise = new Promise<AcpActivityDetailVm>((resolve) => {
      resolveDetail = resolve;
    });
    vi.mocked(getAcpActivityDetail).mockReturnValue(detailPromise);
    const sessionA = buildAcpTimelineProjection([activitySummary('session-a')], 'completed');
    const sessionB = buildAcpTimelineProjection([activitySummary('session-b')], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={sessionA.timeline} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      await act(async () => {
        root.render(<ACPMessageList timeline={sessionB.timeline} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      await act(async () => {
        resolveDetail({
          items: [{ ...activityToolEvent(50, 'session-a'), title: 'stale-session-a-activity' }],
          hasMoreEarlier: false,
          earlierCursor: null,
        });
        await detailPromise;
      });

      expect(container.textContent).not.toContain('stale-session-a-activity');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not let a lower-position tool detail replace newer live output', async () => {
    let resolveDetail!: (value: { event: AcpUiEventVm | null }) => void;
    const detailPromise = new Promise<{ event: AcpUiEventVm | null }>((resolve) => {
      resolveDetail = resolve;
    });
    vi.mocked(getAcpToolDetail).mockReturnValue(detailPromise);
    const currentTool: AcpUiEventVm = {
      ...activityToolEvent(30),
      raw: {
        output: 'fresh-live-output',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const staleDetail: AcpUiEventVm = {
      ...currentTool,
      seq: 10,
      startedSeq: 10,
      endedSeq: 10,
      raw: {
        output: 'stale-detail-output',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[currentTool]} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      await act(async () => {
        resolveDetail({ event: staleDetail });
        await detailPromise;
      });

      expect(container.textContent).toContain('fresh-live-output');
      expect(container.textContent).not.toContain('stale-detail-output');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('ignores a tool detail response owned by the previous session', async () => {
    let resolveDetail!: (value: { event: AcpUiEventVm | null }) => void;
    const detailPromise = new Promise<{ event: AcpUiEventVm | null }>((resolve) => {
      resolveDetail = resolve;
    });
    vi.mocked(getAcpToolDetail).mockReturnValue(detailPromise);
    const sessionATool: AcpUiEventVm = {
      ...activityToolEvent(40, 'session-a'),
      raw: {
        rawInput: { path: 'session-a.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const sessionBTool: AcpUiEventVm = {
      ...sessionATool,
      sessionId: 'session-b',
      raw: {
        rawInput: { path: 'session-b.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const staleSessionADetail: AcpUiEventVm = {
      ...sessionATool,
      raw: {
        output: 'stale-session-a-output',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[sessionATool]} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      await act(async () => {
        root.render(<ACPMessageList timeline={[sessionBTool]} sessionStatus="completed" sending={false} branchLocator={locator} />);
      });
      await act(async () => {
        resolveDetail({ event: staleSessionADetail });
        await detailPromise;
      });

      expect(container.textContent).toContain('session-b.txt');
      expect(container.textContent).not.toContain('stale-session-a-output');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('starts the newer generation tool detail only after the previous request settles', async () => {
    let resolveGenerationOne!: (value: { event: AcpUiEventVm | null }) => void;
    let resolveGenerationTwo!: (value: { event: AcpUiEventVm | null }) => void;
    const generationOne = new Promise<{ event: AcpUiEventVm | null }>((resolve) => {
      resolveGenerationOne = resolve;
    });
    const generationTwo = new Promise<{ event: AcpUiEventVm | null }>((resolve) => {
      resolveGenerationTwo = resolve;
    });
    vi.mocked(getAcpToolDetail)
      .mockReturnValueOnce(generationOne)
      .mockReturnValueOnce(generationTwo);
    const tool: AcpUiEventVm = {
      ...activityToolEvent(50),
      raw: {
        rawInput: { path: 'generation.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const generationOneDetail: AcpUiEventVm = {
      ...tool,
      raw: {
        output: 'generation-1-stale-detail',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const generationTwoDetail: AcpUiEventVm = {
      ...tool,
      raw: {
        output: 'generation-2-current-detail',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);
    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[tool]} sessionStatus="completed" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);

      await act(async () => {
        root.render(<ACPMessageList timeline={[tool]} sessionStatus="completed" sending={false} branchLocator={locator} timelineGeneration={2} />);
        await Promise.resolve();
      });
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);

      await act(async () => {
        resolveGenerationOne({ event: generationOneDetail });
        await generationOne;
      });
      expect(container.textContent).not.toContain('generation-1-stale-detail');
      await vi.waitFor(() => {
        expect(getAcpToolDetail).toHaveBeenCalledTimes(2);
      });

      await act(async () => {
        resolveGenerationTwo({ event: generationTwoDetail });
        await generationTwo;
      });
      expect(container.textContent).toContain('generation-2-current-detail');
      expect(container.textContent).not.toContain('generation-1-stale-detail');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('queues one trailing tool detail when source content changes at the same position', async () => {
    let resolveInitial!: (value: { event: AcpUiEventVm | null }) => void;
    let resolveLatest!: (value: { event: AcpUiEventVm | null }) => void;
    const initialRequest = new Promise<{ event: AcpUiEventVm | null }>((resolve) => {
      resolveInitial = resolve;
    });
    const latestRequest = new Promise<{ event: AcpUiEventVm | null }>((resolve) => {
      resolveLatest = resolve;
    });
    vi.mocked(getAcpToolDetail)
      .mockReturnValueOnce(initialRequest)
      .mockReturnValueOnce(latestRequest);
    const initialTool: AcpUiEventVm = {
      ...activityToolEvent(60),
      raw: {
        rawInput: { path: 'initial-source.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const updatedTool: AcpUiEventVm = {
      ...initialTool,
      raw: {
        rawInput: { path: 'updated-source.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const initialDetail: AcpUiEventVm = {
      ...initialTool,
      raw: {
        output: 'stale-same-position-output',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const latestDetail: AcpUiEventVm = {
      ...updatedTool,
      raw: {
        output: 'latest-same-position-output',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[initialTool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);

      await act(async () => {
        root.render(<ACPMessageList timeline={[updatedTool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
        await Promise.resolve();
      });
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);

      await act(async () => {
        resolveInitial({ event: initialDetail });
        await initialRequest;
      });
      await vi.waitFor(() => {
        expect(getAcpToolDetail).toHaveBeenCalledTimes(2);
      });
      expect(container.textContent).not.toContain('stale-same-position-output');

      await act(async () => {
        resolveLatest({ event: latestDetail });
        await latestRequest;
      });
      expect(container.textContent).toContain('latest-same-position-output');
      expect(container.textContent).not.toContain('stale-same-position-output');
      expect(getAcpToolDetail).toHaveBeenCalledTimes(2);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not reload tool detail for a semantically identical raw snapshot clone', async () => {
    let resolveDetail!: (value: { event: AcpUiEventVm | null }) => void;
    const detailRequest = new Promise<{ event: AcpUiEventVm | null }>((resolve) => {
      resolveDetail = resolve;
    });
    vi.mocked(getAcpToolDetail)
      .mockReturnValueOnce(detailRequest)
      .mockResolvedValue({ event: null });
    const tool: AcpUiEventVm = {
      ...activityToolEvent(70),
      raw: {
        rawInput: { path: 'same-source.txt', options: { encoding: 'utf8' } },
        _meta: {
          goldBandConversation: {
            toolName: 'Read',
            toolDetailAvailable: true,
          },
        },
      },
    };
    const detail: AcpUiEventVm = {
      ...tool,
      raw: {
        output: 'semantically-stable-detail',
        _meta: {
          goldBandConversation: {
            toolName: 'Read',
            toolDetailAvailable: true,
          },
        },
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[tool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);

      const clonedTool = {
        ...tool,
        raw: JSON.parse(JSON.stringify(tool.raw)) as unknown,
      };
      await act(async () => {
        root.render(<ACPMessageList timeline={[clonedTool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
        await Promise.resolve();
      });

      await act(async () => {
        resolveDetail({ event: detail });
        await detailRequest;
      });
      await new Promise((resolve) => window.setTimeout(resolve, 0));

      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);
      expect(container.textContent).toContain('semantically-stable-detail');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('uses the timeline window session when a compact tool omits sessionId', async () => {
    const marker: AcpUiEventVm = {
      id: 'session-marker',
      seq: 1,
      timestamp: '1Z',
      kind: 'textDelta',
      sessionId: 'session-1',
      content: 'session marker',
      status: 'completed',
      startedSeq: 1,
      endedSeq: 1,
      raw: null,
    };
    const tool: AcpUiEventVm = {
      ...activityToolEvent(80),
      sessionId: null,
      raw: {
        rawInput: { path: 'owner-session.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const detail: AcpUiEventVm = {
      ...tool,
      sessionId: 'session-1',
      raw: {
        output: 'detail-owned-by-window-session',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    vi.mocked(getAcpToolDetail).mockResolvedValue({ event: detail });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[marker, tool]} sessionStatus="completed" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      await vi.waitFor(() => {
        expect(container.textContent).toContain('detail-owned-by-window-session');
      });
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('keeps same-position canonical output authoritative without rescanning tool detail', async () => {
    let resolveDetail!: (value: { event: AcpUiEventVm | null }) => void;
    const detailRequest = new Promise<{ event: AcpUiEventVm | null }>((resolve) => {
      resolveDetail = resolve;
    });
    vi.mocked(getAcpToolDetail).mockReturnValue(detailRequest);
    const initialTool: AcpUiEventVm = {
      ...activityToolEvent(85),
      raw: {
        rawInput: { path: 'canonical-output.txt' },
        output: 'initial-canonical-output',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const liveOutputTool: AcpUiEventVm = {
      ...initialTool,
      raw: {
        rawInput: { path: 'canonical-output.txt' },
        output: 'fresh-live-output',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const staleDetail: AcpUiEventVm = {
      ...initialTool,
      raw: {
        rawInput: { path: 'canonical-output.txt' },
        output: 'stale-detail-output',
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[initialTool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);

      await act(async () => {
        root.render(<ACPMessageList timeline={[liveOutputTool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
        await Promise.resolve();
      });
      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);

      await act(async () => {
        resolveDetail({ event: staleDetail });
        await detailRequest;
      });

      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);
      expect(container.textContent).toContain('fresh-live-output');
      expect(container.textContent).not.toContain('stale-detail-output');

      const loadedOutputTool: AcpUiEventVm = {
        ...liveOutputTool,
        raw: {
          rawInput: { path: 'canonical-output.txt' },
          output: 'newest-canonical-output',
          _meta: { goldBandConversation: { toolDetailAvailable: true } },
        },
      };
      await act(async () => {
        root.render(<ACPMessageList timeline={[loadedOutputTool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
        await Promise.resolve();
      });

      expect(getAcpToolDetail).toHaveBeenCalledTimes(1);
      expect(container.textContent).toContain('newest-canonical-output');
      expect(container.textContent).not.toContain('stale-detail-output');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not query tool detail until the timeline has a canonical session owner', async () => {
    const unownedTool: AcpUiEventVm = {
      ...activityToolEvent(86),
      sessionId: null,
      raw: {
        rawInput: { path: 'owner-pending.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    vi.mocked(getAcpToolDetail).mockResolvedValue({
      event: {
        ...unownedTool,
        sessionId: 'different-session',
        raw: {
          output: 'cross-session-detail',
          _meta: { goldBandConversation: { toolDetailAvailable: true } },
        },
      },
    });
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[unownedTool]} sessionStatus="completed" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));

      expect(getAcpToolDetail).not.toHaveBeenCalled();
      expect(container.textContent).not.toContain('cross-session-detail');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('does not query activity detail until the timeline has a canonical session owner', async () => {
    const unownedSummary: AcpUiEventVm = {
      ...activitySummary(),
      sessionId: null,
    };
    vi.mocked(getAcpActivityDetail).mockResolvedValue({
      items: [activityToolEvent(50, 'different-session')],
      hasMoreEarlier: false,
      earlierCursor: null,
    });
    const projection = buildAcpTimelineProjection([unownedSummary], 'completed');
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={projection.timeline} sessionStatus="completed" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));

      expect(getAcpActivityDetail).not.toHaveBeenCalled();
      expect(container.textContent).not.toContain('Tool 50');
    } finally {
      await act(async () => root.unmount());
    }
  });

  it('invalidates a tool detail error when the same-position source changes', async () => {
    vi.mocked(getAcpToolDetail)
      .mockRejectedValueOnce({ code: 'acp.tool-detail-query-failed', params: {} })
      .mockResolvedValueOnce({
        event: {
          ...activityToolEvent(90),
          raw: {
            output: 'detail-after-source-revision',
            _meta: { goldBandConversation: { toolDetailAvailable: true } },
          },
        },
      });
    const initialTool: AcpUiEventVm = {
      ...activityToolEvent(90),
      raw: {
        rawInput: { path: 'before-error.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const updatedTool: AcpUiEventVm = {
      ...initialTool,
      raw: {
        rawInput: { path: 'after-error.txt' },
        _meta: { goldBandConversation: { toolDetailAvailable: true } },
      },
    };
    const container = document.createElement('div');
    document.body.append(container);
    const root = createRoot(container);

    try {
      await act(async () => {
        root.render(<ACPMessageList timeline={[initialTool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await clickButton(container.querySelector<HTMLButtonElement>('[data-slot="collapsible-trigger"]'));
      await vi.waitFor(() => {
        expect(container.querySelector('[data-acp-tool-detail-retry="true"]')).not.toBeNull();
      });

      await act(async () => {
        root.render(<ACPMessageList timeline={[updatedTool]} sessionStatus="running" sending={false} branchLocator={locator} timelineGeneration={1} />);
      });
      await vi.waitFor(() => {
        expect(getAcpToolDetail).toHaveBeenCalledTimes(2);
      });
      expect(container.querySelector('[data-acp-tool-detail-retry="true"]')).toBeNull();
    } finally {
      await act(async () => root.unmount());
    }
  });
});
