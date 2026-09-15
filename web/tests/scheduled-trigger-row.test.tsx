import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ScheduledTriggerRow } from '@/components/conversation/ScheduledTriggerRow';
import i18n from '@/i18n';

const payload = (triggerKind: 'scheduled' | 'manual', instructionSummary: string) => ({ projectId: 'project-1', scheduledTaskId: 'scheduled-1', occurrenceId: 'occurrence-1', triggerKind, acceptedAt: '2026-08-27T00:00:00Z', instructionSummary, contentFingerprint: 'fingerprint', links: { taskId: 'task-1', runId: 'run-1' } });

describe('ScheduledTriggerRow', () => {
  it('renders automatic and manual labels from structured payloads', async () => {
    await i18n.changeLanguage('en');
    const automatic = renderToStaticMarkup(<TooltipProvider><ScheduledTriggerRow payload={payload('scheduled', 'Nightly check')} onOpen={() => {}} /></TooltipProvider>);
    const manual = renderToStaticMarkup(<TooltipProvider><ScheduledTriggerRow payload={payload('manual', 'Run now')} onOpen={() => {}} /></TooltipProvider>);
    expect(automatic).toContain('Scheduled');
    expect(manual).toContain('Manual');
  });

  it('keeps long summaries constrained to the timeline width', () => {
    const html = renderToStaticMarkup(<TooltipProvider><ScheduledTriggerRow payload={payload('scheduled', 'x'.repeat(400))} onOpen={() => {}} /></TooltipProvider>);
    expect(html).toContain('min-w-0');
    expect(html).toContain('truncate');
  });
});
