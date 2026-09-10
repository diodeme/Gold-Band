import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash, randomBytes } from 'node:crypto';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { zipSync } from 'fflate';
import { auditDemoPrivacy, summarizePrivacy } from './audit-demo-privacy.mjs';

const finding = { RuleID: 'generic-api-key', File: 'assets/source.json', StartLine: 1, EndLine: 1,
  StartColumn: 10, EndColumn: 60, Secret: 'REDACTED', Match: 'idempotencyKey:"REDACTED"' };

test('summarizes findings without persisting match values or treating false-positive candidates as approved', () => {
  const report = summarizePrivacy([finding, finding, { ...finding, File: 'resources/second', Match: 'token=REDACTED' }], '8.30.1');
  assert.equal(report.findings, 3);
  assert.equal(report.files, 2);
  assert.equal(report.contexts['idempotency-key'], 2);
  assert.equal(report.status, 'pending-human-review');
  assert.equal(JSON.stringify(report).includes('Secret'), false);
  assert.equal(JSON.stringify(report).includes('Match'), false);
  assert.deepEqual(report.locations[0], { rule: finding.RuleID, file: finding.File, line: 1, column: 10, endLine: 1, endColumn: 60, context: 'idempotency-key' });
});

test('rejects scanner output that has not been redacted', () => {
  assert.throws(() => summarizePrivacy([{ ...finding, Secret: 'synthetic-test-secret' }], '8.30.1'), /not-redacted/);
  assert.throws(() => summarizePrivacy([{ ...finding, Match: 'token=synthetic-test-secret' }], '8.30.1'), /not-redacted/);
});

test('zero findings remains pending review because pattern scanning does not prove safe disclosure', () => {
  assert.equal(summarizePrivacy([], '8.30.1').status, 'pending-human-review');
});

test('scanner integration detects a synthetic token without copying it into the audit report', { skip: !process.env.GITLEAKS_BIN }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'gold-band-privacy-test-'));
  try {
    const token = `ghp_${randomBytes(18).toString('hex')}`;
    await writeFile(join(directory, 'fixture.txt'), `GITHUB_TOKEN=${token} # gitleaks:allow\n`);
    const reportPath = join(directory, 'audit.json');
    const report = await auditDemoPrivacy(directory, reportPath);
    assert(report.findings > 0);
    assert.equal((await readFile(reportPath, 'utf8')).includes(token), false);
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('scanner integration inspects content-addressed nested archives using source format metadata', { skip: !process.env.GITLEAKS_BIN }, async () => {
  const directory = await mkdtemp(join(tmpdir(), 'gold-band-privacy-test-'));
  try {
    const token = `ghp_${randomBytes(18).toString('hex')}`;
    const bytes = zipSync({ 'nested.zip': zipSync({ 'secret.txt': Buffer.from(`GITHUB_TOKEN=${token}\n`) }) });
    const sha256 = createHash('sha256').update(bytes).digest('hex');
    const path = `resources/${sha256}`;
    await mkdir(join(directory, 'resources'));
    await writeFile(join(directory, path), bytes);
    await writeFile(join(directory, 'manifest.json'), JSON.stringify({ resources: [{ path, sha256, bytes: bytes.length, source: 'attachments/package.jar' }] }));
    const reportPath = join(directory, 'audit.json');
    const report = await auditDemoPrivacy(directory, reportPath);
    assert(report.findings > 0, 'nested resource must be scanned');
    const output = await readFile(reportPath, 'utf8');
    assert.equal(output.includes(token), false);
    assert(JSON.parse(output).locations.some(location => location.file.replaceAll('\\', '/').includes(`${path}!nested.zip!secret.txt`)));
    assert.equal(report.scanner.archiveAliases, 1);
    await writeFile(join(directory, path), Buffer.from('changed after export'));
    await assert.rejects(auditDemoPrivacy(directory, reportPath), /privacy-resource-changed/);
  } finally { await rm(directory, { recursive: true, force: true }); }
});
