/** @vitest-environment jsdom */
import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { TooltipProvider } from '@/components/ui/tooltip';
import { memoryEntryValid, type MemorySnapshot } from '@/lib/memory';

const api = vi.hoisted(() => ({ readProjectMemory: vi.fn(), writeProjectMemory: vi.fn() }));
vi.mock('@/api', () => api);
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock('@/components/ui/sheet', () => ({
  Sheet: ({ children, onOpenChange }: { children: React.ReactNode; onOpenChange: (open: boolean) => void }) => <div><button onClick={() => onOpenChange(false)}>close-sheet</button>{children}</div>,
  SheetContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SheetHeader: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  SheetTitle: ({ children }: { children: React.ReactNode }) => <h2>{children}</h2>,
  SheetDescription: ({ children }: { children: React.ReactNode }) => <p>{children}</p>,
}));
import { ProjectMemorySheet } from '@/components/conversation/ProjectMemorySheet';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const snapshot: MemorySnapshot = { projectId: 'project-a', taskId: null, workspacePath: '/memory.json', taskPath: null, workspace: [{ key: 'plan', value: 'B1', desc: 'plan', revision: 'r1' }], task: [], effective: [], limits: { entries: 100, key: 128, value: 4000, desc: 500, effectiveBytes: 32768 } };
let root: Root;
let container: HTMLDivElement;
beforeEach(() => { vi.clearAllMocks(); api.readProjectMemory.mockResolvedValue(structuredClone(snapshot)); container = document.createElement('div'); document.body.append(container); root = createRoot(container); });
afterEach(async () => { await act(async () => root.unmount()); document.body.replaceChildren(); });
async function mount(projectId = 'project-a', onClose = vi.fn()) {
  await act(async () => root.render(<TooltipProvider><ProjectMemorySheet key={projectId} projectId={projectId} name={projectId} onClose={onClose} /></TooltipProvider>)); return onClose;
}
function button(label: string) { return [...document.querySelectorAll<HTMLButtonElement>('button')].find(button => button.getAttribute('aria-label') === label || button.textContent === label)!; }
async function change(field: 'key' | 'value' | 'desc', value: string) {
  const input = container.querySelector<HTMLInputElement | HTMLTextAreaElement>(`[id$="-${field}"]`)!;
  await act(async () => { Object.getOwnPropertyDescriptor(field === 'key' ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype, 'value')!.set!.call(input, value); input.dispatchEvent(new Event('input', { bubbles: true })); });
}

describe('project memory interfaces', () => {
  it('counts Unicode scalar values, including astral characters', () => {
    expect(memoryEntryValid({ key: '😀'.repeat(128), value: '', desc: '' }, snapshot.limits)).toBe(true);
    expect(memoryEntryValid({ key: '😀'.repeat(129), value: '', desc: '' }, snapshot.limits)).toBe(false);
  });
  it('loads the selected scope, saves one row with its read revision and adopts authoritative output', async () => {
    await mount();
    expect(api.readProjectMemory).toHaveBeenCalledWith('project-a');
    expect(button('memory.save').disabled).toBe(true);
    await change('value', 'B2');
    api.writeProjectMemory.mockResolvedValue({ ...snapshot, workspace: [{ ...snapshot.workspace[0], value: 'canonical', revision: 'r2' }] });
    await act(async () => button('memory.save').click());
    expect(api.writeProjectMemory).toHaveBeenCalledWith('project-a', { scope: 'workspace', key: 'plan', expectedRevision: 'r1', entry: { key: 'plan', value: 'B2', desc: 'plan' } });
    expect(container.querySelector<HTMLTextAreaElement>('[id$="-value"]')!.value).toBe('canonical');
    expect(button('memory.save').disabled).toBe(true);
  });
  it('shows loading immediately and rejects responses after changing workspace', async () => {
    let resolve!: (value: MemorySnapshot) => void;
    api.readProjectMemory.mockReturnValueOnce(new Promise<MemorySnapshot>(done => { resolve = done; }));
    await mount();
    expect(container.querySelector('[role="status"]')?.textContent).toContain('memory.loading');
    await mount('project-b');
    await act(async () => resolve({ ...snapshot, workspace: [{ ...snapshot.workspace[0], value: 'late' }] }));
    expect(container.querySelector<HTMLTextAreaElement>('[id$="-value"]')!.value).toBe('B1');
  });
  it('keeps drafts after conflicts and requires explicit use of the latest value', async () => {
    await mount(); await change('value', 'my draft');
    api.writeProjectMemory.mockRejectedValue({ code: 'memory.conflict', params: { latest: { ...snapshot.workspace[0], value: 'latest', revision: 'r2' } } });
    await act(async () => button('memory.save').click());
    expect(container.querySelector<HTMLTextAreaElement>('[id$="-value"]')!.value).toBe('my draft');
    expect(button('memory.save').disabled).toBe(true);
    await act(async () => button('memory.useLatest').click());
    expect(container.querySelector<HTMLTextAreaElement>('[id$="-value"]')!.value).toBe('latest');
  });
  it('guards closing unsaved drafts and cancels without writing', async () => {
    const close = await mount(); await change('value', 'draft');
    await act(async () => button('close-sheet').click());
    expect(close).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('memory.unsavedTitle');
    await act(async () => button('memory.keepEditing').click());
    await act(async () => button('memory.cancel').click());
    await act(async () => button('close-sheet').click());
    expect(close).toHaveBeenCalledOnce();
    expect(api.writeProjectMemory).not.toHaveBeenCalled();
  });
  it('prevents duplicate writes and closing during persistence', async () => {
    const close = await mount(); await change('value', 'B2');
    let resolve!: (value: MemorySnapshot) => void;
    api.writeProjectMemory.mockReturnValue(new Promise<MemorySnapshot>(done => { resolve = done; }));
    await act(async () => { button('memory.save').click(); button('memory.save').click(); });
    expect(api.writeProjectMemory).toHaveBeenCalledOnce();
    expect(container.querySelector('[role="status"]')).not.toBeNull();
    await act(async () => button('close-sheet').click()); expect(close).not.toHaveBeenCalled();
    await act(async () => resolve(snapshot));
    await act(async () => button('close-sheet').click()); expect(close).toHaveBeenCalledOnce();
  });
  it('requires delete confirmation and releases the pending row after deletion', async () => {
    const close = await mount();
    await act(async () => button('memory.delete').click());
    expect(api.writeProjectMemory).not.toHaveBeenCalled();
    api.writeProjectMemory.mockResolvedValue({ ...snapshot, workspace: [] });
    await act(async () => {
      const confirm = [...document.querySelectorAll<HTMLButtonElement>('[role="alertdialog"] button')].find(button => button.textContent === 'memory.delete')!;
      confirm.click();
    });
    expect(api.writeProjectMemory).toHaveBeenCalledWith('project-a', { scope: 'workspace', key: 'plan', expectedRevision: 'r1', entry: null });
    expect(container.querySelector('[data-memory-row]')).toBeNull();
    await act(async () => button('close-sheet').click());
    expect(close).toHaveBeenCalledOnce();
  });
  it('retains drafts on save failure and permits retry', async () => {
    await mount(); await change('value', 'draft');
    api.writeProjectMemory.mockRejectedValue({ code: 'memory.io', params: { path: '/memory.json' } });
    await act(async () => button('memory.save').click());
    expect(container.textContent).toContain('/memory.json');
    expect(container.querySelector<HTMLTextAreaElement>('[id$="-value"]')!.value).toBe('draft');
    expect(button('memory.save').disabled).toBe(false);
  });
});
