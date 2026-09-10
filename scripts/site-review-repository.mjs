import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { promisify } from 'node:util';
import content from '../marketing/site/review-content.json' with { type: 'json' };

const execute = promisify(execFile);
const hash = value => createHash('sha256').update(value).digest('hex');
const failure = code => Object.assign(new Error(code), { code });
export async function createReviewRepository(language) {
  if (!['zh', 'en'].includes(language)) throw failure('site.review-language-invalid');
  const directory = await mkdtemp(join(tmpdir(), 'gold-band-site-review-'));
  const root = join(directory, 'workspace');
  const hooks = join(directory, 'empty-hooks');
  await mkdir(root);
  await mkdir(hooks);
  const globalConfig = join(directory, 'gitconfig');
  await writeFile(globalConfig, '');
  const env = { ...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_'))),
    GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_GLOBAL: globalConfig };
  const git = async (...args) => (await execute('git', ['-c', `core.hooksPath=${hooks}`, '-c', 'commit.gpgSign=false',
    '-c', 'user.name=Gold Band Demo', '-c', 'user.email=demo@example.invalid', ...args],
  { cwd: root, env, windowsHide: true, timeout: 10_000, maxBuffer: 128 * 1024 })).stdout;
  const paths = content.files.map(file => file.path);
  const filePath = path => {
    if (!paths.includes(path)) throw failure('site.review-path-invalid');
    return join(root, path);
  };
  const dispose = async () => {
    const target = resolve(directory);
    if (dirname(target) !== resolve(tmpdir()) || !target.startsWith(join(resolve(tmpdir()), 'gold-band-site-review-'))) throw failure('site.review-cleanup-invalid');
    await rm(target, { recursive: true, force: true });
  };
  try {
    await git('-c', `init.templateDir=${hooks}`, 'init', '-b', 'preview');
    await git('config', 'core.autocrlf', 'false');
    const gitVersion = (await git('--version')).trim().replace(/^git version /, '');
    for (const file of content.files) {
      await mkdir(dirname(filePath(file.path)), { recursive: true });
      if (file.before !== null) await writeFile(filePath(file.path), file.before);
    }
    await git('add', '--', ...paths.filter(path => content.files.find(file => file.path === path).before !== null));
    await git('commit', '-m', 'Initial workspace');
    const initialHead = (await git('rev-parse', 'HEAD')).trim();
    for (const file of content.files) await writeFile(filePath(file.path), file.after[language]);
    const read = async path => {
      const value = await readFile(filePath(path), 'utf8');
      const metadata = await stat(filePath(path), { bigint: true });
      return { path, content: value, hash: hash(value), bytes: Buffer.byteLength(value), modifiedAtNs: metadata.mtimeNs.toString() };
    };
    const revisionContent = async (path, area) => {
      filePath(path);
      if (area === 'worktree') return (await read(path)).content;
      const exists = area === 'index' ? await git('ls-files', '--', path) : await git('ls-tree', '--name-only', 'HEAD', '--', path);
      return exists.trim() ? await git('show', `${area === 'index' ? '' : 'HEAD'}:${path}`) : null;
    };
    const status = async () => {
      const raw = await git('status', '--porcelain=v1', '-z', '--untracked-files=all');
      const head = (await git('rev-parse', 'HEAD')).trim();
      const workingHashes = await Promise.all(paths.map(async path => (await read(path)).hash));
      return { root, initialHead, head, gitVersion, revision: hash(head + raw + workingHashes.join('') + (await git('diff', '--cached', '--', ...paths))),
        changes: raw.split('\0').filter(Boolean).map(row => ({ index: row[0], worktree: row[1], path: row.slice(3) })) };
    };
    let pending = Promise.resolve();
    let closed = false;
    const run = command => {
      if (closed) return Promise.reject(failure('site.review-closed'));
      const operation = pending.then(async () => {
        if (['stage', 'commit'].includes(command.action) && command.expectedRevision != null
          && command.expectedRevision !== (await status()).revision) throw failure('git.snapshot-stale');
        if (command.action === 'read') return read(command.path);
        if (command.action === 'status') return status();
        if (command.action === 'write') {
          const current = await read(command.path);
          if (current.hash !== command.expectedHash) throw failure('workspace-file.changed-on-disk');
          if (typeof command.content !== 'string' || Buffer.byteLength(command.content) > 16_384) throw failure('site.review-content-invalid');
          const path = filePath(command.path);
          await writeFile(`${path}.tmp`, command.content);
          await rename(`${path}.tmp`, path);
          return read(command.path);
        }
        if (command.action === 'comparison') {
          if (!['staged', 'unstaged'].includes(command.area)) throw failure('site.review-operation-rejected');
          const before = await revisionContent(command.path, command.area === 'staged' ? 'head' : 'index');
          const after = await revisionContent(command.path, command.area === 'staged' ? 'index' : 'worktree');
          const raw = await git('diff', ...(command.area === 'staged' ? ['--cached'] : []), '--numstat', '--', command.path);
          const counts = raw.trim().split('\t');
          return { path: command.path, before, after, stats: { addedLines: raw ? Number(counts[0]) : before === null ? (after?.split('\n').length ?? 1) - 1 : 0, deletedLines: raw ? Number(counts[1]) : 0 } };
        }
        if (command.action === 'stage') {
          for (const path of command.paths) filePath(path);
          if (!command.paths.length) throw failure('site.review-path-invalid');
          await git('add', '--', ...command.paths);
          return status();
        }
        if (command.action === 'commit') {
          if (typeof command.subject !== 'string' || !command.subject.trim() || command.subject.length > 120) throw failure('site.review-subject-invalid');
          await git('commit', '-m', command.subject.trim());
          return status();
        }
        throw failure('site.review-operation-rejected');
      });
      pending = operation.catch(() => {});
      return operation;
    };
    return { root, initialHead, run, dispose: async () => { closed = true; await pending; await dispose(); } };
  } catch (error) { await dispose(); throw error; }
}
