import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';

import { ACPMessageList, buildAcpTimelineProjection, pendingPermissionFromEvents, PermissionRequestCard, permissionRequestSummary } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpUiEventVm } from '@/types';

function renderPermissionCard() {
  return renderToStaticMarkup(
    React.createElement(
      TooltipProvider,
      null,
      React.createElement(PermissionRequestCard, {
        request: {
          requestId: 'permission-1',
          title: 'Permission required',
          raw: {},
          options: [
            {
              optionId: 'allow-command-prefix',
              kind: 'allow_always',
              name: 'Allow Commands Starting With `rg --files -g !*node_modules*`',
            },
            {
              optionId: 'reject-once',
              kind: 'reject_once',
              name: 'Reject',
            },
          ],
        },
        onSelect: () => undefined,
      }),
    ),
  );
}

describe('PermissionRequestCard', () => {
  it('uses a compact low-emphasis approval surface', () => {
    const html = renderPermissionCard();

    expect(html).toContain('acp-permission-request-card');
    expect(html).toContain('max-w-2xl');
    expect(html).toContain('bg-card/65');
    expect(html).toContain('bg-accent/65');
    expect(html).toContain('bg-background/45');
    expect(html).not.toContain('bg-primary text-primary-foreground');
  });

  it('exposes the full option label from the truncated button trigger', () => {
    const html = renderPermissionCard();
    const longLabel = 'Allow Commands Starting With `rg --files -g !*node_modules*`';

    expect(html).toContain('data-slot="tooltip-trigger"');
    expect(html).toContain(`aria-label="${longLabel}"`);
    expect(html).toContain('min-w-0 truncate');
  });

  it('shows what command the permission decision applies to', () => {
    const request = {
      requestId: 'permission-2',
      title: 'PowerShell',
      toolCallId: 'call-powershell',
      raw: {
        toolCall: {
          title: 'PowerShell',
          rawInput: {
            description: 'List projects under the ai directory',
            command: 'Get-ChildItem -Force "D:\\Projects\\code\\ai"',
          },
        },
      },
      options: [{ optionId: 'allow', kind: 'allow_once', name: 'Allow' }],
    };

    expect(permissionRequestSummary(request)).toBe(
      'List projects under the ai directory · Get-ChildItem -Force "D:\\Projects\\code\\ai"',
    );
    const html = renderToStaticMarkup(
      React.createElement(
        TooltipProvider,
        null,
        React.createElement(PermissionRequestCard, {
          request,
          onSelect: () => undefined,
        }),
      ),
    );
    expect(html).toContain('List projects under the ai directory');
    expect(html).toContain('Get-ChildItem -Force &quot;D:\\Projects\\code\\ai&quot;');
  });

  it('does not render resolved permission records', () => {
    const html = renderToStaticMarkup(
      React.createElement(
        TooltipProvider,
        null,
        React.createElement(PermissionRequestCard, {
          request: {
            requestId: 'permission-3',
            title: 'PowerShell',
            raw: {
              optionId: 'allow',
              toolCall: { rawInput: { command: 'git status --short' } },
            },
            options: [{ optionId: 'allow', kind: 'allow_once', name: 'Allow once' }],
          },
          status: 'selected',
        }),
      ),
    );

    expect(html).toBe('');
  });

  it('shows the ACP-provided Skill name and arguments without inferring intent', () => {
    expect(permissionRequestSummary({
      requestId: 'permission-skill',
      title: 'Skill',
      raw: {
        toolCall: {
          title: 'Skill',
          rawInput: {
            skill: 'prompt-kit',
            args: '只读 very thorough 调查仓库',
          },
        },
      },
      options: [{ optionId: 'allow', kind: 'allow_once', name: 'Allow' }],
    })).toBe('prompt-kit · 只读 very thorough 调查仓库');
  });

  it('keeps permission cards out of the conversation timeline', () => {
    const event = (partial: Partial<AcpUiEventVm>): AcpUiEventVm => ({
      id: 'event', seq: 1, timestamp: '1Z', kind: 'toolCall', sessionId: 'session',
      content: null, title: null, toolCallId: null, status: null, raw: null,
      ...partial,
    });
    const permission = event({
      id: 'permission', seq: 2, timestamp: '2Z', kind: 'permissionRequest', title: 'Skill', status: 'pending',
      raw: {
        requestId: 'json-rpc-skill',
        toolCall: { title: 'Skill', rawInput: { skill: 'prompt-kit', args: 'read only' } },
        options: [{ optionId: 'allow', kind: 'allow_once', name: 'Allow' }],
      },
    });
    const projection = buildAcpTimelineProjection([permission], 'running');

    const html = renderToStaticMarkup(
      React.createElement(TooltipProvider, null,
        React.createElement(ACPMessageList, {
          timeline: projection.timeline,
          sessionStatus: 'running',
          sending: false,
        }),
      ),
    );
    expect(html).not.toContain('prompt-kit');
    expect(html).not.toContain('acp-permission-request-card');
    expect(pendingPermissionFromEvents([permission], new Set())).toMatchObject({ requestId: 'json-rpc-skill' });
  });

  it('selects only the latest pending permission for the intervention layer', () => {
    const event = (partial: Partial<AcpUiEventVm>): AcpUiEventVm => ({
      id: 'event', seq: 1, timestamp: '1Z', kind: 'toolCall', sessionId: 'session',
      content: null, title: null, toolCallId: null, status: null, raw: null,
      ...partial,
    });
    const events = [
      event({
        id: 'permission', seq: 3, timestamp: '3Z', kind: 'permissionRequest',
        toolCallId: 'skill-call', title: 'Skill', status: 'pending',
        raw: {
          requestId: 'json-rpc-skill',
          toolCall: { title: 'Skill', rawInput: { skill: 'prompt-kit', args: 'read only' } },
          options: [{ optionId: 'allow', kind: 'allow_once', name: 'Allow' }],
        },
      }),
      event({
        id: 'permission-selected', seq: 4, timestamp: '4Z', kind: 'permissionRequest',
        toolCallId: 'selected-skill-call', title: 'Permission required', status: 'selected',
        raw: {
          requestId: 'json-rpc-selected',
          optionId: 'allow',
          toolCall: { title: 'Skill', rawInput: { skill: 'selected-hidden-skill', args: 'audit only' } },
          options: [{ optionId: 'allow', kind: 'allow_once', name: 'Allow once' }],
        },
      }),
    ];

    const pending = pendingPermissionFromEvents(events, new Set());
    expect(pending).toMatchObject({ requestId: 'json-rpc-skill', title: 'Skill' });
    expect(pending?.raw).not.toMatchObject({ requestId: 'json-rpc-selected' });
  });
});
