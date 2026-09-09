import { readFile, writeFile, mkdir } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export async function indexResources(root, resources) {
  const hash = (value) => createHash('sha256').update(value).digest('hex');
  async function publish(path, value) {
    const bytes = Buffer.from(JSON.stringify(value)); await mkdir(dirname(join(root, path)), { recursive: true }); await writeFile(join(root, path), bytes);
    return { path, byteLength: bytes.length, sha256: hash(bytes) };
  }
  const sessions = {};
  for (const [key, session] of Object.entries(resources.sessions)) {
    const tree = { directories: new Map(), entries: [] };
    for (const [path, ref] of Object.entries(session.attachments)) {
      const segments = path.split('/'); let directory = tree;
      for (const segment of segments.slice(0, -1)) {
        if (!directory.directories.has(segment)) directory.directories.set(segment, { directories: new Map(), entries: [] });
        directory = directory.directories.get(segment);
      }
      directory.entries.push({ name: segments.at(-1), relativePath: path, canonicalPath: ref.canonicalPath, kind: 'file', hasChildren: false, byteLength: null, modifiedAtNs: null, resource: ref });
    }
    const emitDirectory = async (directory, path = '') => {
      const entries = [...directory.entries];
      for (const [name, child] of directory.directories) {
        const relativePath = path ? `${path}/${name}` : name;
        entries.push({ name, relativePath, canonicalPath: `/demo-history/${key}/attachments/${relativePath}`, kind: 'directory', hasChildren: true, byteLength: null, modifiedAtNs: null, resource: await emitDirectory(child, relativePath) });
      }
      entries.sort((a, b) => a.name.localeCompare(b.name));
      return publish(`${key}/directories/${hash(path)}.json`, entries);
    };
    const changes = {};
    for (const [id, ref] of Object.entries(session.changes)) {
      const { comparisons, ...resource } = ref;
      changes[id] = { ...resource, comparisonIndex: await publish(`${key}/comparisons/${hash(id)}-index.json`, comparisons) };
    }
    sessions[key] = await publish(`${key}/resources.json`, { changes, directory: await emitDirectory(tree) });
  }
  const reference = await publish('resources.json', { sessions });
  const dataset = JSON.parse(await readFile(join(root, 'dataset.json'), 'utf8'));
  dataset.resources = reference; await publish('dataset.json', dataset);
  return reference;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const root = resolve(process.argv[2]);
  console.log(await indexResources(root, JSON.parse(await readFile(join(root, 'resources.json'), 'utf8'))));
}
