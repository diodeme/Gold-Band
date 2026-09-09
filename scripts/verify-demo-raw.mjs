import { readFile, writeFile } from 'node:fs/promises';
import { gunzipSync } from 'node:zlib';
import { resolve, join } from 'node:path';

const [input, reportPath] = process.argv.slice(2);
if (!input || !reportPath) throw new Error('Dataset and report paths are required');
const root = resolve(input);
const load = async (path) => JSON.parse(await readFile(join(root, path), 'utf8'));
const dataset = await load('dataset.json'); const manifest = await load('manifest.json');
const report = { sessions: 0, frames: 0, failures: [] };
for (const ref of dataset.sessions.filter((item) => item.branchId === 'root')) {
  try {
    const prefix = `rounds/${ref.roundId}/nodes/${ref.outerNodeId}/${ref.outerAttemptId}/dynamic/nodes/${ref.nodeId}/${ref.attemptId}/`;
    const archive = manifest.files.find((file) => file.source === `${prefix}acp.raw.jsonl`);
    const text = gunzipSync(await readFile(join(root, archive.path))).toString('utf8').trimEnd();
    const lines = text ? text.split('\n') : [];
    const session = await load(ref.path); const raw = await load(session.raw.path);
    let count = 0;
    for (const page of raw.pages) {
      for (const item of await load(page.path)) {
        if (item.content !== lines[count] || item.lineNumber !== count + 1 || item.contentTruncated) throw new Error(`Frame mismatch at ${count + 1}`);
        count++;
      }
    }
    if (count !== lines.length || count !== raw.frames.length || count !== session.session.diagnostics.rawFrameCount) throw new Error('Frame count mismatch');
    report.sessions++; report.frames += count;
  } catch (error) { report.failures.push({ session: ref.path, error: error.message }); }
}
await writeFile(resolve(reportPath), JSON.stringify(report, null, 2));
console.log(JSON.stringify(report));
if (report.failures.length) process.exitCode = 1;
