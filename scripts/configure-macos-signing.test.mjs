import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const script = path.resolve('scripts/configure-macos-signing.mjs');
const sourceNames = [
  'MACOS_APPLE_CERTIFICATE',
  'MACOS_APPLE_CERTIFICATE_PASSWORD',
  'MACOS_APPLE_SIGNING_IDENTITY',
  'MACOS_APPLE_ID',
  'MACOS_APPLE_PASSWORD',
  'MACOS_APPLE_TEAM_ID',
];

function run(values = {}) {
  const directory = mkdtempSync(path.join(tmpdir(), 'gold-band-signing-'));
  const githubEnv = path.join(directory, 'github.env');
  const env = { ...process.env, GITHUB_ENV: githubEnv };
  sourceNames.forEach((name) => delete env[name]);
  Object.assign(env, values);
  const result = spawnSync(process.execPath, [script], { env, encoding: 'utf8' });
  const output = result.status === 0 ? readFileSync(githubEnv, 'utf8') : '';
  return { ...result, output };
}

test('uses only the ad-hoc identity when Apple credentials are absent', () => {
  const result = run();
  assert.equal(result.status, 0);
  assert.equal(result.output, 'APPLE_SIGNING_IDENTITY=-\n');
  assert.doesNotMatch(result.output, /APPLE_ID|APPLE_PASSWORD|APPLE_TEAM_ID/);
});

test('rejects a partial Apple credential set', () => {
  const result = run({ MACOS_APPLE_CERTIFICATE: 'certificate' });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Incomplete Apple credentials/);
  assert.match(result.stderr, /APPLE_TEAM_ID/);
});

test('exports a complete Apple credential set for the same build step', () => {
  const values = Object.fromEntries(sourceNames.map((name) => [name, `${name}-value`]));
  const result = run(values);
  assert.equal(result.status, 0);
  for (const target of [
    'APPLE_CERTIFICATE',
    'APPLE_CERTIFICATE_PASSWORD',
    'APPLE_SIGNING_IDENTITY',
    'APPLE_ID',
    'APPLE_PASSWORD',
    'APPLE_TEAM_ID',
  ]) {
    assert.match(result.output, new RegExp(`${target}<<GOLD_BAND_`));
  }
  assert.doesNotMatch(result.output, /APPLE_SIGNING_IDENTITY=-/);
});
