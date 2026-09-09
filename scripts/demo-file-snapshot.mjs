import { createHash } from 'node:crypto';
import { imageSize } from 'image-size';

export function fileSnapshot(bytes, projectId, key, relativePath) {
  const hash = createHash('sha256').update(bytes).digest('hex');
  const canonicalPath = `/demo-history/${key}/files/${relativePath}`;
  const base = { locator: { projectId, canonicalPath, relativePath, scope: 'workspace' }, name: relativePath.split('/').at(-1), revision: { contentHash: hash, byteLength: bytes.length, modifiedAtNs: '0' }, externalAccessGrant: null };
  let image;
  try { image = imageSize(bytes); } catch { /* Non-image files use the existing text/unsupported snapshots. */ }
  if (image && ['png', 'jpg', 'gif', 'webp', 'bmp', 'ico', 'avif'].includes(image.type)) {
    const path = `${key}/images/${hash}.${image.type}`;
    return { imagePath: path, snapshot: { ...base, kind: 'image', mimeType: `image/${image.type === 'jpg' ? 'jpeg' : image.type}`, width: image.width, height: image.height,
      animated: image.type === 'gif' || (image.type === 'webp' && bytes.includes(Buffer.from('ANIM'))) || (image.type === 'png' && bytes.includes(Buffer.from('acTL'))),
      previewGrant: { token: `demo-history/${path}`, expiresAtMs: '8640000000000000' }, sourceEditable: false } };
  }
  if (bytes.length > 10 * 1024 * 1024) return { snapshot: { ...base, kind: 'unsupported', mimeType: null, limitationCode: 'workspace-file.too-large' } };
  let content;
  try { content = new TextDecoder('utf-8', { fatal: true }).decode(bytes); } catch { content = null; }
  if (content == null || content.includes('\0')) return { snapshot: { ...base, kind: 'unsupported', mimeType: null, limitationCode: 'workspace-file.format-unsupported' } };
  return { snapshot: { ...base, kind: 'text', content, encoding: 'utf-8', language: relativePath.endsWith('.md') ? 'markdown' : 'text', lineEnding: content.includes('\r\n') ? 'crlf' : 'lf', editable: false, limitationCode: null } };
}
