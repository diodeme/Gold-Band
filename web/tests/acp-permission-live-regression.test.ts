import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import {
  ACPMessageList,
  PermissionRequestCard,
  buildAcpTimelineProjection,
  canInferPendingInteractionFromWindow,
  latestLiveSessionTimingFromEvents,
  liveTimelineUpdatesFromEvents,
  pendingPermissionFromEvents,
} from '@/components/acp/ACPChatDialog';
import { InterventionLayer } from '@/components/conversation/InterventionLayer';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpUiEventVm } from '@/types';

describe('live ACP permission delivery', () => {
  it('renders an actionable card from the permission event without waiting for a session refresh', () => {
    const permissionEvent: AcpUiEventVm = {
      id: 'permission-json-rpc-7',
      seq: 7,
      timestamp: '7Z',
      kind: 'permissionRequest',
      sessionId: 'session-1',
      content: null,
      title: 'mcp__code_graph__list_projects',
      toolCallId: 'tool-7',
      status: 'pending',
      timing: {
        sessionElapsedSeconds: 12,
        revision: 7,
        observedAt: '7Z',
        paused: true,
        waitReason: 'permission',
      },
      raw: {
        requestId: 'json-rpc-7',
        toolCall: {
          toolCallId: 'tool-7',
          title: 'mcp__code_graph__list_projects',
          rawInput: { path: 'D:\\Projects\\code\\ai' },
        },
        options: [
          { optionId: 'allow-once', name: 'Allow', kind: 'allow_once' },
          { optionId: 'reject', name: 'Reject', kind: 'reject_once' },
        ],
      },
    };

    const timing = latestLiveSessionTimingFromEvents([permissionEvent]);
    expect(timing).toMatchObject({
      revision: 7,
      paused: true,
      waitReason: 'permission',
    });
    expect(canInferPendingInteractionFromWindow(
      { status: 'running', timing },
      false,
      'permission',
    )).toBe(true);

    const liveTimeline = liveTimelineUpdatesFromEvents([permissionEvent]);
    const pending = pendingPermissionFromEvents(liveTimeline, new Set());
    expect(pending).toMatchObject({
      interactionId: 'json-rpc-7',
      title: 'mcp__code_graph__list_projects',
    });

    const projection = buildAcpTimelineProjection(liveTimeline, 'running');
    const timelineHtml = renderToStaticMarkup(
      React.createElement(
        TooltipProvider,
        null,
        React.createElement(ACPMessageList, {
          timeline: projection.timeline,
          sessionStatus: 'running',
          sending: false,
          onPermissionSelect: () => undefined,
        }),
      ),
    );
    expect(timelineHtml).not.toContain('acp-permission-request-card');

    const html = renderToStaticMarkup(
      React.createElement(
        TooltipProvider,
        null,
        React.createElement(
          InterventionLayer,
          null,
          pending ? React.createElement(PermissionRequestCard, { request: pending, onSelect: () => undefined }) : null,
        ),
      ),
    );

    expect(html).toContain('data-conversation-intervention-layer="true"');
    expect(html).toContain('mx-auto w-full max-w-[var(--conversation-content-rail-max-inline-size)]');
    expect(html).toContain('space-y-4 px-5 pb-10');
    expect(html).toContain('acp-permission-request-card');
    expect(html).toContain('mcp__code_graph__list_projects');
    expect(html).toContain('Allow');
    expect(html).toContain('Reject');
  });
});
