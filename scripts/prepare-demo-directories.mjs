import { readFile } from 'node:fs/promises';
import { imageSize } from 'image-size';

const imageTypes = { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', avif: 'image/avif', svg: 'image/svg+xml', bmp: 'image/bmp', ico: 'image/x-icon' };
export async function prepareArchiveDirectories(resources, emit, resolveResource) {
  const directories = new Map([['', new Map()]]);
  const images = new Map();
  for (const entry of resources) {
    if (!entry.source || /[\\:\0]|(^|\/)\.\.?($|\/)|^\/|\/$|\/\//.test(entry.source)) throw new Error('archive.directory-source-invalid');
    const parts = entry.source.split('/');
    let parent = '';
    for (const name of parts.slice(0, -1)) {
      const path = parent ? `${parent}/${name}` : name;
      if (!directories.has(path)) directories.set(path, new Map());
      const previous = directories.get(parent).get(name);
      if (previous && previous.kind !== 'directory') throw new Error('archive.directory-source-conflict');
      directories.get(parent).set(name, { name, kind: 'directory', source: path });
      parent = path;
    }
    const name = parts.at(-1);
    const extension = name.split('.').at(-1).toLowerCase();
    let image = null;
    if (imageTypes[extension] && entry.bytes <= 20 * 1024 * 1024) {
      if (!images.has(entry.sha256)) {
        let metadata = null;
        const { absolutePath } = await resolveResource(entry.source);
        const contents = await readFile(absolutePath);
        try {
          const dimensions = imageSize(contents);
          if (imageTypes[dimensions.type]) metadata = { mimeType: imageTypes[dimensions.type], width: dimensions.width, height: dimensions.height,
            animated: dimensions.type === 'gif' || dimensions.type === 'webp' };
        } catch {
          // Unrecognized image formats remain downloadable as ordinary files.
        }
        images.set(entry.sha256, metadata);
      }
      image = images.get(entry.sha256);
    }
    const previous = directories.get(parent).get(name);
    if (previous && (previous.kind !== 'file' || previous.resource.sha256 !== entry.sha256)) throw new Error('archive.directory-source-conflict');
    directories.get(parent).set(name, { name, kind: 'file', resource: { path: entry.path, bytes: entry.bytes, sha256: entry.sha256 }, image });
  }
  const references = new Map();
  const ordered = [...directories.keys()].sort((a, b) => b.split('/').length - a.split('/').length || b.length - a.length);
  for (const directory of ordered) {
    const entries = [...directories.get(directory).values()].sort((a, b) => Number(a.kind !== 'directory') - Number(b.kind !== 'directory')
      || a.name.toLowerCase().localeCompare(b.name.toLowerCase(), 'en'));
    const value = { entries: entries.map(entry => entry.kind === 'file' ? entry : {
      name: entry.name, kind: entry.kind, directory: references.get(entry.source), hasChildren: directories.get(entry.source).size > 0,
    }) };
    if (value.entries.some(entry => entry.kind === 'directory' && !entry.directory)) throw new Error('archive.directory-child-unprepared');
    references.set(directory, await emit(value));
  }
  return references;
}
