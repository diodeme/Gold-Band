import { createHash } from 'node:crypto';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import npa from 'npm-package-arg';
import semver from 'semver';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const REGISTRY = 'https://registry.npmjs.org';
const NODE_RELEASES = 'https://nodejs.org/dist/index.json';
const NETWORK_TIMEOUT_MS = 20_000;
const METADATA_CONCURRENCY = 4;

export function compatibilityTargets(catalog, policy) {
  const targets = catalog.agents.filter((agent) => agent.command === 'npx').map((agent) => {
    const spec = agent.args.find((arg) => !['-y', '--yes'].includes(arg));
    const parsed = npa(spec);
    if (parsed.type !== 'version' || !parsed.name || !semver.valid(parsed.rawSpec)) {
      throw new Error(`Exact npm package version required: ${agent.id}`);
    }
    return { id: agent.id, package: parsed.name, version: parsed.rawSpec };
  });
  return [...targets, ...policy.npm.matrixVersions.map((version) => ({
    id: `npm-${version}`, package: 'npm', version,
  }))];
}

function platformAllowed(rule, value) {
  if (rule === undefined) return null;
  const values = Array.isArray(rule) ? rule : [rule];
  if (values.includes(`!${value}`)) return false;
  return values.includes('any') || values.includes(value) || values.every((item) => item.startsWith('!'));
}

export function assessMetadata(target, metadata, policy) {
  if (metadata.name !== target.package || metadata.version !== target.version) {
    throw new Error(`Registry identity mismatch: ${target.id}`);
  }
  const nodeRange = metadata.engines?.node;
  if (nodeRange !== undefined && !semver.validRange(nodeRange)) {
    throw new Error(`Invalid Node engines range: ${target.id}`);
  }
  return {
    ...target,
    engines: metadata.engines ?? null,
    os: metadata.os ?? null,
    cpu: metadata.cpu ?? null,
    nodeCandidates: Object.fromEntries(policy.nodeMatrixVersions.map((version) => [
      version, nodeRange === undefined ? 'unknown' : semver.satisfies(version, nodeRange) ? 'declared-compatible' : 'incompatible',
    ])),
    platforms: policy.platforms.map(({ os, cpu }) => ({
      os, cpu,
      declaration: platformAllowed(metadata.os, os) === false || platformAllowed(metadata.cpu, cpu) === false
        ? 'excluded' : metadata.os === undefined && metadata.cpu === undefined ? 'unspecified' : 'not-excluded',
      runtime: 'not-tested',
    })),
    // Declarations cannot establish native/transitive dependency or authenticated runtime support.
    runtime: 'not-tested',
  };
}

export async function collectCompatibility(catalog, policy, fetchJson, checkedAt = new Date().toISOString()) {
  const targets = compatibilityTargets(catalog, policy);
  const results = [];
  for (let offset = 0; offset < targets.length; offset += METADATA_CONCURRENCY) {
    results.push(...await Promise.all(targets.slice(offset, offset + METADATA_CONCURRENCY).map(async (target) => {
      const source = `${REGISTRY}/${encodeURIComponent(target.package)}/${target.version}`;
      try {
        const metadata = await fetchJson(source);
        return { ...assessMetadata(target, metadata, policy), source };
      } catch (error) {
        return { ...target, source, error: error.message, runtime: 'not-tested' };
      }
    })));
  }
  let recommendedNode;
  try {
    const releases = await fetchJson(NODE_RELEASES);
    const release = releases.find((entry) => entry.version === `v${policy.node.recommendedVersion}`);
    if (!release?.lts) throw new Error('Configured recommended Node release is not a published LTS release');
    if (!policy.npm.matrixVersions.includes(release.npm)) throw new Error('Bundled npm is missing from the compatibility matrix');
    recommendedNode = { source: NODE_RELEASES, version: release.version, npm: release.npm, lts: release.lts, date: release.date, files: release.files };
  } catch (error) {
    recommendedNode = { source: NODE_RELEASES, error: error.message };
  }
  return {
    checkedAt,
    policyVersion: policy.version,
    policyStatus: policy.status,
    catalogFingerprint: createHash('sha256').update(JSON.stringify(catalog)).digest('hex'),
    scope: 'Published top-level metadata only. Unknown engines and untested runtime/platform combinations remain unverified.',
    recommendedNode,
    results,
    nativeAgents: catalog.agents.filter((agent) => agent.command !== 'npx').map(({ id, command, version }) => ({ id, command, version, runtime: 'not-tested' })),
  };
}

export function reportHasErrors(report) {
  return Boolean(report.recommendedNode.error) || report.results.some((result) => result.error
    || Object.values(result.nodeCandidates ?? {}).includes('incompatible'));
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const [outputArg, ...extra] = process.argv.slice(2);
  if (extra.length || !outputArg) throw new Error('Usage: node scripts/onboarding-compatibility.mjs <report.json>');
  const [catalog, policy] = await Promise.all([
    readFile(resolve(root, 'resources/agent-catalog.json'), 'utf8').then(JSON.parse),
    readFile(resolve(root, 'configs/onboarding-environment-policy.json'), 'utf8').then(JSON.parse),
  ]);
  const report = await collectCompatibility(catalog, policy, async (url) => {
    const response = await fetch(url, { signal: AbortSignal.timeout(NETWORK_TIMEOUT_MS) });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
  });
  const output = resolve(outputArg);
  await mkdir(dirname(output), { recursive: true });
  await writeFile(output, `${JSON.stringify(report, null, 2)}\n`);
  console.log(JSON.stringify({ output, packages: report.results.length, errors: reportHasErrors(report), runtimeVerified: false }));
  if (reportHasErrors(report)) process.exitCode = 1;
}
