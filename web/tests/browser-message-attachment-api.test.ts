// @vitest-environment jsdom
import { inflateSync } from 'node:zlib';
import { afterEach, describe, expect, it } from 'vitest';
import { browserApi } from '@/api/browser';

const locator = ['default', 'mock-task', 'run-052', 'round-001', 'dev', 'attempt-001'] as const;

function preview(path: string, attemptId: string = locator[5]) {
  return browserApi.showConversationMessageAttachment(locator[0], locator[1], locator[2], locator[3], locator[4], attemptId, 'image.png', path);
}

afterEach(() => window.history.replaceState(null, '', '/'));

describe('browser message attachment fixtures', () => {
  it('serves three distinct PNG contents by persisted path, not display name', async () => {
    window.history.replaceState(null, '', '/?attachmentIdentity=1');
    const session = await browserApi.getAcpSession(...locator);
    const attachments = session!.events
      .filter((event) => event.id.startsWith('browser-user-image-'))
      .flatMap((event) => event.raw.attachments as { name: string; path: string; size: number }[]);
    expect(attachments).toHaveLength(3);
    expect(new Set(attachments.map((attachment) => attachment.name))).toEqual(new Set(['image.png']));
    expect(new Set(attachments.map((attachment) => attachment.path)).size).toBe(3);
    const contents = await Promise.all(attachments.map((attachment) => preview(attachment.path)));
    expect(new Set(contents.map((content) => content.content)).size).toBe(3);
    for (const [index, content] of contents.entries()) {
      expect(content.metadata.path).toBe(attachments[index].path);
      const bytes = Buffer.from(content.content.split(',')[1], 'base64');
      expect(bytes.length).toBe(attachments[index].size);
      expect(bytes.readUInt32BE(16)).toBe(32);
      expect(bytes.readUInt32BE(20)).toBe(16);
      const pixels = inflateSync(bytes.subarray(41, 41 + bytes.readUInt32BE(33)));
      expect(pixels.length).toBe((32 * 3 + 1) * 16);
      expect([...pixels.subarray(1, 4)]).toEqual([[220, 70, 60], [30, 150, 100], [55, 100, 215]][index]);
    }
    expect(await preview(attachments[0].path)).toEqual(contents[0]);
  });

  it('does not resolve an unknown path or a different attempt by filename', async () => {
    await expect(preview('user-inputs/image.png')).rejects.toMatchObject({ code: 'attachment.not-found' });
    await expect(preview(`user-inputs/${'1'.repeat(32)}/image.png`, 'attempt-002')).rejects.toMatchObject({ code: 'attachment.not-found' });
    await expect(browserApi.showConversationMessageAttachment('other-project', locator[1], locator[2], locator[3], locator[4], locator[5], 'image.png', `user-inputs/${'1'.repeat(32)}/image.png`))
      .rejects.toMatchObject({ code: 'attachment.not-found' });
  });
});
