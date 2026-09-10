import { unified } from 'unified';
import remarkParse from 'remark-parse';
import remarkGfm from 'remark-gfm';
import { parseFragment } from 'parse5';
import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const parser = unified().use(remarkParse).use(remarkGfm);
const digest = value => createHash('sha256').update(value).digest('hex');

export function messageReferences(markdown) {
  const tree = parser.parse(markdown), definitions = new Map(), entries = [];
  const walk = (node, visit) => { visit(node); for (const child of node.children ?? []) walk(child, visit); };
  walk(tree, node => { if (node.type === 'definition' && !definitions.has(node.identifier)) definitions.set(node.identifier, node.url); });
  walk(tree, node => {
    const position = node.position?.start;
    if (['link', 'image'].includes(node.type)) entries.push({ kind: node.type, value: node.url, position });
    if (['linkReference', 'imageReference'].includes(node.type)) entries.push({ kind: node.type,
      value: definitions.get(node.identifier) ?? null, identifier: node.identifier, position });
    if (node.type === 'inlineCode') entries.push({ kind: node.type, value: node.value, position });
    if (node.type === 'html') {
      const fragment = parseFragment(node.value, { sourceCodeLocationInfo: true });
      const html = element => {
        for (const attr of element.attrs ?? []) {
          if (['href', 'src', 'srcset', 'poster', 'data'].includes(attr.name)) entries.push({
            kind: 'htmlAttribute', tag: element.tagName, attribute: attr.name, value: attr.value,
            position, htmlPosition: element.sourceCodeLocation?.attrs?.[attr.name]?.startOffset,
          });
        }
        for (const child of element.childNodes ?? []) html(child);
        if (element.content) html(element.content);
      };
      html(fragment);
    }
  });
  return entries;
}

export async function auditMessageReferences(directory) {
  const manifestBytes = await readFile(resolve(directory, 'manifest.json'));
  const manifest = JSON.parse(manifestBytes), resources = new Map();
  for (const entry of [...manifest.assets, ...manifest.resources]) {
    const previous = resources.get(entry.path);
    if (previous && (previous.sha256 !== entry.sha256 || previous.bytes !== entry.bytes)) throw new Error('audit.resource-conflict');
    resources.set(entry.path, entry);
  }
  const readAsset = async path => {
    const metadata = resources.get(path);
    if (!metadata || !/^(?:assets|resources)\/[a-f0-9]+(?:\.json)?$/.test(path)) throw new Error('audit.resource-path');
    const bytes = await readFile(resolve(directory, path));
    if (bytes.length !== metadata.bytes || digest(bytes) !== metadata.sha256) throw new Error('audit.resource-integrity');
    return JSON.parse(bytes);
  };
  const counts = { sessions: 0, branches: 0, pages: 0, events: 0, bodies: 0 };
  const records = [];
  for (const session of manifest.sessions) {
    counts.sessions++;
    const locator = Object.fromEntries(['projectId', 'taskId', 'runId', 'roundId', 'nodeId', 'attemptId', 'outerNodeId', 'outerAttemptId']
      .filter(key => session[key] !== undefined).map(key => [key, session[key]]));
    const detail = await readAsset(session.detail);
    for (const branch of detail.branches) {
      counts.branches++;
      for (const page of branch.pages) {
        counts.pages++;
        for (const event of await readAsset(page.path)) {
          counts.events++;
          if (typeof event.content !== 'string') continue;
          counts.bodies++;
          const entries = messageReferences(event.content);
          if (entries.length) records.push({ locator, branchId: branch.id, eventId: event.id, seq: event.seq,
            page: page.path, bodySha256: digest(event.content), entries });
        }
      }
    }
  }
  const byKind = {};
  for (const record of records) for (const entry of record.entries) byKind[entry.kind] = (byKind[entry.kind] ?? 0) + 1;
  return { version: 1, scope: 'All event.content Markdown AST links, reference links, inline code and HTML href/src/srcset/poster/data attributes. Inline code and srcset values are candidates, not resolved files. Excludes code blocks, tool payloads and directory document bodies.',
    manifestSha256: digest(manifestBytes), counts, byKind, records };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 4) throw new Error('Usage: node scripts/audit-demo-message-references.mjs <archive-directory> <report.json>');
  const report = await auditMessageReferences(resolve(process.argv[2]));
  await writeFile(resolve(process.argv[3]), JSON.stringify(report, null, 2));
  console.log(JSON.stringify({ counts: report.counts, byKind: report.byKind, manifestSha256: report.manifestSha256 }));
}
