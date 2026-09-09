import * as cssTree from 'css-tree';
import parseSrcset from 'parse-srcset';

const resourceAttributes = new Set(['src', 'poster', 'background', 'xlink:href']);
const hrefElements = new Set(['link', 'use', 'image']);

// rrweb mirror IDs identify style text and resource attributes in later mutations.
export function mapRecordingResources(recording, mapUrl) {
  const result = structuredClone(recording);
  const nodes = new Map();
  function css(value, context = 'stylesheet') {
    if (value == null || value === '') return value;
    const ast = cssTree.parse(value, { context, parseCustomProperty: true });
    cssTree.walk(ast, (node) => {
      if (node.type === 'Url') node.value = mapUrl(node.value);
      if (node.type === 'Function' && ['image-set', '-webkit-image-set'].includes(node.name.toLowerCase())) {
        node.children.forEach((child) => { if (child.type === 'String') child.value = mapUrl(child.value); });
      }
      if (node.type === 'Atrule' && node.name.toLowerCase() === 'import' && node.prelude) {
        const first = node.prelude.children.first;
        if (first?.type === 'String') first.value = mapUrl(first.value);
      }
    });
    return cssTree.generate(ast);
  }
  function attributes(values, tag, previous = {}) {
    const merged = { ...previous, ...values };
    const executable = tag === 'script' || (tag === 'link' &&
      (String(merged.rel).toLowerCase() === 'modulepreload' ||
        (String(merged.rel).toLowerCase() === 'preload' && String(merged.as).toLowerCase() === 'script')));
    if (executable) {
      // Keep mirror identity, but make nonvisual executable hints inert in replay.
      delete values.href; delete values.src;
    }
    for (const [key, value] of Object.entries(values ?? {})) {
      if (typeof value === 'string') {
        if (resourceAttributes.has(key) || (key === 'href' && hrefElements.has(tag))) values[key] = mapUrl(value);
        else if (key === '_cssText') values[key] = css(value);
        else if (key === 'style') values[key] = css(value, 'declarationList');
        else if (key === 'srcset' && value.trim()) values[key] = parseSrcset(value).map((candidate) => {
          const descriptors = ['w', 'h', 'd'].filter((key) => candidate[key] != null)
            .map((key) => `${candidate[key]}${key === 'd' ? 'x' : key}`);
          return [mapUrl(candidate.url), ...descriptors].join(' ');
        }).join(', ');
      } else if (key === 'style' && value && typeof value === 'object') {
        for (const [property, declaration] of Object.entries(value)) {
          if (typeof declaration === 'string') value[property] = css(declaration, 'value');
          else if (Array.isArray(declaration)) declaration[0] = css(declaration[0], 'value');
        }
      }
    }
  }
  function snapshot(node, parentTag) {
    if (!node) return;
    const style = node.isStyle || parentTag === 'style';
    nodes.set(node.id, { tag: node.tagName, style, attributes: { ...node.attributes } });
    attributes(node.attributes, node.tagName);
    if (node.type === 3 && style) node.textContent = css(node.textContent);
    for (const child of node.childNodes ?? []) snapshot(child, node.tagName);
  }
  for (const event of result.events) {
    const data = event.data;
    if (event.type === 2) { nodes.clear(); snapshot(data.node); }
    else if (event.type === 3 && data.source === 0) {
      for (const add of data.adds ?? []) snapshot(add.node, nodes.get(add.parentId)?.tag);
      for (const change of data.attributes ?? []) {
        const node = nodes.get(change.id);
        const merged = { ...node?.attributes, ...change.attributes };
        attributes(change.attributes, node?.tag, node?.attributes);
        if (node) node.attributes = merged;
      }
      for (const change of data.texts ?? []) if (nodes.get(change.id)?.style) change.value = css(change.value);
    } else if (event.type === 3 && data.source === 8) {
      for (const add of data.adds ?? []) add.rule = css(add.rule);
      for (const key of ['replace', 'replaceSync']) if (data[key] != null) data[key] = css(data[key]);
    } else if (event.type === 3 && data.source === 13 && data.set) {
      data.set.value = css(data.set.value, 'value');
    } else if (event.type === 3 && data.source === 10 && !data.buffer) {
      data.fontSource = css(data.fontSource, 'value');
    } else if (event.type === 3 && data.source === 15) {
      for (const style of data.styles ?? []) for (const rule of style.rules) rule.rule = css(rule.rule);
    }
  }
  return result;
}

export function recordingBytes(recording) {
  recording.bytes = 0;
  let bytes = Buffer.byteLength(JSON.stringify(recording), 'utf8');
  while (bytes !== recording.bytes) {
    recording.bytes = bytes;
    bytes = Buffer.byteLength(JSON.stringify(recording), 'utf8');
  }
  return recording;
}

export function portableRecording(recording, captureOrigin) {
  const origin = new URL(captureOrigin).origin;
  return recordingBytes(mapRecordingResources(recording, (value) => {
    if (!value || value.startsWith('#') || value.startsWith('data:')) return value;
    const url = new URL(value, captureOrigin);
    if (url.origin !== origin) throw { code: 'site.resource-external', params: { url: value } };
    return `${url.pathname}${url.search}${url.hash}`;
  }));
}

export function recordingResourceUrls(recording) {
  const urls = new Set();
  mapRecordingResources(recording, (url) => { if (url && !url.startsWith('#') && !url.startsWith('data:')) urls.add(url); return url; });
  return [...urls];
}
