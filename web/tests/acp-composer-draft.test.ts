import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  AcpComposerDraftStore,
  queuedPromptToAcpComposerDraft,
} from '@/lib/acp-composer-draft';
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

  it('restores queued content, attachment paths, and structured quotes as one composer draft', () => {
    const draft = queuedPromptToAcpComposerDraft({
      content: '继续处理',
      quotes: [{ id: 'quote-1', sourceMessageKey: 'message-1', text: 'Agent 原文' }],
      attachmentPaths: ['C:/work/evidence.png'],
    }, [{
      path: 'C:/work/evidence.png',
      name: 'evidence.png',
      size: 24,
      previewUrl: 'asset://evidence',
    }]);

    expect(draft.content).toBe('继续处理');
    expect(draft.quotes).toEqual([{ id: 'quote-1', sourceKey: 'message-1', text: 'Agent 原文' }]);
    expect(draft.attachments).toHaveLength(1);
    expect(draft.attachments[0]).toMatchObject({
      name: 'evidence.png',
      path: 'C:/work/evidence.png',
      size: 24,
      mime: 'image/png',
      previewUrl: 'asset://evidence',
    });
  });

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

  it('enriches a restored queued draft only while the user has not changed it', () => {
    const store = new AcpComposerDraftStore();
    const restored = { content: 'queued', attachments: [attachment('fallback', 0)], quotes: [] };
    const enriched = { content: 'queued', attachments: [attachment('enriched', 12)], quotes: [] };
    store.restoreIfEmpty('session', restored);

    expect(store.replaceIfUnchanged('session', restored, enriched)).toBe(true);
    expect(store.read('session')).toBe(enriched);
    expect(store.replaceIfUnchanged('session', restored, restored)).toBe(false);
    expect(store.read('session')).toBe(enriched);
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
