/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, expect, it, vi } from 'vitest';
import { ACPMessageList, buildAcpTimelineProjection } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import type { AcpUiEventVm } from '@/types';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => { vi.unstubAllGlobals(); document.body.replaceChildren(); });

it('places archived process images before file changes and the assistant reply without opening the process', async () => {
  vi.stubGlobal('ResizeObserver', class { observe() {} unobserve() {} disconnect() {} });
  const image = { eventId: 'screenshot', pointer: '/rawOutput/result/content/0', contentHash: 'abc', mimeType: 'image/png' };
  const events: AcpUiEventVm[] = [
    { id: 'activity-1', seq: 1, timestamp: '1Z', kind: 'activitySummary', raw: {
      goldBandActivity: { activityStartSeq: 1, activityEndSeq: 90, totalEventCount: 90, images: [image] },
    } },
    { id: 'files', seq: 91, timestamp: '2Z', kind: 'fileChangeSet', raw: {} },
    { id: 'reply', seq: 92, timestamp: '3Z', kind: 'textDelta', content: 'Screenshot captured.' },
  ];
  const container = document.createElement('div'); document.body.append(container);
  const root = createRoot(container);
  try {
    await act(async () => root.render(<TooltipProvider><ACPMessageList
      timeline={buildAcpTimelineProjection(events, 'completed').timeline}
      sessionStatus="completed" sending={false}
    /></TooltipProvider>));
    const strip = container.querySelector('[data-acp-image-strip]');
    expect(strip).not.toBeNull();
    expect(strip?.querySelectorAll('[data-acp-image-thumbnail]')).toHaveLength(1);
    expect(container.querySelector('[data-slot="collapsible-trigger"]')?.getAttribute('data-state')).toBe('closed');
    expect(strip?.compareDocumentPosition(container.querySelector('[data-slot="collapsible-trigger"]')!) & Node.DOCUMENT_POSITION_PRECEDING).toBeTruthy();
    expect(container.textContent?.indexOf('Screenshot captured.')).toBeGreaterThan(-1);
    const live = buildAcpTimelineProjection([events[0]], 'running').timeline;
    await act(async () => root.render(<TooltipProvider><ACPMessageList timeline={live} sessionStatus="running" sending={false} /></TooltipProvider>));
    expect(container.querySelector('[data-acp-image-strip]')).toBeNull();
  } finally { await act(async () => root.unmount()); }
});
