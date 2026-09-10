import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { execFile } from 'node:child_process';
import { copyFile, link, mkdir, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { promisify } from 'node:util';
import { pathToFileURL } from 'node:url';

const execute = promisify(execFile);
export const PRIVACY_SCAN_LIMITS = { archiveDepth: 5, decodeDepth: 5, timeoutSeconds: 1800 };

async function archiveAliases(directory, temporary) {
  const root = await realpath(directory);
  let manifest;
  try { manifest = JSON.parse(await readFile(join(root, 'manifest.json'), 'utf8')); }
  catch (error) { if (error.code === 'ENOENT') return []; throw error; }
  if (!Array.isArray(manifest.resources)) throw new Error('archive.privacy-manifest-invalid');
  const aliases = new Map();
  await mkdir(join(temporary, 'archive-resources'));
  for (const resource of manifest.resources) {
    const extension = /\.(tar\.gz|tgz|gz|zip|jar|war|ear|tar)$/i.exec(resource.source ?? '')?.[1].toLowerCase();
    if (!extension) continue;
    if (!/^resources\/[a-f0-9]{64}$/.test(resource.path) || resource.path !== `resources/${resource.sha256}`
      || !Number.isSafeInteger(resource.bytes) || resource.bytes < 0) throw new Error('archive.privacy-resource-invalid');
    if (aliases.has(resource.path)) continue;
    const source = await realpath(join(root, resource.path));
    if (!source.startsWith(`${root}${sep}`)) throw new Error('archive.privacy-resource-outside-root');
    const format = ['jar', 'war', 'ear'].includes(extension) ? 'zip' : extension === 'tgz' ? 'tar.gz' : extension;
    const target = join(temporary, 'archive-resources', `${resource.sha256}.${format}`);
    try { await link(source, target); }
    catch (error) { if (error.code !== 'EXDEV') throw error; await copyFile(source, target); }
    const hash = createHash('sha256');
    let bytes = 0;
    for await (const chunk of createReadStream(target)) { hash.update(chunk); bytes += chunk.length; }
    if (bytes !== resource.bytes || hash.digest('hex') !== resource.sha256) throw new Error('archive.privacy-resource-changed');
    aliases.set(resource.path, { source, target });
  }
  return [...aliases.values()];
}

// Only scanner-redacted context is retained; the original match and Secret never enter this report.
export function summarizePrivacy(findings, scannerVersion) {
  if (!Array.isArray(findings)) throw new Error('archive.privacy-report-invalid');
  const rules = new Map();
  const files = new Set();
  const contexts = new Map();
  const locations = [];
  for (const finding of findings) {
    if (typeof finding.RuleID !== 'string' || typeof finding.File !== 'string'
      || finding.Secret !== 'REDACTED' || typeof finding.Match !== 'string' || !finding.Match.includes('REDACTED')) {
      throw new Error('archive.privacy-report-not-redacted');
    }
    rules.set(finding.RuleID, (rules.get(finding.RuleID) ?? 0) + 1);
    files.add(finding.File);
    // Classify context for review, never auto-approve a finding based on a field name.
    const context = finding.Match.startsWith('idempotencyKey') ? 'idempotency-key'
      : finding.Match.startsWith('Sec-WebSocket-Key') ? 'websocket-handshake'
      : finding.Match.startsWith('API baseline SHA-256') ? 'api-baseline-digest' : 'other';
    contexts.set(context, (contexts.get(context) ?? 0) + 1);
    locations.push({ rule: finding.RuleID, file: finding.File, line: finding.StartLine, column: finding.StartColumn,
      endLine: finding.EndLine, endColumn: finding.EndColumn, context });
  }
  return { version: 1, scanner: { name: 'gitleaks', version: scannerVersion }, status: 'pending-human-review',
    findings: findings.length, files: files.size, rules: Object.fromEntries(rules), contexts: Object.fromEntries(contexts), locations,
    limitations: ['Pattern scanning does not establish source-reference completeness or public disclosure approval.',
      'Image contents and unsupported binary encodings require separate review.'] };
}

export async function auditDemoPrivacy(directory, reportPath, executable = process.env.GITLEAKS_BIN || 'gitleaks') {
  const temporary = await mkdtemp(join(tmpdir(), 'gold-band-privacy-'));
  try {
    const version = (await execute(executable, ['version'], { windowsHide: true })).stdout.trim();
    const config = join(temporary, 'gitleaks.toml');
    await writeFile(config, '[extend]\nuseDefault = true\n');
    const aliases = await archiveAliases(directory, temporary);
    const aliasesByTarget = new Map(aliases.map(alias => [alias.target.replaceAll('\\', '/'), alias]));
    const findings = [];
    const reportHash = createHash('sha256');
    const targets = [resolve(directory), ...(aliases.length ? [join(temporary, 'archive-resources')] : [])];
    for (const [index, target] of targets.entries()) {
      const rawReport = join(temporary, `redacted-${index}.json`);
      try {
        await execute(executable, ['dir', target, '--config', config, '--gitleaks-ignore-path', join(temporary, 'no-ignores'),
        '--redact=100', '--no-banner', '--no-color', '--ignore-gitleaks-allow', '--report-format', 'json',
        '--max-archive-depth', String(PRIVACY_SCAN_LIMITS.archiveDepth), '--max-decode-depth', String(PRIVACY_SCAN_LIMITS.decodeDepth),
        '--timeout', String(PRIVACY_SCAN_LIMITS.timeoutSeconds),
        '--report-path', rawReport, '--log-level', 'error'], { windowsHide: true, maxBuffer: 1024 * 1024 });
      } catch (error) {
        if (error.code !== 1) throw new Error('archive.privacy-scanner-failed');
      }
      const raw = await readFile(rawReport, 'utf8');
      reportHash.update(raw);
      const parsed = JSON.parse(raw);
      // Preserve member locators while removing temporary scan aliases from evidence.
      for (const finding of parsed) {
        if (index !== 0) {
          const file = finding.File.replaceAll('\\', '/');
          const memberBoundary = file.indexOf('!', target.replaceAll('\\', '/').length + 1);
          const alias = aliasesByTarget.get(memberBoundary === -1 ? file : file.slice(0, memberBoundary));
          if (!alias) throw new Error('archive.privacy-alias-missing');
          finding.File = alias.source.replaceAll('\\', '/') + file.slice(alias.target.replaceAll('\\', '/').length);
        }
        findings.push(finding);
      }
    }
    const report = summarizePrivacy(findings, version);
    report.scanner.archiveAliases = aliases.length;
    report.scanner.limits = { ...PRIVACY_SCAN_LIMITS };
    report.limitations.push('Archive and decoding traversal is bounded by the recorded depth; unsupported, damaged or deeper content requires separate review.');
    report.scannerReportSha256 = reportHash.digest('hex');
    await writeFile(reportPath, JSON.stringify(report, null, 2));
    return { ...report, locations: undefined };
  } finally { await rm(temporary, { recursive: true, force: true }); }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (!process.argv[2] || !process.argv[3]) throw new Error('Usage: node scripts/audit-demo-privacy.mjs <archive-directory> <report-path>');
  console.log(JSON.stringify(await auditDemoPrivacy(process.argv[2], process.argv[3]), null, 2));
}
