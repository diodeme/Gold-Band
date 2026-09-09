import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { createHash, randomUUID } from 'node:crypto';

const execute = promisify(execFile);
export const AFTER_FILES = ['src/config.json', 'docs/workspace-notes.md'];
const hash = content => createHash('sha256').update(content).digest('hex');
const reject = code => { throw { code: `recording.after.${code}`, params: {} }; };

export async function createAfterRepository(parent, language) {
  await mkdir(parent, { recursive: true });
  const directory = await mkdtemp(join(resolve(parent), 'after-'));
  const git = async (...args) => (await execute('git', ['-c', 'core.longpaths=true', '-c', 'core.autocrlf=false', '-c', 'commit.gpgsign=false', ...args], { cwd: directory, windowsHide: true })).stdout;
  const en = language === 'en';
  const before = { [AFTER_FILES[0]]: '{\n  "workspace": "default",\n  "snapshots": false\n}\n', [AFTER_FILES[1]]: null };
  const captured = { [AFTER_FILES[0]]: '{\n  "workspace": "default",\n  "snapshots": true\n}\n',
    [AFTER_FILES[1]]: en ? '# Workspace notes\n\nKeep a file snapshot for every turn.\nReview changes before committing.\n' : '# 工作区说明\n\n每轮保留文件快照。\n审阅修改后再提交。\n' };
  const report = en ? '# Workspace review\n\n- Enabled per-turn file snapshots.\n- Added workspace notes.\n\nReview src/config.json and docs/workspace-notes.md before committing.\n' : '# 工作区审阅报告\n\n- 已启用每轮文件快照。\n- 已补充工作区说明。\n\n提交前请审阅 src/config.json 和 docs/workspace-notes.md。\n';
  await mkdir(join(directory, 'src')); await mkdir(join(directory, 'docs'));
  await git('init', '--initial-branch=review/workspace');
  await git('config', 'user.name', 'Gold Band Recording'); await git('config', 'user.email', 'recording@example.invalid');
  await writeFile(join(directory, AFTER_FILES[0]), before[AFTER_FILES[0]]);
  await git('add', '--', AFTER_FILES[0]); await git('commit', '-m', 'Initialize workspace configuration');
  for (const path of AFTER_FILES) await writeFile(join(directory, path), captured[path]);
  const initialHead = (await git('rev-parse', 'HEAD')).trim();
  const revision = async () => hash(await git('status', '--porcelain=v1', '-z') + await git('diff', 'HEAD') + await git('diff', '--cached') + (await readFile(join(directory, AFTER_FILES[1]), 'utf8')));
  const validatePath = path => { if (!AFTER_FILES.includes(path)) reject('path-invalid'); };
  const read = async path => { validatePath(path); const content = await readFile(join(directory, path), 'utf8'); return { content, revision: { contentHash: hash(content), byteLength: Buffer.byteLength(content), modifiedAtNs: '0' } }; };
  async function snapshot() {
    const changes = (await git('status', '--porcelain=v1', '--untracked-files=all', '-z')).split('\0').filter(Boolean).map(row => ({ index: row[0], worktree: row[1], path: row.slice(3) }));
    const head = (await git('rev-parse', 'HEAD')).trim();
    return { changes, head, initialHead, revision: await revision(), subject: (await git('log', '-1', '--format=%s')).trim(), directory };
  }
  return { directory, before, captured, report, read, snapshot,
    async save(path, content, expectedHash) {
      validatePath(path); if (typeof content !== 'string' || Buffer.byteLength(content) > 65536) reject('content-invalid');
      const current = await read(path); if (current.revision.contentHash !== expectedHash) reject('revision-conflict');
      await writeFile(join(directory, path), content); return (await read(path)).revision;
    },
    async comparison(path, area) {
      validatePath(path); if (!['staged', 'unstaged'].includes(area)) reject('area-invalid');
      const blob = async ref => { try { return await git('show', ref); } catch { return null; } };
      const before = await blob(area === 'staged' ? `HEAD:${path}` : `:${path}`), after = area === 'staged' ? await blob(`:${path}`) : (await read(path)).content;
      const stats = (await git('diff', ...(area === 'staged' ? ['--cached'] : []), '--numstat', '--', path)).trim().split('\t');
      return { before, after, stats: { addedLines: before === null ? after?.split('\n').length - 1 : Number(stats[0] || 0), deletedLines: Number(stats[1] || 0) } };
    },
    async mutate(input) {
      if (input.expectedRevision && input.expectedRevision !== await revision()) reject('revision-conflict');
      if (input.kind === 'stage-paths' || input.kind === 'unstage-paths') {
        if (!Array.isArray(input.paths) || !input.paths.length || input.paths.length > AFTER_FILES.length) reject('path-invalid');
        input.paths.forEach(validatePath);
        await git(...(input.kind === 'stage-paths' ? ['add', '--', ...input.paths] : ['restore', '--staged', '--', ...input.paths]));
      } else if (input.kind === 'stage-all') await git('add', '--', ...AFTER_FILES);
      else if (input.kind === 'unstage-all') await git('restore', '--staged', '--', ...AFTER_FILES);
      else if (input.kind === 'commit') {
        if (typeof input.subject !== 'string' || !input.subject.trim() || input.subject.length > 200) reject('subject-invalid');
        await git('commit', '-m', input.subject, ...(input.body ? ['-m', String(input.body).slice(0, 2000)] : []));
      } else reject('operation-unavailable');
      return snapshot();
    },
  };
}

// This local capture bridge owns only repositories it creates under the declared evidence directory.
export function afterRepositoryPlugin() {
  const repositories = new Map();
  const install = server => { server.middlewares.use('/__recording/after', async (req, res) => {
    if (req.method !== 'POST' || !['127.0.0.1', '::1', '::ffff:127.0.0.1'].includes(req.socket.remoteAddress)) { res.statusCode = 403; res.end(); return; }
    try {
      let body = ''; for await (const chunk of req) { body += chunk; if (Buffer.byteLength(body) > 70000) reject('request-too-large'); }
      const input = JSON.parse(body); let result;
      if (input.action === 'start') {
        if (!process.env.RECORDING_GIT_ROOT || repositories.size >= 12) reject('repository-unavailable');
        const repo = await createAfterRepository(process.env.RECORDING_GIT_ROOT, input.language);
        const id = randomUUID(); repositories.set(id, repo);
        result = { id, before: repo.before, captured: repo.captured, report: repo.report };
      } else {
        const repo = repositories.get(input.id); if (!repo) reject('repository-unavailable');
        if (input.action === 'snapshot') result = await repo.snapshot();
        else if (input.action === 'read') result = await repo.read(input.path);
        else if (input.action === 'save') result = await repo.save(input.path, input.content, input.expectedHash);
        else if (input.action === 'comparison') result = await repo.comparison(input.path, input.area);
        else if (input.action === 'mutate') result = await repo.mutate(input.input);
        else reject('operation-unavailable');
      }
      res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify({ result }));
    } catch (error) { res.statusCode = 400; res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify({ error: error?.code?.startsWith?.('recording.') ? error : { code: 'recording.after.operation-failed', params: {} } })); }
  }); };
  return { name: 'after-recording-repository', configureServer: install, configurePreviewServer: install };
}
