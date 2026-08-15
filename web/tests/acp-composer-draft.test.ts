import { afterEach, describe, expect, it, vi } from 'vitest';
import { AcpComposerDraftStore } from '@/lib/acp-composer-draft';
import type { AttachmentItem } from '@/lib/attachment-service';

function attachment(id: string, size = 1, preview = false): AttachmentItem {
  return {
    id,
    name: `${id}.png`,
    size,
    mime: 'image/png',
    source: 'browser-file',
    previewUrl: preview ? `blob:${id}` : undefined,
  };
}

describe('ACP follow-up composer draft store', () => {
  afterEach(() => vi.restoreAllMocks());

  it('restores text and attachments by full session locator while isolating another session', () => {
    const store = new AcpComposerDraftStore();
    const firstKey = 'project-a/task-a/run-1/round-1/node-a/attempt-1/root';
    const secondKey = 'project-a/task-a/run-1/round-1/node-b/attempt-1/root';
    store.write(firstKey, { content: '继续检查', attachments: [attachment('image')], quotes: [] });

    expect(store.read(firstKey)).toEqual({ content: '继续检查', attachments: [attachment('image')], quotes: [] });
    expect(store.read(secondKey)).toEqual({ content: '', attachments: [], quotes: [] });
  });

  it('removes an empty draft after a successful send or explicit clear', () => {
    const store = new AcpComposerDraftStore();
    store.write('session', { content: 'send me', attachments: [attachment('file')], quotes: [] });
    store.write('session', { content: '', attachments: [], quotes: [] });

    expect(store.size).toBe(0);
    expect(store.read('session')).toEqual({ content: '', attachments: [], quotes: [] });
  });

  it('restores a failed detached draft only into its original empty session', () => {
    const store = new AcpComposerDraftStore();
    const detached = { content: 'session A', attachments: [], quotes: [] };
    store.write('session-b', { content: 'session B', attachments: [], quotes: [] });

    expect(store.restoreIfEmpty('session-a', detached)).toBe(true);
    expect(store.restoreIfEmpty('session-b', detached)).toBe(false);
    expect(store.read('session-a').content).toBe('session A');
    expect(store.read('session-b').content).toBe('session B');
  });

  it('keeps storage bounded and releases preview URLs when an old draft is evicted', () => {
    const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    const store = new AcpComposerDraftStore(2, 10);
    store.write('one', { content: '1', attachments: [attachment('one', 4, true)], quotes: [] });
    store.write('two', { content: '2', attachments: [attachment('two', 4, true)], quotes: [] });
    store.write('three', { content: '3', attachments: [attachment('three', 4, true)], quotes: [] });

    expect(store.size).toBe(2);
    expect(store.read('one')).toEqual({ content: '', attachments: [], quotes: [] });
    expect(revoke).toHaveBeenCalledWith('blob:one');
  });

  it('disposes every retained preview URL on application exit', () => {
    const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    const store = new AcpComposerDraftStore();
    store.write('one', { content: '', attachments: [attachment('one', 1, true)], quotes: [] });
    store.write('two', { content: 'text', attachments: [attachment('two', 1, true)], quotes: [] });

    store.dispose();

    expect(store.size).toBe(0);
    expect(revoke).toHaveBeenCalledTimes(2);
  });
});
