import { readFile, writeFile } from 'node:fs/promises';
import { gunzipSync } from 'node:zlib';
import { resolve, join } from 'node:path';

const [input, output] = process.argv.slice(2);
if (!input || !output) throw new Error('Findings JSON and output directory are required');
const findings = JSON.parse(await readFile(resolve(input), 'utf8'));
const accepted = []; const pending = []; const reasons = {};
let currentFile; let lines; let completeHashes;
for (const finding of findings) {
  if (finding.File !== currentFile) {
    currentFile = finding.File;
    const bytes = await readFile(currentFile);
    const text = (currentFile.endsWith('.gz') ? gunzipSync(bytes) : bytes).toString('utf8');
    lines = text.split('\n');
    completeHashes = [...new Set(text.match(/\b[a-f0-9]{64}\b/gi) ?? [])];
  }
  const line = lines[finding.StartLine - 1] ?? '';
  const prefix = finding.Match.split('REDACTED')[0];
  let reason;
  if (finding.RuleID === 'generic-api-key') {
    if (['idempotencyKey":"', 'yKey":"', 'potencyKey":"'].includes(prefix) && /idempotencyKey":"[a-f0-9]{32}"/i.test(line)) reason = 'canonical-idempotency-identity';
    else if (prefix === 'Sec-WebSocket-Key: ' && /Sec-WebSocket-Key:\s*[a-z0-9+/]{22}==/i.test(line)) reason = 'rfc6455-public-handshake-nonce';
    else if (prefix === 'OAuth terminal: \x60' && /OAuth terminal:\s*\x60[a-f0-9]{40}\x60/i.test(line)) reason = 'documented-git-commit';
    else if (prefix === 'API baseline SHA-256=\x60' && /API baseline SHA-256=\x60[a-f0-9]{64}\x60/i.test(line)) reason = 'documented-content-checksum';
    else if (prefix === 'API baseline SHA-256=\x60' && currentFile.endsWith('acp.raw.jsonl.gz')) {
      const partial = /API baseline SHA-256=\x60([a-f0-9]{32,63})"/i.exec(line)?.[1];
      if (partial && completeHashes.some((hash) => hash.startsWith(partial))) reason = 'streamed-content-checksum-prefix';
    } else if (prefix === 'phase-2-credentials-oauth","' && /"dependsOn":\[[^\]]*"phase-2-credentials-oauth","phase-2-(?:observability-artifacts|workspace-tools)"/.test(line)) reason = 'canonical-workflow-dependency';
  }
  if (reason) { accepted.push(finding); reasons[reason] = (reasons[reason] ?? 0) + 1; }
  else pending.push({ rule: finding.RuleID, file: finding.File, line: finding.StartLine, fingerprint: finding.Fingerprint });
}
await writeFile(join(resolve(output), 'reviewed-secret-baseline.json'), JSON.stringify(accepted));
await writeFile(join(resolve(output), 'secret-review.json'), JSON.stringify({ total: findings.length, accepted: accepted.length, reasons, pending }, null, 2));
console.log(JSON.stringify({ total: findings.length, accepted: accepted.length, reasons, pending: pending.length }));
if (pending.length) process.exitCode = 1;
