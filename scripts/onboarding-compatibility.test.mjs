import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { assessMetadata, collectCompatibility, compatibilityTargets, reportHasErrors } from './onboarding-compatibility.mjs';

const catalog = JSON.parse(await readFile(new URL('../resources/agent-catalog.json', import.meta.url), 'utf8'));
const policy = JSON.parse(await readFile(new URL('../configs/onboarding-environment-policy.json', import.meta.url), 'utf8'));
const target = { id: 'fixture', package: 'fixture', version: '1.0.0' };
const manifest = { name: 'fixture', version: '1.0.0' };

test('matrix covers every current npm catalog entry including Gemini, without executing packages', () => {
  const targets = compatibilityTargets(catalog, policy);
  assert.deepEqual(targets.filter((entry) => entry.package !== 'npm').map((entry) => entry.id), catalog.agents.filter((entry) => entry.command === 'npx').map((entry) => entry.id));
  assert.ok(targets.some((entry) => entry.id === 'gemini'));
  assert.equal(new Set(targets.map((entry) => entry.id)).size, targets.length);
});

test('unknown engines remain unknown; declarations never become runtime results', () => {
  const result = assessMetadata(target, manifest, policy);
  assert.ok(Object.values(result.nodeCandidates).every((status) => status === 'unknown'));
  assert.equal(result.runtime, 'not-tested');
  assert.ok(result.platforms.every((entry) => entry.runtime === 'not-tested'));
});

test('npm semver syntax, platform exclusions, and exact metadata identity are enforced', () => {
  const result = assessMetadata(target, { ...manifest, engines: { node: '^22.20.0 || >=24.0.0 <25' }, os: ['!win32'], cpu: ['x64'] }, policy);
  assert.ok(Object.values(result.nodeCandidates).every((status) => status === 'declared-compatible'));
  assert.ok(result.platforms.filter((entry) => entry.os === 'win32' || entry.cpu === 'arm64').every((entry) => entry.declaration === 'excluded'));
  assert.throws(() => assessMetadata(target, { ...manifest, version: '2.0.0' }, policy), /identity mismatch/);
  assert.throws(() => assessMetadata(target, { ...manifest, engines: { node: 'broken' } }, policy), /Invalid Node/);
  assert.throws(() => compatibilityTargets({ agents: [{ id: 'fixture', command: 'npx', args: ['-y', 'fixture@latest'] }] }, policy), /Exact npm/);
});

test('one failed fetch preserves the rest, requests stay bounded, and recommendation is checked', async () => {
  let active = 0;
  let peak = 0;
  const targets = compatibilityTargets(catalog, policy);
  const report = await collectCompatibility(catalog, policy, async (url) => {
    active++;
    peak = Math.max(peak, active);
    try {
      await new Promise((resolve) => setTimeout(resolve, 1));
      if (url.includes('nodejs.org')) return [{ version: `v${policy.node.recommendedVersion}`, npm: policy.npm.matrixVersions.at(-1), lts: 'fixture' }];
      if (url.includes('gemini')) throw new Error('offline');
      const target = targets.find((entry) => url.endsWith(`/${encodeURIComponent(entry.package)}/${entry.version}`));
      return { name: target.package, version: target.version, engines: { node: '>=20' } };
    } finally { active--; }
  }, '2026-10-20T00:00:00.000Z');
  assert.ok(peak <= 4);
  assert.equal(report.results.length, targets.length);
  assert.equal(report.results.filter((result) => result.error).length, 1);
  assert.equal(reportHasErrors(report), true);
  assert.ok(report.nativeAgents.every((entry) => entry.runtime === 'not-tested'));
});
