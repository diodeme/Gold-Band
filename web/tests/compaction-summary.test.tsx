/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
vi.mock('@/api', async () => ({ ...await vi.importActual<typeof import('@/api')>('@/api'), getAcpCompactionSummary: vi.fn() }));
vi.mock('@/components/workspace/files/WorkspaceFileEditor', () => ({
  WorkspaceFileEditor: ({ value, editable }: { value: string; editable: boolean }) => <pre data-source-view data-editable={String(editable)}>{value}</pre>,
}));
import '@/i18n';
import { getAcpCompactionSummary } from '@/api';
import { ACPMessageList } from '@/components/acp/ACPChatDialog';
import { TooltipProvider } from '@/components/ui/tooltip';
import { CompactionSummaryWorkspacePanel } from '@/components/workspace/CompactionSummaryWorkspacePanel';
import { createCompactionSummaryWorkspaceResource, createConversationWorkspaceScope, RightWorkspaceProvider, useRightWorkspace, type AcpAttemptWorkspaceLocator } from '@/components/workspace/right-workspace-context';
import { hasCompactionSummary } from '@/lib/acp-compaction-summary';
globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const locator: AcpAttemptWorkspaceLocator = { projectId: 'p', taskId: 't', runId: 'r', roundId: 'round', nodeId: 'n', attemptId: 'a', branchId: 'root' };
const resource = createCompactionSummaryWorkspaceResource({ scopeKey: 'scope', title: '压缩摘要', locator, eventId: 'cmp' });
let host: HTMLDivElement;
let root: Root;
beforeEach(() => { host = document.createElement('div'); document.body.append(host); root = createRoot(host); });
afterEach(async () => { await act(async () => root.unmount()); host.remove(); vi.clearAllMocks(); });
function Probe() {
  const workspace = useRightWorkspace();
  return <output data-tabs={workspace.tabs.length} data-active={workspace.activeTabKey} data-open={String(workspace.requestedOpen)}>{JSON.stringify(workspace.tabs)}</output>;
}
describe('compaction summary', () => {
  it('uses availability metadata and rejects empty or non-text summaries', () => {
    for (const raw of [null, {}, { summary: null }, { summary: [] }, { summary: [{ type: 'text', text: '  ' }] }, { summary: [{ type: 'image', data: 'x' }] }]) expect(hasCompactionSummary(raw)).toBe(false);
    expect(hasCompactionSummary({ summary: [{ type: 'text', text: '# summary' }] })).toBe(true);
    expect(hasCompactionSummary({ compactionSummaryAvailable: false, summary: [{ type: 'text', text: 'stale' }] })).toBe(false);
    expect(hasCompactionSummary({ compactionSummaryAvailable: true })).toBe(true);
  });
  it('opens one scoped locator tab without loading or storing body in navigation state', async () => {
    await act(async () => root.render(<TooltipProvider><RightWorkspaceProvider scope={createConversationWorkspaceScope(locator)}>
      <ACPMessageList timeline={[
        { id: 'a:cmp', seq: 1, kind: 'contextCompaction', timestamp: '', status: 'completed', raw: { compactionSummaryAvailable: true, goldBandScope: { originalId: 'cmp' } } },
        { id: 'empty', seq: 2, kind: 'contextCompaction', timestamp: '', status: 'completed', raw: { summary: [] } },
        { id: 'running', seq: 3, kind: 'contextCompaction', timestamp: '', status: 'running', raw: { summary: [{ type: 'text', text: 'partial' }] } },
      ]} sessionStatus="completed" sending={false} branchLocator={locator} /><Probe />
    </RightWorkspaceProvider></TooltipProvider>));
    const buttons = host.querySelectorAll<HTMLButtonElement>('[data-compaction-summary-link]');
    expect(buttons).toHaveLength(1);
    expect(buttons[0].parentElement?.textContent).not.toContain('上下文压缩完成');
    expect(buttons[0].parentElement?.classList.contains('flex-wrap')).toBe(true);
    expect(buttons[0].getAttribute('data-variant')).toBe('link');
    expect(buttons[0].classList.contains('font-normal')).toBe(true);
    await act(async () => buttons[0].click());
    await act(async () => buttons[0].click());
    const probe = host.querySelector('output')!;
    expect(probe.dataset.tabs).toBe('1');
    expect(probe.dataset.open).toBe('true');
    const tabs = JSON.parse(probe.textContent!);
    expect(tabs[0].locator).toEqual({ ...locator, eventId: 'cmp' });
    expect(tabs[0]).not.toHaveProperty('markdown');
    expect(getAcpCompactionSummary).not.toHaveBeenCalled();
    expect(createCompactionSummaryWorkspaceResource({ scopeKey: 'scope', title: 'same', locator: { ...locator, projectId: 'other' }, eventId: 'cmp' }).key).not.toBe(resource.key);
  });
  it('loads on mount, shows loading, renders Markdown and switches to exact read-only source', async () => {
    let resolve!: (value: { markdown: string }) => void;
    vi.mocked(getAcpCompactionSummary).mockReturnValue(new Promise(done => { resolve = done; }));
    await act(async () => root.render(<TooltipProvider><CompactionSummaryWorkspacePanel resource={resource} /></TooltipProvider>));
    expect(host.textContent).toContain('正在加载摘要');
    const markdown = '# Summary\n\n**保留内容**\n\n' + 'long context '.repeat(4_000) + '\nEND';
    await act(async () => resolve({ markdown }));
    expect(getAcpCompactionSummary).toHaveBeenCalledWith(resource.locator, { branchId: 'root', eventId: 'cmp' });
    expect(host.querySelector('h1')?.textContent).toBe('Summary');
    expect(host.querySelector('strong')?.textContent).toBe('保留内容');
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="切换到源码模式"]')!.click());
    expect(host.querySelector('[data-source-view]')?.textContent).toBe(markdown);
    expect(host.querySelector('[data-source-view]')?.getAttribute('data-editable')).toBe('false');
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="渲染 Markdown"]')!.click());
    expect(host.querySelector('h1')?.textContent).toBe('Summary');
    expect(getAcpCompactionSummary).toHaveBeenCalledTimes(1);
  });
  it('shows a recoverable error and handles a summary cleared since the row was loaded', async () => {
    vi.mocked(getAcpCompactionSummary).mockRejectedValueOnce({ code: 'acp.compaction-summary-query-failed', params: {} }).mockResolvedValueOnce({ markdown: null });
    await act(async () => root.render(<TooltipProvider><CompactionSummaryWorkspacePanel resource={resource} /></TooltipProvider>));
    expect(host.querySelector('[role="alert"]')).not.toBeNull();
    await act(async () => host.querySelector<HTMLButtonElement>('button')!.click());
    expect(host.textContent).toContain('摘要已清除或不可用');
    expect(getAcpCompactionSummary).toHaveBeenCalledTimes(2);
  });
  it('ignores a late response after switching to another compaction', async () => {
    let finishOld!: (value: { markdown: string }) => void;
    vi.mocked(getAcpCompactionSummary).mockReturnValueOnce(new Promise(done => { finishOld = done; })).mockResolvedValueOnce({ markdown: '# New summary' });
    await act(async () => root.render(<TooltipProvider><CompactionSummaryWorkspacePanel key={resource.key} resource={resource} /></TooltipProvider>));
    const next = createCompactionSummaryWorkspaceResource({ scopeKey: 'scope', title: 'Next', locator, eventId: 'cmp-2' });
    await act(async () => root.render(<TooltipProvider><CompactionSummaryWorkspacePanel key={next.key} resource={next} /></TooltipProvider>));
    await act(async () => finishOld({ markdown: '# Stale summary' }));
    expect(host.querySelector('h1')?.textContent).toBe('New summary');
  });
});
