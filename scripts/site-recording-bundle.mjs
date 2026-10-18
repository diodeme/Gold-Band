import { createHash } from 'node:crypto';
import { mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import { dirname, extname, isAbsolute, relative, resolve, sep } from 'node:path';
import { JSDOM } from 'jsdom';
import { mapRecordingResources, recordingBytes } from './site-recording-assets.mjs';

const fail = (code, params = {}) => { throw { code, params }; };
const digest = bytes => createHash('sha256').update(bytes).digest('hex');

// Discover transitive dependencies before writing any deployable files.
export async function bundleRecording({ recording, captureOrigin, sourceRoot, outputRoot, publicPath, sourcePublicPath = '/' }) {
  const source = await realpath(sourceRoot);
  const origin = new URL(captureOrigin).origin;
  const files = new Map();
  const pending = new Map();
  if (!publicPath.startsWith('/') || !publicPath.endsWith('/') || publicPath.includes('..')) fail('site.resource-path');
  function resource(value, base = captureOrigin) {
    if (!value || value.startsWith('#') || value.startsWith('data:')) return value;
    const url = new URL(value, base);
    if (url.origin !== origin) fail('site.resource-external', { url: url.href });
    if (!url.pathname.startsWith(sourcePublicPath)) fail('site.resource-path', { path: url.pathname });
    const path = decodeURIComponent(url.pathname.slice(sourcePublicPath.length)).replace(/^\/+/, '');
    if (path.includes('\\') || path.split('/').includes('..')) fail('site.resource-path', { path });
    pending.set(path, url.href);
    return `${publicPath}resources/${path}${url.search}${url.hash}`;
  }
  const result = recordingBytes(mapRecordingResources(recording, value => resource(value)));
  const serialized = JSON.stringify(result);
  const durationMs = result.events.at(-1).timestamp - result.events[0].timestamp;
  if (result.reason !== 'manual' || result.events.length > 12000 || durationMs > 60000 || Buffer.byteLength(serialized) > 12 * 1024 * 1024) fail('site.recording-budget');
  function css(value, base, context = 'stylesheet') {
    const attributes = context === 'stylesheet' ? { _cssText: value } : { style: value };
    const mapped = mapRecordingResources({ events: [{ type: 2, data: { node: { id: 1, type: 2, tagName: 'style', attributes } } }] }, url => resource(url, base));
    return mapped.events[0].data.node.attributes[context === 'stylesheet' ? '_cssText' : 'style'];
  }
  for (const [path, base] of pending) {
    if (files.has(path)) continue;
    let actual;
    try { actual = await realpath(resolve(source, path)); } catch { fail('site.resource-missing', { path }); }
    const within = relative(source, actual);
    if (within === '..' || within.startsWith(`..${sep}`) || isAbsolute(within)) fail('site.resource-path', { path });
    let bytes = await readFile(actual);
    const extension = extname(path).toLowerCase();
    if (['.js', '.mjs', '.html'].includes(extension)) fail('site.resource-executable', { path });
    if (extension === '.css') bytes = Buffer.from(css(bytes.toString('utf8'), base));
    if (extension === '.svg') {
      const dom = new JSDOM(bytes.toString('utf8'), { contentType: 'image/svg+xml' });
      try {
        for (const node of dom.window.document.querySelectorAll('*')) {
          if (['script', 'foreignObject'].includes(node.localName)) fail('site.resource-executable', { path });
          for (const attribute of [...node.attributes]) {
            if (attribute.name.startsWith('on')) fail('site.resource-executable', { path });
            if (['href', 'xlink:href', 'src'].includes(attribute.name)) node.setAttribute(attribute.name, resource(attribute.value, base));
            else if (attribute.name === 'style') node.setAttribute('style', css(attribute.value, base, 'declarationList'));
            else if (attribute.value.includes('url(')) node.setAttribute(attribute.name, css(`${attribute.name}:${attribute.value}`, base, 'declarationList').slice(attribute.name.length + 1));
          }
          if (node.localName === 'style') node.textContent = css(node.textContent, base);
        }
        bytes = Buffer.from(dom.serialize());
      } finally { dom.window.close(); }
    }
    files.set(path, bytes);
  }
  const resources = [];
  for (const [path, bytes] of files) {
    const destination = resolve(outputRoot, 'resources', path);
    await mkdir(dirname(destination), { recursive: true });
    await writeFile(destination, bytes);
    resources.push({ url: `${publicPath}resources/${path}`, byteLength: bytes.length, sha256: digest(bytes) });
  }
  await mkdir(outputRoot, { recursive: true });
  await writeFile(resolve(outputRoot, 'events.json'), serialized);
  const metadata = { eventsUrl: `${publicPath}events.json`, byteLength: Buffer.byteLength(serialized), sha256: digest(serialized), eventCount: result.events.length, durationMs };
  await writeFile(resolve(outputRoot, 'resources.json'), JSON.stringify({ version: 1, resources }, null, 2));
  return { recording: result, metadata, resources };
}
