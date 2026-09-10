import { readFile } from 'node:fs/promises';
import { imageSize } from 'image-size';

const mimeTypes = { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', avif: 'image/avif', svg: 'image/svg+xml', bmp: 'image/bmp', ico: 'image/x-icon' };
const published = resource => ({ path: resource.path, bytes: resource.bytes, sha256: resource.sha256 });
export async function prepareArchiveChangeSet(source, resource) {
  const original = await resource(source);
  const changeSet = JSON.parse(await readFile(original.absolutePath, 'utf8'));
  const prefix = source.slice(0, source.lastIndexOf('/turn-file-change-sets/'));
  const versions = {};
  const attachments = {};
  const missing = [];
  for (const change of changeSet.changes) for (const version of [change.beforeVersion, change.afterVersion]) {
    if (!version || versions[version.contentHash]) continue;
    if (!/^[a-f0-9]{64}$/.test(version.contentHash)) throw new Error('archive.version-hash-invalid');
    const path = `${prefix}/acp.file-blobs/${version.contentHash.slice(0, 2)}/${version.contentHash}`;
    try {
      const entry = await resource(path);
      if (entry.sourceBytes !== version.byteLength) throw new Error('archive.version-size-mismatch');
      versions[version.contentHash] = published(entry);
    } catch (error) {
      if (error.message !== 'archive.required-resource-missing') throw error;
      missing.push({ path, reason: 'file-version-missing' });
    }
  }
  for (const attachment of changeSet.attachments) {
    if (!attachment.relativePath || /(^|[\\/])\.\.([\\/]|$)|^[\\/]|:/.test(attachment.relativePath)) throw new Error('archive.attachment-path-invalid');
    const path = `${prefix}/attachments/${attachment.relativePath.replaceAll('\\', '/')}`;
    try {
      const entry = await resource(path);
      const bytes = await readFile(entry.absolutePath);
      let format;
      try {
        const dimensions = imageSize(bytes);
        if (mimeTypes[dimensions.type]) format = { kind: 'image', mimeType: mimeTypes[dimensions.type], width: dimensions.width, height: dimensions.height,
          animated: dimensions.type === 'gif' || dimensions.type === 'webp' };
      } catch { /* Non-image attachments use the text or unsupported contract. */ }
      if (!format) {
        try {
          const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
          format = text.includes('\0') ? { kind: 'unsupported' } : { kind: 'text', encoding: 'utf-8',
            lineEnding: text.includes('\r\n') ? text.replaceAll('\r\n', '').includes('\n') ? 'mixed' : 'crlf' : 'lf' };
        } catch { format = { kind: 'unsupported' }; }
      }
      attachments[attachment.id] = { ...published(entry), ...format };
    } catch (error) {
      if (error.message !== 'archive.required-resource-missing') throw error;
      missing.push({ path, reason: 'attachment-missing' });
    }
  }
  return { changeSet, versions, attachments, missing };
}
